//! Provider configuration, remote authorization and the [`RemoteGate`] (FR-045–FR-050,
//! research R9).
//!
//! Resolution rules:
//! - The default provider of a role is used when it is usable: local, or remote with a granted
//!   authorization covering its configuration.
//! - Otherwise only local providers are picked, so a failing or unauthorized default never
//!   moves the stage to another remote provider (FR-048). A remote provider other than the
//!   default is picked only when no default is set.
//! - When nothing is usable but a remote provider of the role is configured without
//!   authorization, the error is `authorization_required` naming it, so the UI can show the
//!   authorization dialog (US7.2).
//!
//! Changing a remote provider's adapter, endpoint or categories revokes its authorization.
//! API keys go to the OS credential store; the provider keeps only the reference (FR-049).

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_domain::Redacted;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::language::UiLanguage;
use devcopilot_domain::provider::{
    AuthorizationState, AuthorizationStatus, CredentialRef, DataCategory, Locality, Provider,
    ProviderAuthorization, ProviderRole, ProviderSpec, authorization_status,
};

use crate::error::ApplicationError;
use crate::ports::{
    AuthorizationError, Clock, ProviderRepository, RemoteGate, RepositoryError, SecretStore,
    SettingsRepository,
};

/// Longest accepted API key, in bytes.
pub const SECRET_MAX_BYTES: usize = 8 * 1024;

fn default_key(role: ProviderRole) -> String {
    format!("default_provider.{}", role.code())
}

/// A provider plus its default flag and authorization status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEntry {
    /// The provider.
    pub provider: Provider,
    /// Default for its role.
    pub is_default: bool,
    /// Authorization status (always `not_required` for local providers).
    pub authorization: AuthorizationStatus,
}

/// What the authorization dialog shows before the user accepts (FR-046).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationPreview {
    /// Provider to authorize.
    pub provider_id: ProviderId,
    /// Host (and explicit port) the data goes to.
    pub destination: String,
    /// Data the provider receives.
    pub data_categories: BTreeSet<DataCategory>,
    /// Known retention implications, in the UI language.
    pub retention_notice: String,
}

/// Retention implications shown before authorizing a remote provider. Dev Copilot cannot know
/// a third party's actual retention, so the notice says what is known: the destination's own
/// terms apply and revocation does not delete what was already sent.
#[must_use]
pub fn retention_notice(destination: &str, language: UiLanguage) -> String {
    match language {
        UiLanguage::PtBr => format!(
            "Os dados enviados a {destination} são processados e podem ser retidos conforme os \
             termos e a política de retenção desse serviço, que o Dev Copilot não controla. \
             Consulte esses termos antes de autorizar. Você pode revogar esta autorização a \
             qualquer momento: a revogação impede novos envios, mas não apaga o que o serviço \
             já recebeu."
        ),
        UiLanguage::En => format!(
            "Data sent to {destination} is processed and may be retained under that service's \
             own terms and retention policy, which Dev Copilot does not control. Review those \
             terms before authorizing. You can revoke this authorization at any time: \
             revocation stops new sends but does not delete what the service already received."
        ),
    }
}

/// Lists, configures, authorizes and resolves providers.
pub struct ProviderService {
    providers: Arc<dyn ProviderRepository>,
    settings: Arc<dyn SettingsRepository>,
    secrets: Arc<dyn SecretStore>,
    clock: Arc<dyn Clock>,
}

