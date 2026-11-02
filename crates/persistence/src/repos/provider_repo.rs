//! [`ProviderRepository`] over the `providers` and `provider_authorizations` tables.
//!
//! Authorizations are kept as history: granting adds a row, revoking updates the provider's
//! latest row. Secrets are never stored here, only their [`CredentialRef`] (FR-049).

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ports::{ProviderRepository, RepositoryError};
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    AuthorizationSpec, AuthorizationState, CredentialRef, DataCategory, Provider,
    ProviderAdapterKind, ProviderAuthorization, ProviderRole, ProviderSpec,
};
use devcopilot_domain::time::Timestamp;
use rusqlite::{OptionalExtension, Row, params};

use crate::db::{Database, storage_error};

const COLUMNS: &str = "id, role, adapter, endpoint, model, credential_ref, data_categories";

/// Raw row; mapped to [`Provider`] through domain validation.
struct ProviderRow {
    id: String,
    role: String,
    adapter: String,
    endpoint: Option<String>,
    model: String,
    credential_ref: Option<String>,
    data_categories: String,
}

impl ProviderRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            role: row.get(1)?,
            adapter: row.get(2)?,
            endpoint: row.get(3)?,
            model: row.get(4)?,
            credential_ref: row.get(5)?,
            data_categories: row.get(6)?,
        })
    }

    fn into_provider(self) -> Result<Provider, RepositoryError> {
        let corrupt = |_| RepositoryError::Corrupt("providers");
        let data_categories = parse_categories(&self.data_categories, "providers")?;
        Provider::new(ProviderSpec {
            id: ProviderId::parse(&self.id).map_err(corrupt)?,
            role: ProviderRole::parse(&self.role).map_err(corrupt)?,
            adapter: ProviderAdapterKind::parse(&self.adapter).map_err(corrupt)?,
            endpoint: self.endpoint,
            model: self.model,
            credential_ref: self
                .credential_ref
                .as_deref()
                .map(CredentialRef::parse)
                .transpose()
                .map_err(corrupt)?,
            data_categories,
        })
        .map_err(corrupt)
    }
}

fn parse_categories(
    raw: &str,
    table: &'static str,
) -> Result<BTreeSet<DataCategory>, RepositoryError> {
    raw.split(',')
        .filter(|code| !code.is_empty())
        .map(DataCategory::parse)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| RepositoryError::Corrupt(table))
}

const AUTH_COLUMNS: &str =
    "provider_id, state, destination, data_categories, retention_notice, granted_at, revoked_at";

/// Raw authorization row; mapped through domain validation.
struct AuthorizationRow {
    provider_id: String,
    state: String,
    destination: String,
    data_categories: String,
    retention_notice: String,
    granted_at: i64,
    revoked_at: Option<i64>,
}

impl AuthorizationRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            provider_id: row.get(0)?,
            state: row.get(1)?,
            destination: row.get(2)?,
            data_categories: row.get(3)?,
            retention_notice: row.get(4)?,
            granted_at: row.get(5)?,
            revoked_at: row.get(6)?,
        })
    }

    fn into_authorization(self) -> Result<ProviderAuthorization, RepositoryError> {
        const TABLE: &str = "provider_authorizations";
        let corrupt = |_| RepositoryError::Corrupt(TABLE);
        ProviderAuthorization::restore(AuthorizationSpec {
            provider_id: ProviderId::parse(&self.provider_id).map_err(corrupt)?,
            state: AuthorizationState::parse(&self.state).map_err(corrupt)?,
            destination: self.destination,
            data_categories: parse_categories(&self.data_categories, TABLE)?,
            retention_notice: self.retention_notice,
            granted_at: Timestamp::from_unix_millis(self.granted_at).map_err(corrupt)?,
            revoked_at: self
                .revoked_at
                .map(Timestamp::from_unix_millis)
                .transpose()
                .map_err(corrupt)?,
        })
        .map_err(corrupt)
    }
}

fn join_categories<'a>(categories: impl IntoIterator<Item = &'a DataCategory>) -> String {
    categories
        .into_iter()
        .map(|c| c.code())
        .collect::<Vec<_>>()
        .join(",")
}

