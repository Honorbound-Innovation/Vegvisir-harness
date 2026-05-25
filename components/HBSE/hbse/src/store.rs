#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::audit::AuditEvent;
use crate::policy::AccessPolicy;
use crate::records::{SecretRecord, SecretStatus};
use crate::rotation::RotationJob;
use crate::tickets::SecretAccessTicket;

pub const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("vault is not initialized")]
    VaultNotInitialized,
    #[error("vault is already initialized")]
    VaultAlreadyInitialized,
    #[error("secret not found: {0}")]
    SecretNotFound(String),
    #[error("ticket changed during consume: {0}")]
    TicketConflict(String),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultHeader {
    #[serde(default = "schema_version")]
    pub schema_version: u64,
    pub vault_id: String,
    pub namespace_id: String,
    pub provider_binding: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretSummary {
    pub secret_ref: String,
    pub secret_id: String,
    pub namespace_id: String,
    pub latest_version: u64,
    pub status: String,
    pub secret_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SQLiteVaultStore {
    path: PathBuf,
}

impl SQLiteVaultStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize_schema(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
            harden_dir_permissions(parent)?;
        }
        let conn = self.connect()?;
        harden_vault_file_permissions(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS vault_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS secret_records (
                secret_ref TEXT NOT NULL,
                secret_id TEXT NOT NULL,
                namespace_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                status TEXT NOT NULL,
                secret_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                record_json TEXT NOT NULL,
                PRIMARY KEY (secret_ref, version)
            );
            CREATE INDEX IF NOT EXISTS idx_secret_records_ref_version
                ON secret_records(secret_ref, version DESC);
            CREATE TABLE IF NOT EXISTS policies (
                policy_id TEXT PRIMARY KEY,
                policy_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tickets (
                ticket_id TEXT PRIMARY KEY,
                ticket_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_events (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL UNIQUE,
                event_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS redaction_fingerprints (
                secret_ref TEXT NOT NULL,
                secret_version INTEGER NOT NULL,
                fingerprint TEXT NOT NULL,
                PRIMARY KEY (secret_ref, secret_version)
            );
            CREATE TABLE IF NOT EXISTS rotation_jobs (
                job_id TEXT PRIMARY KEY,
                secret_ref TEXT NOT NULL,
                status TEXT NOT NULL,
                job_json TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn create_vault(&self, header: &VaultHeader) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        if get_metadata(&conn, "vault_header")?.is_some() {
            return Err(StoreError::VaultAlreadyInitialized);
        }
        conn.execute(
            "INSERT INTO vault_metadata (key, value) VALUES (?, ?)",
            params!["vault_header", serde_json::to_string(header)?],
        )?;
        Ok(())
    }

    pub fn load_header(&self) -> Result<VaultHeader, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let raw = get_metadata(&conn, "vault_header")?.ok_or(StoreError::VaultNotInitialized)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn update_header(&self, header: &VaultHeader) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        if get_metadata(&conn, "vault_header")?.is_none() {
            return Err(StoreError::VaultNotInitialized);
        }
        conn.execute(
            "UPDATE vault_metadata SET value = ? WHERE key = ?",
            params![serde_json::to_string(header)?, "vault_header"],
        )?;
        Ok(())
    }

    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        get_metadata(&conn, key)
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO vault_metadata (key, value) VALUES (?, ?)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn save_secret_record(&self, record: &SecretRecord) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        if get_metadata(&conn, "vault_header")?.is_none() {
            return Err(StoreError::VaultNotInitialized);
        }
        conn.execute(
            r#"
            INSERT OR REPLACE INTO secret_records (
                secret_ref, secret_id, namespace_id, version, status, secret_type, created_at, record_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                record.secret_ref,
                record.secret_id,
                record.namespace_id,
                record.secret_version,
                enum_json_string(&record.status)?,
                enum_json_string(&record.secret_type)?,
                record.created_at,
                serde_json::to_string(record)?,
            ],
        )?;
        Ok(())
    }

    pub fn latest_version(&self, secret_ref: &str) -> Result<Option<u64>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let value: Option<u64> = conn.query_row(
            "SELECT MAX(version) FROM secret_records WHERE secret_ref = ?",
            params![secret_ref],
            |row| row.get(0),
        )?;
        Ok(value)
    }

    pub fn load_latest_secret(&self, secret_ref: &str) -> Result<SecretRecord, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let raw: Option<String> = conn
            .query_row(
                r#"
                SELECT record_json FROM secret_records
                WHERE secret_ref = ?
                ORDER BY version DESC
                LIMIT 1
                "#,
                params![secret_ref],
                |row| row.get(0),
            )
            .optional()?;
        let raw = raw.ok_or_else(|| StoreError::SecretNotFound(secret_ref.to_string()))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn load_secret_version(
        &self,
        secret_ref: &str,
        version: u64,
    ) -> Result<SecretRecord, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let raw: Option<String> = conn
            .query_row(
                r#"
                SELECT record_json FROM secret_records
                WHERE secret_ref = ? AND version = ?
                "#,
                params![secret_ref, version],
                |row| row.get(0),
            )
            .optional()?;
        let raw = raw.ok_or_else(|| StoreError::SecretNotFound(secret_ref.to_string()))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_updated_secret_status(
        &self,
        secret_ref: &str,
        status: SecretStatus,
    ) -> Result<SecretRecord, StoreError> {
        let mut record = self.load_latest_secret(secret_ref)?;
        record.status = status;
        self.save_secret_record(&record)?;
        Ok(record)
    }

    pub fn list_secrets(&self) -> Result<Vec<SecretSummary>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT sr.secret_ref, sr.secret_id, sr.namespace_id, sr.version, sr.status,
                   sr.secret_type, sr.created_at
            FROM secret_records sr
            JOIN (
                SELECT secret_ref, MAX(version) AS version
                FROM secret_records
                GROUP BY secret_ref
            ) latest
            ON sr.secret_ref = latest.secret_ref AND sr.version = latest.version
            ORDER BY sr.secret_ref
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SecretSummary {
                secret_ref: row.get(0)?,
                secret_id: row.get(1)?,
                namespace_id: row.get(2)?,
                latest_version: row.get(3)?,
                status: row.get(4)?,
                secret_type: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn save_audit_event(&self, event: &AuditEvent) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO audit_events (event_id, event_json) VALUES (?, ?)",
            params![event.event_id, serde_json::to_string(event)?],
        )?;
        Ok(())
    }

    pub fn append_audit_event<F>(&self, build: F) -> Result<AuditEvent, StoreError>
    where
        F: FnOnce(Vec<AuditEvent>) -> AuditEvent,
    {
        self.initialize_schema()?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let events = {
            let mut stmt = tx.prepare("SELECT event_json FROM audit_events ORDER BY sequence")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let raw = rows.collect::<Result<Vec<_>, _>>()?;
            raw.into_iter()
                .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
                .collect::<Result<Vec<_>, _>>()?
        };
        let event = build(events);
        tx.execute(
            "INSERT INTO audit_events (event_id, event_json) VALUES (?, ?)",
            params![event.event_id, serde_json::to_string(&event)?],
        )?;
        tx.commit()?;
        Ok(event)
    }

    pub fn list_audit_events(&self) -> Result<Vec<AuditEvent>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT event_json FROM audit_events ORDER BY sequence")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn save_policy(&self, policy: &AccessPolicy) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO policies (policy_id, policy_json) VALUES (?, ?)",
            params![policy.policy_id, serde_json::to_string(policy)?],
        )?;
        Ok(())
    }

    pub fn list_policies(&self) -> Result<Vec<AccessPolicy>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT policy_json FROM policies ORDER BY policy_id")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn save_ticket(&self, ticket: &SecretAccessTicket) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO tickets (ticket_id, ticket_json) VALUES (?, ?)",
            params![ticket.ticket_id, serde_json::to_string(ticket)?],
        )?;
        Ok(())
    }

    pub fn save_ticket_if_current(
        &self,
        expected: &SecretAccessTicket,
        updated: &SecretAccessTicket,
    ) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE tickets SET ticket_json = ? WHERE ticket_id = ? AND ticket_json = ?",
            params![
                serde_json::to_string(updated)?,
                expected.ticket_id,
                serde_json::to_string(expected)?,
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::TicketConflict(expected.ticket_id.clone()))
        }
    }

    pub fn load_ticket(&self, ticket_id: &str) -> Result<Option<SecretAccessTicket>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT ticket_json FROM tickets WHERE ticket_id = ?",
                params![ticket_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub fn list_tickets(&self) -> Result<Vec<SecretAccessTicket>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT ticket_json FROM tickets ORDER BY ticket_id")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn save_rotation_job(&self, job: &RotationJob) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT OR REPLACE INTO rotation_jobs (job_id, secret_ref, status, job_json) VALUES (?, ?, ?, ?)",
            params![
                job.job_id,
                job.secret_ref,
                enum_json_string(&job.status)?,
                serde_json::to_string(job)?,
            ],
        )?;
        Ok(())
    }

    pub fn load_rotation_job(&self, job_id: &str) -> Result<Option<RotationJob>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT job_json FROM rotation_jobs WHERE job_id = ?",
                params![job_id],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub fn list_rotation_jobs(&self) -> Result<Vec<RotationJob>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT job_json FROM rotation_jobs ORDER BY job_id")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .collect()
    }

    pub fn count_redaction_fingerprints(&self) -> Result<u64, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let count: u64 =
            conn.query_row("SELECT COUNT(*) FROM redaction_fingerprints", [], |row| {
                row.get(0)
            })?;
        Ok(count)
    }

    pub fn save_redaction_fingerprint(
        &self,
        secret_ref: &str,
        secret_version: u64,
        fingerprint: &str,
    ) -> Result<(), StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT OR REPLACE INTO redaction_fingerprints
                (secret_ref, secret_version, fingerprint)
            VALUES (?, ?, ?)
            "#,
            params![secret_ref, secret_version, fingerprint],
        )?;
        Ok(())
    }

    pub fn list_redaction_fingerprints(&self) -> Result<Vec<String>, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT fingerprint FROM redaction_fingerprints ORDER BY secret_ref, secret_version",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    pub fn integrity_check(&self) -> Result<String, StoreError> {
        self.initialize_schema()?;
        let conn = self.connect()?;
        let result: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(result)
    }

    fn connect(&self) -> Result<Connection, StoreError> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }
}

