//! Remote authorization rules of the provider service and its [`RemoteGate`] (T129, SC-013,
//! FR-046, FR-047).
//!
//! These tests cover the gate decisions over storage; the zero-requests guarantee against a
//! fake HTTP server, through the real adapters, lives in
//! `crates/intelligence/tests/remote_gate.rs` (the adapters cannot be a dependency of this
//! crate).

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::ports::{AuthorizationError, ProviderRepository, SecretStore};
use devcopilot_application::provider_service::{ProviderService, retention_notice};
use devcopilot_application::testing::{
    FakeClock, InMemoryProviderRepository, InMemorySettingsRepository, ToggleableSecretStore,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::language::UiLanguage;
use devcopilot_domain::provider::{
    AuthorizationState, AuthorizationStatus, CredentialRef, DataCategory, ProviderAdapterKind,
    ProviderRole, ProviderSpec,
};

struct Fixture {
    service: ProviderService,
    repo: Arc<InMemoryProviderRepository>,
    secrets: Arc<ToggleableSecretStore>,
}

fn fixture() -> Fixture {
    let repo = Arc::new(InMemoryProviderRepository::default());
    let secrets = Arc::new(ToggleableSecretStore::available());
    Fixture {
        service: ProviderService::new(
            repo.clone(),
            Arc::new(InMemorySettingsRepository::default()),
            secrets.clone(),
            Arc::new(FakeClock::at(1_000)),
        ),
        repo,
        secrets,
    }
}

fn anthropic(id: ProviderId, endpoint: &str, categories: &[DataCategory]) -> ProviderSpec {
    ProviderSpec {
        id,
        role: ProviderRole::Analysis,
        adapter: ProviderAdapterKind::Anthropic,
        endpoint: Some(endpoint.into()),
        model: "claude-opus-5-5".into(),
        credential_ref: None,
        data_categories: categories.iter().copied().collect(),
    }
}

const ANALYSIS: &[DataCategory] = &[DataCategory::Transcript, DataCategory::Records];

#[test]
fn nothing_passes_the_gate_without_authorization() {
    let f = fixture();
    let id = ProviderId::new();
    let gate = f.service.gate();
    assert_eq!(
        gate.check(&id, &[DataCategory::Transcript]),
        Err(AuthorizationError::NotAuthorized),
        "unknown provider"
    );
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    assert_eq!(
        gate.check(&id, &[DataCategory::Transcript]),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(
        f.service.entry(id).unwrap().authorization,
        AuthorizationStatus::Configured
    );
}

#[test]
fn authorization_allows_only_declared_categories() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    let preview = f.service.authorization_preview(id).unwrap();
    assert_eq!(preview.destination, "api.example.com");
    assert_eq!(
        preview.data_categories,
        ANALYSIS.iter().copied().collect::<BTreeSet<_>>()
    );
    let auth = f.service.authorize(id).unwrap();
    assert_eq!(auth.retention_notice(), preview.retention_notice);
    let gate = f.service.gate();
    assert_eq!(gate.check(&id, &[DataCategory::Transcript]), Ok(()));
    assert_eq!(gate.check(&id, ANALYSIS), Ok(()));
    assert_eq!(
        gate.check(&id, &[DataCategory::Questions]),
        Err(AuthorizationError::NotAuthorized)
    );
    assert_eq!(
        f.service.entry(id).unwrap().authorization,
        AuthorizationStatus::Authorized
    );
}

#[test]
fn endpoint_change_revokes_and_blocks() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service.authorize(id).unwrap();
    f.service
        .upsert(anthropic(id, "https://attacker.example.net", ANALYSIS))
        .unwrap();
    assert_eq!(
        f.service.gate().check(&id, &[DataCategory::Transcript]),
        Err(AuthorizationError::NotAuthorized)
    );
    let latest = f.repo.latest_authorization(id).unwrap().unwrap();
    assert_eq!(latest.state(), AuthorizationState::Revoked);
    assert_eq!(
        f.service.entry(id).unwrap().authorization,
        AuthorizationStatus::Revoked
    );
}

#[test]
fn category_change_revokes_and_blocks() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(
            id,
            "https://api.example.com",
            &[DataCategory::Transcript],
        ))
        .unwrap();
    f.service.authorize(id).unwrap();
    f.service
        .upsert(anthropic(
            id,
            "https://api.example.com",
            &[DataCategory::Transcript, DataCategory::Questions],
        ))
        .unwrap();
    assert_eq!(
        f.service.gate().check(&id, &[DataCategory::Transcript]),
        Err(AuthorizationError::NotAuthorized)
    );
}