fn categories_column(provider: &Provider) -> String {
    provider
        .data_categories()
        .iter()
        .map(|c| c.code())
        .collect::<Vec<_>>()
        .join(",")
}

/// Providers stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteProviderRepository {
    db: Arc<Database>,
}

impl SqliteProviderRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl ProviderRepository for SqliteProviderRepository {
    fn list(&self) -> Result<Vec<Provider>, RepositoryError> {
        let rows = self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT {COLUMNS} FROM providers ORDER BY rowid"))
                .map_err(|e| storage_error(&e))?;
            let rows = stmt
                .query_map([], ProviderRow::read)
                .map_err(|e| storage_error(&e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| storage_error(&e))?;
            Ok(rows)
        })?;
        rows.into_iter().map(ProviderRow::into_provider).collect()
    }

    fn get(&self, id: ProviderId) -> Result<Provider, RepositoryError> {
        let row = self.db.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {COLUMNS} FROM providers WHERE id = ?1"),
                params![id.to_string()],
                ProviderRow::read,
            )
            .optional()
            .map_err(|e| storage_error(&e))
        })?;
        row.ok_or(RepositoryError::NotFound)?.into_provider()
    }

    fn upsert(&self, provider: &Provider) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO providers (id, role, adapter, endpoint, model, credential_ref, data_categories) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT (id) DO UPDATE SET role = excluded.role, adapter = excluded.adapter, \
                 endpoint = excluded.endpoint, model = excluded.model, \
                 credential_ref = excluded.credential_ref, data_categories = excluded.data_categories",
                params![
                    provider.id().to_string(),
                    provider.role().code(),
                    provider.adapter().code(),
                    provider.endpoint().map(ToString::to_string),
                    provider.model(),
                    provider.credential_ref().map(CredentialRef::as_str),
                    categories_column(provider),
                ],
            )
            .map(|_| ())
            .map_err(|e| storage_error(&e))
        })
    }

    fn latest_authorization(
        &self,
        provider: ProviderId,
    ) -> Result<Option<ProviderAuthorization>, RepositoryError> {
        let row = self.db.with_conn(|conn| {
            conn.query_row(
                &format!(
                    "SELECT {AUTH_COLUMNS} FROM provider_authorizations \
                     WHERE provider_id = ?1 ORDER BY id DESC LIMIT 1"
                ),
                params![provider.to_string()],
                AuthorizationRow::read,
            )
            .optional()
            .map_err(|e| storage_error(&e))
        })?;
        row.map(AuthorizationRow::into_authorization).transpose()
    }

    fn save_authorization(
        &self,
        authorization: &ProviderAuthorization,
    ) -> Result<(), RepositoryError> {
        let provider_id = authorization.provider_id().to_string();
        self.db.with_conn(|conn| {
            let exists = conn
                .query_row(
                    "SELECT 1 FROM providers WHERE id = ?1",
                    params![provider_id],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| storage_error(&e))?;
            if exists.is_none() {
                return Err(RepositoryError::NotFound);
            }
            let changed = match authorization.state() {
                AuthorizationState::Granted => conn.execute(
                    &format!(
                        "INSERT INTO provider_authorizations ({AUTH_COLUMNS}) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                    ),
                    params![
                        provider_id,
                        authorization.state().code(),
                        authorization.destination(),
                        join_categories(authorization.data_categories()),
                        authorization.retention_notice(),
                        authorization.granted_at().unix_millis(),
                        authorization.revoked_at().map(|t| t.unix_millis()),
                    ],
                ),
                AuthorizationState::Revoked => conn.execute(
                    "UPDATE provider_authorizations SET state = ?2, revoked_at = ?3 \
                     WHERE id = (SELECT max(id) FROM provider_authorizations WHERE provider_id = ?1)",
                    params![
                        provider_id,
                        authorization.state().code(),
                        authorization.revoked_at().map(|t| t.unix_millis()),
                    ],
                ),
            }
            .map_err(|e| storage_error(&e))?;
            if changed == 0 {
                return Err(RepositoryError::NotFound);
            }
            Ok(())
        })
    }
}