impl ProviderService {
    /// Creates the service.
    #[must_use]
    pub fn new(
        providers: Arc<dyn ProviderRepository>,
        settings: Arc<dyn SettingsRepository>,
        secrets: Arc<dyn SecretStore>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            providers,
            settings,
            secrets,
            clock,
        }
    }

    /// The network enforcement point for remote adapters, reading authorizations from storage
    /// on every check so revocation applies to the next request (FR-047).
    #[must_use]
    pub fn gate(&self) -> Arc<dyn RemoteGate> {
        Arc::new(ProviderGate {
            providers: Arc::clone(&self.providers),
        })
    }

    /// Every provider with its default flag and authorization status.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when a repository fails.
    pub fn list(&self) -> Result<Vec<ProviderEntry>, ApplicationError> {
        let transcription = self.default_id(ProviderRole::Transcription)?;
        let analysis = self.default_id(ProviderRole::Analysis)?;
        self.providers
            .list()?
            .into_iter()
            .map(|provider| {
                let default = match provider.role() {
                    ProviderRole::Transcription => transcription,
                    ProviderRole::Analysis => analysis,
                };
                Ok(ProviderEntry {
                    is_default: default == Some(provider.id()),
                    authorization: self.status(&provider)?,
                    provider,
                })
            })
            .collect()
    }

    /// One provider with its default flag and authorization status.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn entry(&self, id: ProviderId) -> Result<ProviderEntry, ApplicationError> {
        self.list()?
            .into_iter()
            .find(|entry| entry.provider.id() == id)
            .ok_or(ApplicationError::NotFound)
    }

    /// Creates or replaces a local provider.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] for invalid specs or remote adapters; see
    /// [`Self::upsert`].
    pub fn upsert_local(&self, spec: ProviderSpec) -> Result<Provider, ApplicationError> {
        if spec.adapter.locality() != Locality::Local {
            return Err(ApplicationError::InvalidInput {
                field: "adapter",
                problem: "inconsistent",
            });
        }
        self.upsert(spec)
    }

    /// Creates or replaces a provider. An existing provider keeps its stored API key; if it
    /// changes role it stops being the default of its former role; if its adapter, endpoint or
    /// categories change, its authorization is revoked (a new one is required).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] for invalid specs (remote endpoints must be
    /// `https`); [`ApplicationError::Storage`] when a repository fails.
    pub fn upsert(&self, spec: ProviderSpec) -> Result<Provider, ApplicationError> {
        let mut provider = Provider::new(spec)?;
        match self.providers.get(provider.id()) {
            Ok(previous) => {
                if provider.credential_ref().is_none()
                    && let Some(reference) = previous.credential_ref()
                {
                    provider = provider.with_credential_ref(reference.clone());
                }
                if previous.role() != provider.role()
                    && self.default_id(previous.role())? == Some(provider.id())
                {
                    self.settings.set(&default_key(previous.role()), "")?;
                }
                if previous.changes_destination_or_data(&provider) {
                    self.revoke_if_granted(provider.id())?;
                }
            }
            Err(RepositoryError::NotFound) => {}
            Err(other) => return Err(other.into()),
        }
        self.providers.upsert(&provider)?;
        Ok(provider)
    }

    /// Stores the API key of a remote provider in the OS credential store. The secret never
    /// reaches the database, logs or events (FR-049).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] on `secret` when empty or too long and on
    /// `provider_id` for local providers; [`ApplicationError::NotFound`];
    /// [`ApplicationError::SecretStoreUnavailable`].
    pub fn set_secret(
        &self,
        id: ProviderId,
        secret: Redacted<Vec<u8>>,
    ) -> Result<(), ApplicationError> {
        let provider = self.providers.get(id)?;
        if provider.locality() != Locality::Remote {
            return Err(ApplicationError::InvalidInput {
                field: "provider_id",
                problem: "inconsistent",
            });
        }
        let bytes = secret.expose();
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Err(ApplicationError::InvalidInput {
                field: "secret",
                problem: "empty",
            });
        }
        if bytes.len() > SECRET_MAX_BYTES {
            return Err(ApplicationError::InvalidInput {
                field: "secret",
                problem: "too_long",
            });
        }
        let reference = provider
            .credential_ref()
            .cloned()
            .unwrap_or_else(|| CredentialRef::for_provider(id));
        self.secrets.set(&reference, &secret)?;
        if provider.credential_ref().is_none() {
            self.providers
                .upsert(&provider.with_credential_ref(reference))?;
        }
        tracing::info!(provider = %id, "provider credential stored");
        Ok(())
    }

    /// What the user must see before authorizing `id`.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] on `provider_id` for local providers;
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn authorization_preview(
        &self,
        id: ProviderId,
    ) -> Result<AuthorizationPreview, ApplicationError> {
        let provider = self.providers.get(id)?;
        let destination = provider
            .endpoint()
            .filter(|_| provider.locality() == Locality::Remote)
            .map(|endpoint| endpoint.destination())
            .ok_or(ApplicationError::InvalidInput {
                field: "provider_id",
                problem: "inconsistent",
            })?;
        let language = self
            .settings
            .get("ui_language")?
            .and_then(|code| UiLanguage::parse(&code).ok())
            .unwrap_or(UiLanguage::PtBr);
        Ok(AuthorizationPreview {
            provider_id: id,
            retention_notice: retention_notice(&destination, language),
            destination,
            data_categories: provider.data_categories().clone(),
        })
    }

    /// Grants `id` exactly what [`Self::authorization_preview`] shows; called only after the
    /// user accepted it (FR-046).
    ///
    /// # Errors
    ///
    /// As [`Self::authorization_preview`].
    pub fn authorize(&self, id: ProviderId) -> Result<ProviderAuthorization, ApplicationError> {
        let preview = self.authorization_preview(id)?;
        let provider = self.providers.get(id)?;
        let authorization =
            ProviderAuthorization::grant(&provider, preview.retention_notice, self.clock.now())?;
        self.providers.save_authorization(&authorization)?;
        tracing::info!(provider = %id, "remote provider authorized");
        Ok(authorization)
    }

    /// Revokes the authorization of `id`; no new request is sent from now on (FR-047).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] without a granted authorization;
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn revoke(&self, id: ProviderId) -> Result<ProviderAuthorization, ApplicationError> {
        self.providers.get(id)?;
        self.revoke_if_granted(id)?
            .ok_or_else(|| ApplicationError::InvalidState("provider is not authorized".into()))
    }

    /// Makes `id` the default provider for `role`.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`] for unknown providers;
    /// [`ApplicationError::InvalidInput`] when the provider has another role.
    pub fn set_default(&self, role: ProviderRole, id: ProviderId) -> Result<(), ApplicationError> {
        let provider = self.providers.get(id)?;
        if provider.role() != role {
            return Err(ApplicationError::InvalidInput {
                field: "role",
                problem: "inconsistent",
            });
        }
        self.settings.set(&default_key(role), &id.to_string())?;
        Ok(())
    }

    /// Usable transcription provider (FR-045a); see the module rules.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::AuthorizationRequired`] when only unauthorized remote providers are
    /// configured; [`ApplicationError::NoTranscriber`] when none is usable.
    pub fn resolve_transcriber(&self) -> Result<Provider, ApplicationError> {
        self.resolve(ProviderRole::Transcription)?
            .ok_or(ApplicationError::NoTranscriber)
    }

    /// Usable analysis provider, if any (analysis is optional for capture).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::AuthorizationRequired`] when only unauthorized remote providers are
    /// configured; [`ApplicationError::Storage`] when a repository fails.
    pub fn resolve_analyzer(&self) -> Result<Option<Provider>, ApplicationError> {
        self.resolve(ProviderRole::Analysis)
    }

    /// Local provider of `role` to take over when an authorized remote one fails (FR-045b):
    /// the default when it is local, otherwise the first local one.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when a repository fails.
    pub fn resolve_local_fallback(
        &self,
        role: ProviderRole,
    ) -> Result<Option<Provider>, ApplicationError> {
        let default = self.default_id(role)?;
        let locals: Vec<Provider> = self
            .providers
            .list()?
            .into_iter()
            .filter(|p| p.role() == role && p.locality() == Locality::Local)
            .collect();
        Ok(default
            .and_then(|id| locals.iter().find(|p| p.id() == id).cloned())
            .or_else(|| locals.into_iter().next()))
    }

    fn resolve(&self, role: ProviderRole) -> Result<Option<Provider>, ApplicationError> {
        let default = self.default_id(role)?;
        let configured: Vec<Provider> = self
            .providers
            .list()?
            .into_iter()
            .filter(|p| p.role() == role)
            .collect();
        let mut unauthorized = None;
        let mut usable = Vec::with_capacity(configured.len());
        for provider in configured {
            if self.is_usable(&provider)? {
                usable.push(provider);
            } else if unauthorized.is_none() || Some(provider.id()) == default {
                unauthorized = Some(provider.id());
            }
        }
        let chosen = default
            .and_then(|id| usable.iter().find(|p| p.id() == id).cloned())
            .or_else(|| {
                usable
                    .iter()
                    .find(|p| p.locality() == Locality::Local)
                    .cloned()
            })
            .or_else(|| {
                default
                    .is_none()
                    .then(|| usable.into_iter().next())
                    .flatten()
            });
        match (chosen, unauthorized) {
            (Some(provider), _) => Ok(Some(provider)),
            (None, Some(provider)) => Err(ApplicationError::AuthorizationRequired {
                provider: Some(provider),
            }),
            (None, None) => Ok(None),
        }
    }

    /// Local providers are usable as configured; remote ones need a covering authorization.
    fn is_usable(&self, provider: &Provider) -> Result<bool, ApplicationError> {
        Ok(matches!(
            self.status(provider)?,
            AuthorizationStatus::NotRequired | AuthorizationStatus::Authorized
        ))
    }

    fn status(&self, provider: &Provider) -> Result<AuthorizationStatus, ApplicationError> {
        if provider.locality() == Locality::Local {
            return Ok(AuthorizationStatus::NotRequired);
        }
        let latest = self.providers.latest_authorization(provider.id())?;
        Ok(authorization_status(provider, latest.as_ref()))
    }

    fn revoke_if_granted(
        &self,
        id: ProviderId,
    ) -> Result<Option<ProviderAuthorization>, ApplicationError> {
        let Some(mut latest) = self
            .providers
            .latest_authorization(id)?
            .filter(|auth| auth.state() == AuthorizationState::Granted)
        else {
            return Ok(None);
        };
        latest.revoke(self.clock.now())?;
        self.providers.save_authorization(&latest)?;
        tracing::info!(provider = %id, "remote provider authorization revoked");
        Ok(Some(latest))
    }

    fn default_id(&self, role: ProviderRole) -> Result<Option<ProviderId>, ApplicationError> {
        Ok(self
            .settings
            .get(&default_key(role))?
            .and_then(|raw| ProviderId::parse(&raw).ok()))
    }
}

