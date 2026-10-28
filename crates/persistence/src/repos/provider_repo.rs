//! [`ProviderRepository`] over the `providers` table (authorizations are added in US7, T135).

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ports::{ProviderRepository, RepositoryError};
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    CredentialRef, DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
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
        let data_categories = self
            .data_categories
            .split(',')
            .filter(|code| !code.is_empty())
            .map(DataCategory::parse)
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(corrupt)?;
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
}