#[test]
fn model_change_keeps_the_authorization() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service.authorize(id).unwrap();
    f.service
        .upsert(ProviderSpec {
            model: "claude-sonnet-5-5".into(),
            ..anthropic(id, "https://api.example.com", ANALYSIS)
        })
        .unwrap();
    assert_eq!(f.service.gate().check(&id, ANALYSIS), Ok(()));
}

#[test]
fn revocation_applies_to_the_next_check() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service.authorize(id).unwrap();
    // A gate handed out before revocation sees it at once (adapters keep their gate).
    let gate = f.service.gate();
    assert_eq!(gate.check(&id, ANALYSIS), Ok(()));
    let revoked = f.service.revoke(id).unwrap();
    assert_eq!(revoked.state(), AuthorizationState::Revoked);
    assert_eq!(
        gate.check(&id, ANALYSIS),
        Err(AuthorizationError::NotAuthorized)
    );
    assert!(matches!(
        f.service.revoke(id),
        Err(ApplicationError::InvalidState(_))
    ));
}

#[test]
fn storage_failure_blocks_requests() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service.authorize(id).unwrap();
    f.repo.fail_with(Some(
        devcopilot_application::ports::RepositoryError::Storage("io".into()),
    ));
    assert_eq!(
        f.service.gate().check(&id, ANALYSIS),
        Err(AuthorizationError::NotAuthorized)
    );
}

#[test]
fn remote_endpoints_must_be_https() {
    let f = fixture();
    let id = ProviderId::new();
    assert_eq!(
        f.service
            .upsert(anthropic(id, "http://api.example.com", ANALYSIS))
            .map(|_| ()),
        Err(ApplicationError::InvalidInput {
            field: "endpoint",
            problem: "inconsistent"
        })
    );
    assert!(f.repo.list().unwrap().is_empty());
}

#[test]
fn local_providers_have_nothing_to_authorize() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(ProviderSpec {
            id,
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3.1".into(),
            credential_ref: None,
            data_categories: ANALYSIS.iter().copied().collect(),
        })
        .unwrap();
    assert!(matches!(
        f.service.authorize(id),
        Err(ApplicationError::InvalidInput {
            field: "provider_id",
            ..
        })
    ));
    assert_eq!(
        f.service.entry(id).unwrap().authorization,
        AuthorizationStatus::NotRequired
    );
}

#[test]
fn unauthorized_remote_default_asks_for_authorization() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service.set_default(ProviderRole::Analysis, id).unwrap();
    assert_eq!(
        f.service.resolve_analyzer(),
        Err(ApplicationError::AuthorizationRequired { provider: Some(id) })
    );
    f.service.authorize(id).unwrap();
    assert_eq!(
        f.service.resolve_analyzer().unwrap().map(|p| p.id()),
        Some(id)
    );
}

#[test]
fn secret_goes_to_the_vault_and_survives_reconfiguration() {
    let f = fixture();
    let id = ProviderId::new();
    f.service
        .upsert(anthropic(id, "https://api.example.com", ANALYSIS))
        .unwrap();
    f.service
        .set_secret(id, Redacted::new(b"sk-test-123".to_vec()))
        .unwrap();
    let reference = CredentialRef::for_provider(id);
    assert_eq!(
        f.secrets
            .get(&reference)
            .unwrap()
            .map(|s| s.expose().clone()),
        Some(b"sk-test-123".to_vec())
    );
    assert_eq!(f.repo.get(id).unwrap().credential_ref(), Some(&reference));
    // The UI never sends the reference; an edit keeps it.
    f.service
        .upsert(ProviderSpec {
            model: "claude-sonnet-5-5".into(),
            ..anthropic(id, "https://api.example.com", ANALYSIS)
        })
        .unwrap();
    assert_eq!(f.repo.get(id).unwrap().credential_ref(), Some(&reference));
    assert!(matches!(
        f.service.set_secret(id, Redacted::new(b"  ".to_vec())),
        Err(ApplicationError::InvalidInput {
            field: "secret",
            ..
        })
    ));
    f.secrets.set_available(false);
    assert_eq!(
        f.service.set_secret(id, Redacted::new(b"sk-new".to_vec())),
        Err(ApplicationError::SecretStoreUnavailable)
    );
}

#[test]
fn retention_notice_follows_the_ui_language() {
    assert!(retention_notice("api.example.com", UiLanguage::PtBr).contains("api.example.com"));
    assert!(retention_notice("api.example.com", UiLanguage::En).contains("revocation"));
}