/// [`RemoteGate`] over the provider repository (research R9). Refuses whenever storage
/// cannot confirm a covering, granted authorization.
struct ProviderGate {
    providers: Arc<dyn ProviderRepository>,
}

impl RemoteGate for ProviderGate {
    fn check(
        &self,
        provider: &ProviderId,
        categories: &[DataCategory],
    ) -> Result<(), AuthorizationError> {
        let allowed = self
            .providers
            .get(*provider)
            .ok()
            .is_some_and(|configured| {
                self.providers
                    .latest_authorization(*provider)
                    .ok()
                    .flatten()
                    .is_some_and(|auth| auth.covers(&configured, categories))
            });
        if allowed {
            Ok(())
        } else {
            tracing::warn!(provider = %provider, "remote request blocked: not authorized");
            Err(AuthorizationError::NotAuthorized)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind};

    use super::*;
    use crate::ports::RepositoryError;
    use crate::testing::{
        FakeClock, InMemoryProviderRepository, InMemorySettingsRepository, ToggleableSecretStore,
    };

    struct Fixture {
        service: ProviderService,
        repo: Arc<InMemoryProviderRepository>,
    }

    fn fixture() -> Fixture {
        let repo = Arc::new(InMemoryProviderRepository::default());
        let settings = Arc::new(InMemorySettingsRepository::default());
        Fixture {
            service: ProviderService::new(
                repo.clone(),
                settings,
                Arc::new(ToggleableSecretStore::available()),
                Arc::new(FakeClock::default()),
            ),
            repo,
        }
    }

    fn whisper(id: ProviderId) -> ProviderSpec {
        ProviderSpec {
            id,
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::WhisperLocal,
            endpoint: None,
            model: "ggml-base".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Audio]),
        }
    }

    fn ollama(id: ProviderId) -> ProviderSpec {
        ProviderSpec {
            id,
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Records]),
        }
    }

    #[test]
    fn no_transcriber_until_one_is_configured() {
        let f = fixture();
        assert_eq!(
            f.service.resolve_transcriber(),
            Err(ApplicationError::NoTranscriber)
        );
        assert_eq!(f.service.resolve_analyzer(), Ok(None));
        let id = ProviderId::new();
        f.service.upsert_local(whisper(id)).unwrap();
        assert_eq!(f.service.resolve_transcriber().unwrap().id(), id);
    }

    #[test]
    fn default_wins_over_first_usable() {
        let f = fixture();
        let first = ProviderId::new();
        let second = ProviderId::new();
        f.service.upsert_local(whisper(first)).unwrap();
        f.service.upsert_local(whisper(second)).unwrap();
        f.service
            .set_default(ProviderRole::Transcription, second)
            .unwrap();
        assert_eq!(f.service.resolve_transcriber().unwrap().id(), second);
        let flags: Vec<_> = f
            .service
            .list()
            .unwrap()
            .into_iter()
            .map(|e| (e.provider.id(), e.is_default))
            .collect();
        assert_eq!(flags, [(first, false), (second, true)]);
    }

    #[test]
    fn set_default_checks_existence_and_role() {
        let f = fixture();
        assert_eq!(
            f.service
                .set_default(ProviderRole::Analysis, ProviderId::new()),
            Err(ApplicationError::NotFound)
        );
        let id = ProviderId::new();
        f.service.upsert_local(whisper(id)).unwrap();
        assert_eq!(
            f.service.set_default(ProviderRole::Analysis, id),
            Err(ApplicationError::InvalidInput {
                field: "role",
                problem: "inconsistent"
            })
        );
    }

    #[test]
    fn upsert_local_rejects_remote_adapters() {
        let f = fixture();
        let spec = ProviderSpec {
            adapter: ProviderAdapterKind::Anthropic,
            endpoint: Some("https://api.example.com".into()),
            ..ollama(ProviderId::new())
        };
        assert!(matches!(
            f.service.upsert_local(spec),
            Err(ApplicationError::InvalidInput {
                field: "adapter",
                ..
            })
        ));
        assert!(f.repo.list().unwrap().is_empty());
    }

    #[test]
    fn invalid_spec_maps_to_invalid_input() {
        let f = fixture();
        let spec = ProviderSpec {
            endpoint: Some("http://10.0.0.5:11434".into()),
            ..ollama(ProviderId::new())
        };
        assert_eq!(
            f.service.upsert_local(spec),
            Err(ApplicationError::InvalidInput {
                field: "endpoint",
                problem: "inconsistent"
            })
        );
    }

    #[test]
    fn analyzer_resolves_local_ollama() {
        let f = fixture();
        let id = ProviderId::new();
        f.service.upsert_local(ollama(id)).unwrap();
        assert_eq!(
            f.service.resolve_analyzer().unwrap().map(|p| p.id()),
            Some(id)
        );
    }

    #[test]
    fn storage_failures_surface_as_storage_errors() {
        let f = fixture();
        f.repo
            .fail_with(Some(RepositoryError::Storage("io".into())));
        assert!(matches!(
            f.service.list(),
            Err(ApplicationError::Storage(_))
        ));
    }
}
