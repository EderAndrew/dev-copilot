//! Encrypted local persistence (SQLCipher) and the OS credential store.
//!
//! Session content is only written encrypted, with the key held in the OS credential store
//! (FR-039a).
