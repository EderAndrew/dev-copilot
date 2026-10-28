//! Encrypted local persistence (SQLCipher) and the OS credential store.
//!
//! Session content is only written encrypted, with the key held in the OS credential store
//! (FR-039a).

pub mod db;
pub mod migrations;
pub mod repos;
pub mod secret_store;

pub use db::Database;
pub use repos::provider_repo::SqliteProviderRepository;
pub use repos::segment_repo::SqliteSegmentRepository;
pub use repos::session_repo::SqliteSessionRepository;
pub use repos::settings_repo::SqliteSettingsRepository;
pub use secret_store::KeyringSecretStore;
