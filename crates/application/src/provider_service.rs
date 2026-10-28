//! Provider configuration (local providers; remote providers and authorization arrive with
//! US7, T136).

use std::sync::Arc;

use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{Locality, Provider, ProviderRole, ProviderSpec};

use crate::error::ApplicationError;
use crate::ports::{ProviderRepository, SettingsRepository};

fn default_key(role: ProviderRole) -> String {
    format!("default_provider.{}", role.code())
}

/// A provider plus whether it is the default for its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEntry {
    /// The provider.
    pub provider: Provider,
    /// Default for its role.
    pub is_default: bool,
}

/// Lists, configures and resolves providers.
pub struct ProviderService {
    providers: Arc<dyn ProviderRepository>,
    settings: Arc<dyn SettingsRepository>,
}

impl ProviderService {
    /// Creates the service.
    #[must_use]
    pub fn new(
        providers: Arc<dyn ProviderRepository>,
        settings: Arc<dyn SettingsRepository>,
    ) -> Self {
        Self {
            providers,
            settings,
        }
    }

    /// Every provider with its default flag.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when a repository fails.
    pub fn list(&self) -> Result<Vec<ProviderEntry>, ApplicationError> {
        let transcription = self.default_id(ProviderRole::Transcription)?;
        let analysis = self.default_id(ProviderRole::Analysis)?;
        Ok(self
            .providers
            .list()?
            .into_iter()
            .map(|provider| {
                let default = match provider.role() {
                    ProviderRole::Transcription => transcription,
                    ProviderRole::Analysis => analysis,
                };
                ProviderEntry {
                    is_default: default == Some(provider.id()),
                    provider,
                }
            })
            .collect())
    }

    /// Creates or replaces a local provider. If an existing provider changes role, it stops
    /// being the default of its former role.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] for invalid specs or remote adapters (until US7);
    /// [`ApplicationError::Storage`] when a repository fails.
    pub fn upsert_local(&self, spec: ProviderSpec) -> Result<Provider, ApplicationError> {
        if spec.adapter.locality() != Locality::Local {
            return Err(ApplicationError::InvalidInput {
                field: "adapter",
                problem: "remote_requires_authorization_flow",
            });
        }
        let provider = Provider::new(spec)?;
        match self.providers.get(provider.id()) {
            Ok(previous) if previous.role() != provider.role() => {
                if self.default_id(previous.role())? == Some(provider.id()) {
                    self.settings.set(&default_key(previous.role()), "")?;
                }
            }
            Ok(_) | Err(crate::ports::RepositoryError::NotFound) => {}
            Err(other) => return Err(other.into()),
        }
        self.providers.upsert(&provider)?;
        Ok(provider)
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

    /// Usable transcription provider: the default when usable, otherwise the first usable one
    /// (FR-045a).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NoTranscriber`] when none is usable.
    pub fn resolve_transcriber(&self) -> Result<Provider, ApplicationError> {
        self.resolve(ProviderRole::Transcription)?
            .ok_or(ApplicationError::NoTranscriber)
    }

    /// Usable analysis provider, if any (analysis is optional for capture).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when a repository fails.
    pub fn resolve_analyzer(&self) -> Result<Option<Provider>, ApplicationError> {
        self.resolve(ProviderRole::Analysis)
    }

    fn resolve(&self, role: ProviderRole) -> Result<Option<Provider>, ApplicationError> {
        let default = self.default_id(role)?;
        let usable: Vec<Provider> = self
            .providers
            .list()?
            .into_iter()
            .filter(|p| p.role() == role && is_usable(p))
            .collect();
        let chosen = default
            .and_then(|id| usable.iter().find(|p| p.id() == id).cloned())
            .or_else(|| usable.into_iter().next());
        Ok(chosen)
    }

    fn default_id(&self, role: ProviderRole) -> Result<Option<ProviderId>, ApplicationError> {
        Ok(self
            .settings
            .get(&default_key(role))?
            .and_then(|raw| ProviderId::parse(&raw).ok()))
    }
}

/// Local providers are usable as configured; remote ones need an authorization (US7).
fn is_usable(provider: &Provider) -> bool {
    provider.locality() == Locality::Local
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind};

    use super::*;
    use crate::ports::RepositoryError;
    use crate::testing::{InMemoryProviderRepository, InMemorySettingsRepository};

    struct Fixture {
        service: ProviderService,
        repo: Arc<InMemoryProviderRepository>,
    }

    fn fixture() -> Fixture {
        let repo = Arc::new(InMemoryProviderRepository::default());
        let settings = Arc::new(InMemorySettingsRepository::default());
        Fixture {
            service: ProviderService::new(repo.clone(), settings),
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
    fn remote_adapters_are_rejected_until_authorization_exists() {
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