#[cfg(unix)]
fn harden_dir_permissions(path: &Path) -> Result<(), StoreError> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn harden_dir_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

#[cfg(unix)]
fn harden_vault_file_permissions(path: &Path) -> Result<(), StoreError> {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if candidate.exists() {
            std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn harden_vault_file_permissions(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

fn get_metadata(conn: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    conn.query_row(
        "SELECT value FROM vault_metadata WHERE key = ?",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(StoreError::from)
}

fn schema_version() -> u64 {
    SCHEMA_VERSION
}

fn enum_json_string<T: Serialize>(value: &T) -> Result<String, StoreError> {
    let value = serde_json::to_value(value)?;
    Ok(value.as_str().unwrap_or_default().to_string())
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::crypto::{hash_bytes, CryptoEngine};
    use crate::keys::{KeyHierarchy, KEY_SIZE};
    use crate::records::{SecretStatus, SecretType};

    #[test]
    fn store_round_trips_header_and_secret_summary() {
        let dir = tempdir().unwrap();
        let store = SQLiteVaultStore::new(dir.path().join("vault.db"));
        let header = VaultHeader {
            schema_version: SCHEMA_VERSION,
            vault_id: "vault".to_string(),
            namespace_id: "default".to_string(),
            provider_binding: serde_json::json!({"provider_id": "test"}),
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
        };
        store.create_vault(&header).unwrap();
        assert_eq!(store.load_header().unwrap(), header);

        let keys = KeyHierarchy::new("vault".to_string(), &[7u8; KEY_SIZE]).unwrap();
        let record = CryptoEngine.encrypt_secret(
            &keys,
            "default",
            "secret-id",
            "secret://example",
            1,
            b"value",
            SecretType::Generic,
            "default-deny",
            "policy-hash",
            &hash_bytes(b"secret://example"),
            "unbound-provider-policy",
        );
        store.save_secret_record(&record).unwrap();

        assert_eq!(store.latest_version("secret://example").unwrap(), Some(1));
        assert_eq!(
            store
                .load_latest_secret("secret://example")
                .unwrap()
                .secret_ref,
            "secret://example"
        );
        let summaries = store.list_secrets().unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].status, "active");

        store
            .save_updated_secret_status("secret://example", SecretStatus::Disabled)
            .unwrap();
        assert_eq!(
            store.load_latest_secret("secret://example").unwrap().status,
            SecretStatus::Disabled
        );
        assert_eq!(store.list_secrets().unwrap()[0].status, "disabled");
    }

    #[test]
    fn store_round_trips_audit_events() {
        let dir = tempdir().unwrap();
        let store = SQLiteVaultStore::new(dir.path().join("vault.db"));
        store.initialize_schema().unwrap();
        let event = AuditEvent {
            event_id: "event".to_string(),
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            vault_id: "vault".to_string(),
            namespace_id: "default".to_string(),
            event_type: "test".to_string(),
            severity: "info".to_string(),
            decision: "allow".to_string(),
            previous_hash: crate::audit::ZERO_HASH.to_string(),
            event_hash: "hash".to_string(),
            event_mac: "mac".to_string(),
            metadata: serde_json::Map::new(),
        };
        store.save_audit_event(&event).unwrap();
        assert_eq!(store.list_audit_events().unwrap(), vec![event]);
    }
}
