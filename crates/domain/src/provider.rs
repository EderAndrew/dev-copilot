//! Transcription and analysis providers (data-model.md "Provider").
//!
//! Remote providers must use `https`; local providers may only point at the loopback interface,
//! so a "local" provider can never send session content off the machine without authorization.

use std::collections::BTreeSet;
use std::fmt;

use crate::error::{DomainError, InputProblem};
use crate::ids::ProviderId;
use crate::time::Timestamp;

/// What a provider is used for. `Analysis` also answers questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderRole {
    /// Speech to text.
    Transcription,
    /// Technical analysis and contextual questions.
    Analysis,
}

/// Where a provider processes data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locality {
    /// On this machine.
    Local,
    /// On a third-party service; requires explicit authorization (FR-046).
    Remote,
}

/// Category of session data a provider receives (FR-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataCategory {
    /// Captured audio.
    Audio,
    /// Transcript text.
    Transcript,
    /// Technical records.
    Records,
    /// User questions.
    Questions,
}

/// Adapter implementing a provider. Vendor details stay in the `intelligence` crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderAdapterKind {
    /// whisper.cpp running in-process.
    WhisperLocal,
    /// HTTP API compatible with OpenAI `/audio/transcriptions`.
    OpenAiCompatibleStt,
    /// Anthropic Messages API.
    Anthropic,
    /// HTTP API compatible with OpenAI Chat Completions.
    OpenAiCompatibleChat,
    /// Ollama server started by the user on this machine.
    Ollama,
}

code_enum!(ProviderRole, "role", {
    Transcription => "transcription",
    Analysis => "analysis",
});
code_enum!(Locality, "locality", {
    Local => "local",
    Remote => "remote",
});
code_enum!(DataCategory, "data_categories", {
    Audio => "audio",
    Transcript => "transcript",
    Records => "records",
    Questions => "questions",
});
code_enum!(ProviderAdapterKind, "adapter", {
    WhisperLocal => "whisper_local",
    OpenAiCompatibleStt => "openai_compatible_stt",
    Anthropic => "anthropic",
    OpenAiCompatibleChat => "openai_compatible_chat",
    Ollama => "ollama",
});

impl ProviderAdapterKind {
    /// Role this adapter serves.
    #[must_use]
    pub const fn role(self) -> ProviderRole {
        match self {
            Self::WhisperLocal | Self::OpenAiCompatibleStt => ProviderRole::Transcription,
            Self::Anthropic | Self::OpenAiCompatibleChat | Self::Ollama => ProviderRole::Analysis,
        }
    }

    /// Where this adapter processes data.
    #[must_use]
    pub const fn locality(self) -> Locality {
        match self {
            Self::WhisperLocal | Self::Ollama => Locality::Local,
            Self::OpenAiCompatibleStt | Self::Anthropic | Self::OpenAiCompatibleChat => {
                Locality::Remote
            }
        }
    }

    /// Whether the adapter talks to an HTTP endpoint.
    #[must_use]
    pub const fn needs_endpoint(self) -> bool {
        !matches!(self, Self::WhisperLocal)
    }
}

impl ProviderRole {
    /// Data categories a provider with this role may receive.
    #[must_use]
    pub fn allowed_categories(self) -> &'static [DataCategory] {
        match self {
            Self::Transcription => &[DataCategory::Audio],
            Self::Analysis => &[
                DataCategory::Transcript,
                DataCategory::Records,
                DataCategory::Questions,
            ],
        }
    }
}

/// URL scheme of an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// Plain HTTP; only allowed for loopback endpoints of local providers.
    Http,
    /// TLS.
    Https,
}

/// A validated `http(s)://host[:port][/path]` endpoint without credentials, query or fragment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    scheme: Scheme,
    host: String,
    port: Option<u16>,
    path: String,
}

impl Endpoint {
    /// Maximum accepted length of the textual form.
    pub const MAX_LEN: usize = 2048;

