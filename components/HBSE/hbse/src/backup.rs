use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::serialization::{b64url_no_padding, utc_millis};
use crate::store::{SQLiteVaultStore, StoreError};

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("{0}")]
    Store(#[from] StoreError),
    #[error("vault database does not exist")]
    VaultDatabaseMissing,
    #[error("backup has unexpected contents")]
    UnexpectedContents,
    #[error("backup database hash mismatch")]
    DatabaseHashMismatch,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u64,
    pub created_at: String,
    pub vault_id: String,
    pub database_sha256: String,
    #[serde(default)]
    pub contains_plaintext_secrets: bool,
    #[serde(default)]
    pub contains_plaintext_root_key: bool,
}

pub fn create_backup(
    store: &SQLiteVaultStore,
    destination: impl AsRef<Path>,
) -> Result<BackupManifest, BackupError> {
    let header = store.load_header()?;
    if !store.path().exists() {
        return Err(BackupError::VaultDatabaseMissing);
    }
    checkpoint_database(store.path())?;
    let data = fs::read(store.path())?;
    let manifest = BackupManifest {
        format_version: 1,
        created_at: utc_millis(Utc::now()),
        vault_id: header.vault_id,
        database_sha256: b64url_no_padding(&Sha256::digest(&data)),
        contains_plaintext_secrets: false,
        contains_plaintext_root_key: false,
    };
    let destination = destination.as_ref();
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(destination)?;
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("manifest.json", options)?;
    archive.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    archive.start_file("vault.db", options)?;
    archive.write_all(&data)?;
    archive.finish()?;
    Ok(manifest)
}

pub fn restore_backup(
    source: impl AsRef<Path>,
    destination_db: impl AsRef<Path>,
) -> Result<BackupManifest, BackupError> {
    let file = File::open(source)?;
    let mut archive = ZipArchive::new(file)?;
    let names = archive
        .file_names()
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    if names != BTreeSet::from(["manifest.json".to_string(), "vault.db".to_string()]) {
        return Err(BackupError::UnexpectedContents);
    }

    let mut manifest_json = String::new();
    archive
        .by_name("manifest.json")?
        .read_to_string(&mut manifest_json)?;
    let manifest: BackupManifest = serde_json::from_str(&manifest_json)?;
    let mut db_data = Vec::new();
    archive.by_name("vault.db")?.read_to_end(&mut db_data)?;
    let actual_hash = b64url_no_padding(&Sha256::digest(&db_data));
    if actual_hash != manifest.database_sha256 {
        return Err(BackupError::DatabaseHashMismatch);
    }

    let destination_db = destination_db.as_ref();
    if let Some(parent) = destination_db.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = destination_db.with_extension(format!(
        "{}tmp",
        destination_db
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
    ));
    fs::write(&tmp, db_data)?;
    fs::rename(tmp, destination_db)?;
    Ok(manifest)
}

fn checkpoint_database(path: &Path) -> Result<(), BackupError> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "wal_checkpoint", "FULL")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::records::SecretType;
    use crate::vault::LocalVault;

    use super::*;

    #[test]
    fn backup_restore_preserves_encrypted_vault_without_plaintext() {
        let dir = tempdir().unwrap();
        let store = SQLiteVaultStore::new(dir.path().join("vault.db"));
        let vault = LocalVault::new(store.clone());
        vault.init("passphrase", "default").unwrap();
        vault
            .put_secret(
                "secret://backup",
                b"backup-secret",
                "passphrase",
                SecretType::Generic,
            )
            .unwrap();

        let backup_path = dir.path().join("backup.hbse.zip");
        let manifest = create_backup(&store, &backup_path).unwrap();
        assert!(!manifest.contains_plaintext_secrets);
        assert!(!fs::read(&backup_path)
            .unwrap()
            .windows(b"backup-secret".len())
            .any(|window| window == b"backup-secret"));

        let restored_store = SQLiteVaultStore::new(dir.path().join("restored.db"));
        restore_backup(&backup_path, restored_store.path()).unwrap();
        let restored_vault = LocalVault::new(restored_store);
        assert_eq!(
            restored_vault
                .get_secret("secret://backup", "passphrase")
                .unwrap(),
            b"backup-secret"
        );
    }
}
