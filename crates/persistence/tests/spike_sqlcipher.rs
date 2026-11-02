//! Spike S3 (T011): SQLCipher + OS credential store on macOS, Windows and Linux.
//!
//! Throwaway verification recorded in `docs/adr/ADR-004.md`. The SQLCipher checks always run;
//! the credential-store round trip touches the real OS vault and only runs when
//! `DEVCOPILOT_KEYRING_SPIKE=1` (set by CI, where the vault is provisioned).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use rusqlite::Connection;

fn hex_key(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn open_keyed(path: &std::path::Path, key_hex: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(&format!("PRAGMA key = \"x'{key_hex}'\";"))?;
    Ok(conn)
}

fn write_and_read_back(key_hex: &str) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spike.db");
    let secret_phrase = "frase-sigilosa-do-spike";

    {
        let conn = open_keyed(&path, key_hex).unwrap();
        let version: String = conn
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .unwrap();
        assert!(!version.is_empty(), "SQLCipher must be linked");
        conn.execute_batch("CREATE TABLE t (v TEXT NOT NULL)")
            .unwrap();
        conn.execute("INSERT INTO t (v) VALUES (?1)", [secret_phrase])
            .unwrap();
    }

    // At rest, neither the phrase nor the SQLite header are readable.
    let raw = std::fs::read(&path).unwrap();
    assert!(
        !raw.windows(secret_phrase.len())
            .any(|w| w == secret_phrase.as_bytes())
    );
    assert!(!raw.starts_with(b"SQLite format 3"));

    // The right key reads the data back.
    let conn = open_keyed(&path, key_hex).unwrap();
    let value: String = conn
        .query_row("SELECT v FROM t", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, secret_phrase);
    drop(conn);

    // A wrong key cannot read it.
    let wrong = open_keyed(&path, &"00".repeat(32)).unwrap();
    assert!(
        wrong
            .query_row("SELECT count(*) FROM t", [], |row| row.get::<_, i64>(0))
            .is_err()
    );
}

#[test]
fn sqlcipher_encrypts_with_raw_256_bit_key() {
    let key: Vec<u8> = (0u8..32)
        .map(|i| i.wrapping_mul(37).wrapping_add(11))
        .collect();
    write_and_read_back(&hex_key(&key));
}

#[test]
fn key_round_trips_through_os_credential_store() {
    if std::env::var("DEVCOPILOT_KEYRING_SPIKE").as_deref() != Ok("1") {
        eprintln!("skipped: set DEVCOPILOT_KEYRING_SPIKE=1 to touch the OS credential store");
        return;
    }
    let entry = keyring::Entry::new("devcopilot-spike-s3", "db-key").unwrap();
    let key: Vec<u8> = (0u8..32)
        .map(|i| i.wrapping_mul(91).wrapping_add(7))
        .collect();
    entry.set_secret(&key).unwrap();
    let stored = entry.get_secret().unwrap();
    entry.delete_credential().unwrap();
    assert_eq!(stored, key);
    write_and_read_back(&hex_key(&stored));
}