    /// Parses an endpoint URL.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] on `endpoint` for anything other than a plain
    /// `http(s)://host[:port][/path]` URL.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let malformed = || DomainError::invalid("endpoint", InputProblem::Malformed);
        if value.len() > Self::MAX_LEN {
            return Err(DomainError::invalid("endpoint", InputProblem::TooLong));
        }
        if value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '@' | '?' | '#' | '\\'))
        {
            return Err(malformed());
        }
        let (scheme, rest) = if let Some(rest) = value.strip_prefix("https://") {
            (Scheme::Https, rest)
        } else if let Some(rest) = value.strip_prefix("http://") {
            (Scheme::Http, rest)
        } else {
            return Err(malformed());
        };
        let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
        let (host, port) = if let Some(after_bracket) = authority.strip_prefix('[') {
            let (inner, tail) = after_bracket.split_once(']').ok_or_else(malformed)?;
            if inner.is_empty() || !inner.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
                return Err(malformed());
            }
            (format!("[{}]", inner.to_ascii_lowercase()), tail)
        } else {
            let end = authority.find(':').unwrap_or(authority.len());
            let (host, tail) = authority.split_at(end);
            let valid = !host.is_empty()
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'));
            if !valid {
                return Err(malformed());
            }
            (host.to_ascii_lowercase(), tail)
        };
        let port = match port {
            "" => None,
            p => Some(
                p.strip_prefix(':')
                    .and_then(|digits| digits.parse::<u16>().ok())
                    .filter(|port| *port != 0)
                    .ok_or_else(malformed)?,
            ),
        };
        Ok(Self {
            scheme,
            host,
            port,
            path: path.trim_end_matches('/').to_owned(),
        })
    }

    /// URL scheme.
    #[must_use]
    pub const fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// Lowercased host (IPv6 literals keep their brackets).
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Explicit port, if any.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        self.port
    }

    /// Host plus explicit port: what the authorization dialog shows as the destination.
    #[must_use]
    pub fn destination(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host),
            None => self.host.clone(),
        }
    }

    /// Whether the host is the loopback interface.
    #[must_use]
    pub fn is_loopback(&self) -> bool {
        self.host == "localhost"
            || self.host == "[::1]"
            || self
                .host
                .strip_prefix("127.")
                .is_some_and(|rest| rest.split('.').count() == 3)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        write!(f, "{scheme}://{}{}", self.destination(), self.path)
    }
}

/// Reference to a secret held in the OS credential store; never the secret itself (FR-049).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialRef(String);

impl CredentialRef {
    /// Credential reference of a provider's API key.
    #[must_use]
    pub fn for_provider(id: ProviderId) -> Self {
        Self(format!("provider:{id}"))
    }

    /// Parses a stored reference (`[a-z0-9:._-]`, 1–128 characters).
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] for anything else.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '.' | '_' | '-')
            });
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(DomainError::invalid(
                "credential_ref",
                InputProblem::Malformed,
            ))
        }
    }

    /// The reference string (used as the credential-store entry name).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Fields of a provider before validation.
#[derive(Debug, Clone)]
pub struct ProviderSpec {
    /// Identifier.
    pub id: ProviderId,
    /// Declared role; must match the adapter.
    pub role: ProviderRole,
    /// Implementation.
    pub adapter: ProviderAdapterKind,
    /// HTTP endpoint, required by every adapter except `whisper_local`.
    pub endpoint: Option<String>,
    /// Model name or, for `whisper_local`, the model identifier.
    pub model: String,
    /// Where the API key lives, if any.
    pub credential_ref: Option<CredentialRef>,
    /// Data the provider receives.
    pub data_categories: BTreeSet<DataCategory>,
}

/// A configured, validated provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    id: ProviderId,
    adapter: ProviderAdapterKind,
    endpoint: Option<Endpoint>,
    model: String,
    credential_ref: Option<CredentialRef>,
    data_categories: BTreeSet<DataCategory>,
}

impl Provider {
    /// Maximum length of the model name.
    pub const MODEL_MAX_CHARS: usize = 200;

    /// Validates a provider configuration.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] when the adapter does not match the role, the
    /// endpoint is missing, not `https` for a remote provider or not loopback for a local one,
    /// the model name is invalid, or the data categories are empty or not allowed for the role.
    pub fn new(spec: ProviderSpec) -> Result<Self, DomainError> {
        if spec.adapter.role() != spec.role {
            return Err(DomainError::invalid("adapter", InputProblem::Inconsistent));
        }
        let endpoint = match (spec.adapter.needs_endpoint(), spec.endpoint.as_deref()) {
            (true, None) => return Err(DomainError::invalid("endpoint", InputProblem::Empty)),
            (false, Some(_)) => {
                return Err(DomainError::invalid("endpoint", InputProblem::Inconsistent));
            }
            (false, None) => None,
            (true, Some(raw)) => Some(Endpoint::parse(raw)?),
        };
        if let Some(endpoint) = &endpoint {
            let allowed = match spec.adapter.locality() {
                Locality::Remote => endpoint.scheme() == Scheme::Https,
                Locality::Local => endpoint.is_loopback(),
            };
            if !allowed {
                return Err(DomainError::invalid("endpoint", InputProblem::Inconsistent));
            }
        }
        let model = spec.model.trim();
        if model.is_empty() {
            return Err(DomainError::invalid("model", InputProblem::Empty));
        }
        if model.chars().count() > Self::MODEL_MAX_CHARS {
            return Err(DomainError::invalid("model", InputProblem::TooLong));
        }
        if model.chars().any(char::is_control) {
            return Err(DomainError::invalid(
                "model",
                InputProblem::ControlCharacter,
            ));
        }
        if spec.data_categories.is_empty() {
            return Err(DomainError::invalid("data_categories", InputProblem::Empty));
        }
        let allowed = spec.role.allowed_categories();
        if !spec.data_categories.iter().all(|c| allowed.contains(c)) {
            return Err(DomainError::invalid(
                "data_categories",
                InputProblem::Inconsistent,
            ));
        }
        Ok(Self {
            id: spec.id,
            adapter: spec.adapter,
            endpoint,
            model: model.to_owned(),
            credential_ref: spec.credential_ref,
            data_categories: spec.data_categories,
        })
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> ProviderId {
        self.id
    }

    /// Role, derived from the adapter.
    #[must_use]
    pub const fn role(&self) -> ProviderRole {
        self.adapter.role()
    }

    /// Locality, derived from the adapter.
    #[must_use]
    pub const fn locality(&self) -> Locality {
        self.adapter.locality()
    }

    /// Implementation.
    #[must_use]
    pub const fn adapter(&self) -> ProviderAdapterKind {
        self.adapter
    }

    /// HTTP endpoint, if the adapter uses one.
    #[must_use]
    pub const fn endpoint(&self) -> Option<&Endpoint> {
        self.endpoint.as_ref()
    }

    /// Model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Credential reference, if any.
    #[must_use]
    pub const fn credential_ref(&self) -> Option<&CredentialRef> {
        self.credential_ref.as_ref()
    }

    /// Data categories the provider receives.
    #[must_use]
    pub const fn data_categories(&self) -> &BTreeSet<DataCategory> {
        &self.data_categories
    }

    /// The same provider with its API key stored under `reference`.
    #[must_use]
    pub fn with_credential_ref(mut self, reference: CredentialRef) -> Self {
        self.credential_ref = Some(reference);
        self
    }

    /// Whether `other` sends data elsewhere or sends other data than `self`: the changes that
    /// need a new authorization (data-model.md "ProviderAuthorization").
    #[must_use]
    pub fn changes_destination_or_data(&self, other: &Self) -> bool {
        self.adapter != other.adapter
            || self.endpoint != other.endpoint
            || self.data_categories != other.data_categories
    }
}

/// State of a stored remote authorization (data-model.md "ProviderAuthorization").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthorizationState {
    /// The user accepted; requests it covers may be sent.
    Granted,
    /// The user withdrew it; nothing more is sent (FR-047).
    Revoked,
}

code_enum!(AuthorizationState, "state", {
    Granted => "granted",
    Revoked => "revoked",
});

/// Where a provider stands with respect to authorization, as shown to the user (spec
/// "Provedor": não configurado, configurado, autorizado, revogado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthorizationStatus {
    /// Local provider: nothing leaves the machine, no authorization applies.
    NotRequired,
    /// Remote provider without an authorization covering its current configuration.
    Configured,
    /// Remote provider with a granted authorization covering its configuration.
    Authorized,
    /// Remote provider whose last authorization was revoked.
    Revoked,
}

code_enum!(AuthorizationStatus, "authorization", {
    NotRequired => "not_required",
    Configured => "configured",
    Authorized => "authorized",
    Revoked => "revoked",
});

/// Stored fields of an authorization before validation.
#[derive(Debug, Clone)]
pub struct AuthorizationSpec {
    /// Authorized provider.
    pub provider_id: ProviderId,
    /// Granted or revoked.
    pub state: AuthorizationState,
    /// Destination shown to the user (host and explicit port).
    pub destination: String,
    /// Categories shown to the user and accepted.
    pub data_categories: BTreeSet<DataCategory>,
    /// Retention text shown to the user.
    pub retention_notice: String,
    /// When the user accepted.
    pub granted_at: Timestamp,
    /// When the user revoked, for revoked authorizations.
    pub revoked_at: Option<Timestamp>,
}

/// An informed authorization of a remote provider (FR-046): a snapshot of what the user saw
/// and accepted.
///
/// Coverage rule: a request is allowed only while the authorization is granted and its
/// destination and categories still cover the provider's configuration and the request.
/// Changing the provider's endpoint or categories therefore needs a new authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthorization {
    provider_id: ProviderId,
    state: AuthorizationState,
    destination: String,
    data_categories: BTreeSet<DataCategory>,
    retention_notice: String,
    granted_at: Timestamp,
    revoked_at: Option<Timestamp>,
}

impl ProviderAuthorization {
    /// Maximum length of the stored retention notice.
    pub const RETENTION_NOTICE_MAX_CHARS: usize = 4_000;

    /// Grants `provider` what it is configured to receive, after the user saw
    /// `retention_notice`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] on `provider_id` for a local provider (nothing to
    /// authorize) and on `retention_notice` when it is empty or too long.
    pub fn grant(
        provider: &Provider,
        retention_notice: String,
        at: Timestamp,
    ) -> Result<Self, DomainError> {
        let endpoint = provider
            .endpoint()
            .filter(|_| provider.locality() == Locality::Remote)
            .ok_or_else(|| DomainError::invalid("provider_id", InputProblem::Inconsistent))?;
        Self::restore(AuthorizationSpec {
            provider_id: provider.id(),
            state: AuthorizationState::Granted,
            destination: endpoint.destination(),
            data_categories: provider.data_categories().clone(),
            retention_notice,
            granted_at: at,
            revoked_at: None,
        })
    }

    /// Rebuilds a stored authorization.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] when a field is empty or too long, or the revocation
    /// instant does not match the state.
    pub fn restore(spec: AuthorizationSpec) -> Result<Self, DomainError> {
        if spec.destination.is_empty() || spec.destination.len() > Endpoint::MAX_LEN {
            return Err(DomainError::invalid("destination", InputProblem::Malformed));
        }
        if spec.data_categories.is_empty() {
            return Err(DomainError::invalid("data_categories", InputProblem::Empty));
        }
        let notice = spec.retention_notice.trim();
        if notice.is_empty() {
            return Err(DomainError::invalid(
                "retention_notice",
                InputProblem::Empty,
            ));
        }
        if notice.chars().count() > Self::RETENTION_NOTICE_MAX_CHARS {
            return Err(DomainError::invalid(
                "retention_notice",
                InputProblem::TooLong,
            ));
        }
        let revoked = spec.state == AuthorizationState::Revoked;
        if revoked != spec.revoked_at.is_some() {
            return Err(DomainError::invalid(
                "revoked_at",
                InputProblem::Inconsistent,
            ));
        }
        Ok(Self {
            provider_id: spec.provider_id,
            state: spec.state,
            destination: spec.destination,
            data_categories: spec.data_categories,
            retention_notice: notice.to_owned(),
            granted_at: spec.granted_at,
            revoked_at: spec.revoked_at,
        })
    }

    /// Withdraws the authorization; requests stop from now on (FR-047).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] when it is already revoked.
    pub fn revoke(&mut self, at: Timestamp) -> Result<(), DomainError> {
        if self.state == AuthorizationState::Revoked {
            return Err(DomainError::InvalidTransition {
                from: self.state.code(),
                action: "revoke",
            });
        }
        self.state = AuthorizationState::Revoked;
        self.revoked_at = Some(at.max(self.granted_at));
        Ok(())
    }

    /// Whether this authorization allows sending `categories` to `provider` as it is
    /// configured now.
    #[must_use]
    pub fn covers(&self, provider: &Provider, categories: &[DataCategory]) -> bool {
        self.state == AuthorizationState::Granted
            && provider.id() == self.provider_id
            && provider.locality() == Locality::Remote
            && provider
                .endpoint()
                .is_some_and(|endpoint| endpoint.destination() == self.destination)
            && provider.data_categories().is_subset(&self.data_categories)
            && categories
                .iter()
                .all(|category| provider.data_categories().contains(category))
    }

    /// Authorized provider.
    #[must_use]
    pub const fn provider_id(&self) -> ProviderId {
        self.provider_id
    }

    /// Granted or revoked.
    #[must_use]
    pub const fn state(&self) -> AuthorizationState {
        self.state
    }

    /// Destination the user saw.
    #[must_use]
    pub fn destination(&self) -> &str {
        &self.destination
    }

    /// Categories the user accepted.
    #[must_use]
    pub const fn data_categories(&self) -> &BTreeSet<DataCategory> {
        &self.data_categories
    }

    /// Retention text the user saw.
    #[must_use]
    pub fn retention_notice(&self) -> &str {
        &self.retention_notice
    }

    /// When the user accepted.
    #[must_use]
    pub const fn granted_at(&self) -> Timestamp {
        self.granted_at
    }

    /// When the user revoked, if revoked.
    #[must_use]
    pub const fn revoked_at(&self) -> Option<Timestamp> {
        self.revoked_at
    }
}

/// Authorization status of `provider` given its latest stored authorization.
#[must_use]
pub fn authorization_status(
    provider: &Provider,
    latest: Option<&ProviderAuthorization>,
) -> AuthorizationStatus {
    if provider.locality() == Locality::Local {
        return AuthorizationStatus::NotRequired;
    }
    match latest {
        Some(auth) if auth.covers(provider, &[]) => AuthorizationStatus::Authorized,
        Some(auth) if auth.state() == AuthorizationState::Revoked => AuthorizationStatus::Revoked,
        _ => AuthorizationStatus::Configured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(adapter: ProviderAdapterKind, endpoint: Option<&str>) -> ProviderSpec {
        let role = adapter.role();
        ProviderSpec {
            id: ProviderId::new(),
            role,
            adapter,
            endpoint: endpoint.map(str::to_owned),
            model: "model-x".into(),
            credential_ref: None,
            data_categories: role.allowed_categories().iter().copied().collect(),
        }
    }

    fn input_err(field: &'static str, problem: InputProblem) -> Result<Provider, DomainError> {
        Err(DomainError::invalid(field, problem))
    }

    #[test]
    fn local_whisper_has_no_endpoint() {
        let provider = Provider::new(spec(ProviderAdapterKind::WhisperLocal, None)).unwrap();
        assert_eq!(provider.role(), ProviderRole::Transcription);
        assert_eq!(provider.locality(), Locality::Local);
        assert_eq!(
            Provider::new(spec(
                ProviderAdapterKind::WhisperLocal,
                Some("http://127.0.0.1")
            )),
            input_err("endpoint", InputProblem::Inconsistent)
        );
    }

    #[test]
    fn remote_requires_https_endpoint() {
        let ok = Provider::new(spec(
            ProviderAdapterKind::Anthropic,
            Some("https://api.example.com/v1/"),
        ))
        .unwrap();
        assert_eq!(ok.locality(), Locality::Remote);
        assert_eq!(
            ok.endpoint().map(Endpoint::destination).as_deref(),
            Some("api.example.com")
        );
        assert_eq!(
            Provider::new(spec(
                ProviderAdapterKind::OpenAiCompatibleChat,
                Some("http://api.example.com")
            )),
            input_err("endpoint", InputProblem::Inconsistent)
        );
        assert_eq!(
            Provider::new(spec(ProviderAdapterKind::OpenAiCompatibleStt, None)),
            input_err("endpoint", InputProblem::Empty)
        );
    }

    #[test]
    fn local_http_endpoint_must_be_loopback() {
        for endpoint in [
            "http://127.0.0.1:11434",
            "http://localhost:11434",
            "http://[::1]:11434",
        ] {
            assert!(
                Provider::new(spec(ProviderAdapterKind::Ollama, Some(endpoint))).is_ok(),
                "{endpoint}"
            );
        }
        for endpoint in [
            "http://192.168.0.10:11434",
            "https://ollama.example.com",
            "http://127.0.0.1.evil.com",
        ] {
            assert_eq!(
                Provider::new(spec(ProviderAdapterKind::Ollama, Some(endpoint))),
                input_err("endpoint", InputProblem::Inconsistent),
                "{endpoint}"
            );
        }
    }

    #[test]
    fn endpoint_rejects_credentials_query_and_other_schemes() {
        for raw in [
            "ftp://host",
            "https://user:pw@host",
            "https://host/?q=1",
            "https://host#frag",
            "https://",
            "https://host:0",
            "https://host:99999",
            "https://ho st",
            "https://host_name",
            "https://[zz]",
            "https:\\\\host",
        ] {
            assert!(Endpoint::parse(raw).is_err(), "{raw}");
        }
        let long = format!("https://host/{}", "a".repeat(Endpoint::MAX_LEN));
        assert_eq!(
            Endpoint::parse(&long),
            Err(DomainError::invalid("endpoint", InputProblem::TooLong))
        );
    }

    #[test]
    fn endpoint_normalizes_host_and_keeps_port() {
        let endpoint = Endpoint::parse("https://API.Example.com:8443/v1/").unwrap();
        assert_eq!(endpoint.host(), "api.example.com");
        assert_eq!(endpoint.port(), Some(8443));
        assert_eq!(endpoint.destination(), "api.example.com:8443");
        assert_eq!(endpoint.to_string(), "https://api.example.com:8443/v1");
    }

    #[test]
    fn adapter_must_match_role() {
        let mut bad = spec(
            ProviderAdapterKind::Anthropic,
            Some("https://api.example.com"),
        );
        bad.role = ProviderRole::Transcription;
        assert_eq!(
            Provider::new(bad),
            input_err("adapter", InputProblem::Inconsistent)
        );
    }

    #[test]
    fn data_categories_must_be_non_empty_and_allowed_for_role() {
        let mut empty = spec(ProviderAdapterKind::WhisperLocal, None);
        empty.data_categories.clear();
        assert_eq!(
            Provider::new(empty),
            input_err("data_categories", InputProblem::Empty)
        );
        let mut wrong = spec(
            ProviderAdapterKind::OpenAiCompatibleStt,
            Some("https://stt.example.com"),
        );
        wrong.data_categories.insert(DataCategory::Transcript);
        assert_eq!(
            Provider::new(wrong),
            input_err("data_categories", InputProblem::Inconsistent)
        );
    }

    #[test]
    fn model_is_trimmed_and_bounded() {
        let mut s = spec(ProviderAdapterKind::WhisperLocal, None);
        s.model = "  ggml-base  ".into();
        assert_eq!(Provider::new(s).unwrap().model(), "ggml-base");
        let mut blank = spec(ProviderAdapterKind::WhisperLocal, None);
        blank.model = "  ".into();
        assert_eq!(
            Provider::new(blank),
            input_err("model", InputProblem::Empty)
        );
        let mut long = spec(ProviderAdapterKind::WhisperLocal, None);
        long.model = "m".repeat(201);
        assert_eq!(
            Provider::new(long),
            input_err("model", InputProblem::TooLong)
        );
    }

    #[test]
    fn codes_round_trip() {
        for adapter in [
            ProviderAdapterKind::WhisperLocal,
            ProviderAdapterKind::OpenAiCompatibleStt,
            ProviderAdapterKind::Anthropic,
            ProviderAdapterKind::OpenAiCompatibleChat,
            ProviderAdapterKind::Ollama,
        ] {
            assert_eq!(ProviderAdapterKind::parse(adapter.code()), Ok(adapter));
        }
        assert!(DataCategory::parse("secrets").is_err());
    }

    #[test]
    fn credential_ref_is_restricted() {
        let id = ProviderId::new();
        let reference = CredentialRef::for_provider(id);
        assert_eq!(CredentialRef::parse(reference.as_str()), Ok(reference));
        assert!(CredentialRef::parse("Provider Key").is_err());
        assert!(CredentialRef::parse("").is_err());
    }

    fn remote(categories: &[DataCategory]) -> Provider {
        let mut s = spec(
            ProviderAdapterKind::Anthropic,
            Some("https://api.example.com/v1"),
        );
        s.data_categories = categories.iter().copied().collect();
        Provider::new(s).unwrap()
    }

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_unix_millis(ms).unwrap()
    }

    #[test]
    fn grant_snapshots_destination_and_categories() {
        let provider = remote(&[DataCategory::Transcript, DataCategory::Records]);
        let auth =
            ProviderAuthorization::grant(&provider, " Kept 30 days. ".into(), at(1)).unwrap();
        assert_eq!(auth.state(), AuthorizationState::Granted);
        assert_eq!(auth.destination(), "api.example.com");
        assert_eq!(auth.retention_notice(), "Kept 30 days.");
        assert_eq!(auth.data_categories(), provider.data_categories());
        assert!(auth.covers(&provider, &[DataCategory::Transcript]));
        assert!(auth.covers(
            &provider,
            &[DataCategory::Transcript, DataCategory::Records]
        ));
        assert_eq!(
            authorization_status(&provider, Some(&auth)),
            AuthorizationStatus::Authorized
        );
    }

    #[test]
    fn local_providers_need_no_authorization() {
        let local = Provider::new(spec(ProviderAdapterKind::WhisperLocal, None)).unwrap();
        assert_eq!(
            ProviderAuthorization::grant(&local, "n/a".into(), at(1)),
            Err(DomainError::invalid(
                "provider_id",
                InputProblem::Inconsistent
            ))
        );
        assert_eq!(
            authorization_status(&local, None),
            AuthorizationStatus::NotRequired
        );
    }

    #[test]
    fn undeclared_category_is_not_covered() {
        let provider = remote(&[DataCategory::Transcript]);
        let auth = ProviderAuthorization::grant(&provider, "notice".into(), at(1)).unwrap();
        assert!(!auth.covers(&provider, &[DataCategory::Questions]));
    }

    #[test]
    fn endpoint_or_category_change_invalidates() {
        let provider = remote(&[DataCategory::Transcript]);
        let auth = ProviderAuthorization::grant(&provider, "notice".into(), at(1)).unwrap();
        let mut moved = spec(
            ProviderAdapterKind::Anthropic,
            Some("https://other.example.com/v1"),
        );
        moved.id = provider.id();
        moved.data_categories = BTreeSet::from([DataCategory::Transcript]);
        let moved = Provider::new(moved).unwrap();
        assert!(!auth.covers(&moved, &[DataCategory::Transcript]));
        let mut wider = spec(
            ProviderAdapterKind::Anthropic,
            Some("https://api.example.com/v1"),
        );
        wider.id = provider.id();
        wider.data_categories = BTreeSet::from([DataCategory::Transcript, DataCategory::Questions]);
        let wider = Provider::new(wider).unwrap();
        assert!(!auth.covers(&wider, &[DataCategory::Transcript]));
        assert_eq!(
            authorization_status(&wider, Some(&auth)),
            AuthorizationStatus::Configured
        );
        let other = remote(&[DataCategory::Transcript]);
        assert!(!auth.covers(&other, &[DataCategory::Transcript]));
    }

    #[test]
    fn revoked_covers_nothing_and_cannot_be_revoked_twice() {
        let provider = remote(&[DataCategory::Transcript]);
        let mut auth = ProviderAuthorization::grant(&provider, "notice".into(), at(10)).unwrap();
        auth.revoke(at(20)).unwrap();
        assert_eq!(auth.revoked_at(), Some(at(20)));
        assert!(!auth.covers(&provider, &[DataCategory::Transcript]));
        assert_eq!(
            authorization_status(&provider, Some(&auth)),
            AuthorizationStatus::Revoked
        );
        assert!(matches!(
            auth.revoke(at(30)),
            Err(DomainError::InvalidTransition { .. })
        ));
        assert_eq!(
            authorization_status(&provider, None),
            AuthorizationStatus::Configured
        );
    }

    #[test]
    fn restore_validates_fields() {
        let base = AuthorizationSpec {
            provider_id: ProviderId::new(),
            state: AuthorizationState::Granted,
            destination: "api.example.com".into(),
            data_categories: BTreeSet::from([DataCategory::Audio]),
            retention_notice: "notice".into(),
            granted_at: at(1),
            revoked_at: None,
        };
        assert!(ProviderAuthorization::restore(base.clone()).is_ok());
        let revoked_without_instant = AuthorizationSpec {
            state: AuthorizationState::Revoked,
            ..base.clone()
        };
        assert!(ProviderAuthorization::restore(revoked_without_instant).is_err());
        let blank = AuthorizationSpec {
            retention_notice: "  ".into(),
            ..base.clone()
        };
        assert!(ProviderAuthorization::restore(blank).is_err());
        let long = AuthorizationSpec {
            retention_notice: "x".repeat(ProviderAuthorization::RETENTION_NOTICE_MAX_CHARS + 1),
            ..base
        };
        assert!(ProviderAuthorization::restore(long).is_err());
    }
}
