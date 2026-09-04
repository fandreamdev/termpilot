use crate::audit;
use crate::models::{Host, HostUpsert};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Debug)]
pub struct AuditRecord {
    pub event_id: String,
    pub event_type: String,
    pub severity: String,
    pub actor: String,
    pub target_host_id: Option<String>,
    pub session_id: Option<String>,
    pub correlation_id: String,
    pub hash: String,
    pub created_at: String,
}

type AuditListener = Arc<dyn Fn(AuditRecord) + Send + Sync>;
static AUDIT_LISTENER: OnceLock<Mutex<Option<AuditListener>>> = OnceLock::new();

pub fn set_audit_listener(listener: AuditListener) {
    let slot = AUDIT_LISTENER.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = Some(listener);
    }
}

pub type SftpOperation = (String, String, Option<String>, Option<String>, String);

pub fn open() -> rusqlite::Result<Connection> {
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = dir.join("TermPilot").join("data");
    fs::create_dir_all(&dir).ok();
    let c = Connection::open(dir.join("termpilot.db"))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "foreign_keys", true)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch("BEGIN IMMEDIATE;")?;
    if let Err(error) = c.execute_batch(include_str!("../migrations/001_init.sql")) {
        let _ = c.execute_batch("ROLLBACK;");
        return Err(error);
    }
    c.execute_batch("COMMIT;")?;
    c.execute(
        "UPDATE sftp_operations SET status='completed' WHERE status='succeeded'",
        [],
    )?;
    let checksum = hex::encode(sha2::Sha256::digest(
        include_str!("../migrations/001_init.sql").as_bytes(),
    ));
    let existing: Option<String> = c
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=1",
            [],
            |r| r.get(0),
        )
        .ok();
    if let Some(previous) = existing {
        // Databases created by the initial development build used a marker rather
        // than a content hash. Upgrade that marker once; reject unknown drift.
        if previous != checksum && previous != "001_init_sql_v2" {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    c.execute(
        "INSERT INTO schema_migrations(version,checksum,applied_at) VALUES(?,?,?) ON CONFLICT(version) DO UPDATE SET checksum=excluded.checksum",
        params![1, checksum, Utc::now().to_rfc3339()],
    )?;
    let migration2 = include_str!("../migrations/002_status_completed.sql");
    let checksum2 = hex::encode(sha2::Sha256::digest(migration2.as_bytes()));
    let existing2: Option<String> = c
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=2",
            [],
            |r| r.get(0),
        )
        .ok();
    if let Some(previous) = existing2 {
        if previous != checksum2 {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    c.execute_batch(migration2)?;
    c.execute(
        "INSERT OR IGNORE INTO schema_migrations(version,checksum,applied_at) VALUES(?,?,?)",
        params![2, checksum2, Utc::now().to_rfc3339()],
    )?;
    let now = Utc::now().to_rfc3339();
    c.execute("INSERT OR IGNORE INTO app_settings(key,value,value_type,updated_at) VALUES('theme','dark','string',?)", params![now])?;
    c.execute("INSERT OR IGNORE INTO app_settings(key,value,value_type,updated_at) VALUES('audit.retention_days','90','integer',?)", params![now])?;
    c.execute("INSERT OR IGNORE INTO app_settings(key,value,value_type,updated_at) VALUES('remote.default_path','~','string',?)", params![now])?;
    Ok(c)
}
pub fn hosts(c: &Connection) -> rusqlite::Result<Vec<Host>> {
    let mut s = c.prepare("SELECT id,name,connection_type,address,port,username,auth_method,group_name,is_production,endpoint_fingerprint FROM hosts WHERE deleted_at IS NULL ORDER BY name")?;
    let rows = s.query_map([], |r| {
        Ok(Host {
            id: r.get(0)?,
            name: r.get(1)?,
            connection_type: r.get(2)?,
            address: r.get(3)?,
            port: r.get(4)?,
            username: r.get(5)?,
            auth_method: r.get(6)?,
            group_name: r.get(7)?,
            is_production: r.get::<_, i64>(8)? != 0,
            endpoint_fingerprint: r.get(9)?,
        })
    })?;
    rows.collect()
}
pub fn hosts_filtered(
    c: &Connection,
    query: Option<&str>,
    group: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<Host>> {
    let mut items = hosts(c)?;
    items.retain(|h| {
        let query_ok = query
            .map(|q| {
                let q = q.to_lowercase();
                format!("{} {} {}", h.name, h.address, h.username)
                    .to_lowercase()
                    .contains(&q)
            })
            .unwrap_or(true);
        let group_ok = group
            .map(|g| h.group_name.as_deref() == Some(g))
            .unwrap_or(true);
        query_ok && group_ok
    });
    items.truncate(limit);
    Ok(items)
}
pub fn host_exists(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM hosts WHERE id=? AND deleted_at IS NULL)",
        [id],
        |r| r.get(0),
    )
}
pub fn session_exists(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=? AND status NOT IN('closed','disconnected','error'))",[id],|r|r.get(0))
}
pub fn session_context(
    c: &Connection,
    id: &str,
) -> rusqlite::Result<(String, String, String, String)> {
    c.query_row(
        "SELECT h.username,h.address,h.name,COALESCE(s.observed_remote_identity_hmac,'') FROM sessions s JOIN hosts h ON h.id=s.host_id WHERE s.id=? AND s.status NOT IN('closed','disconnected','error')",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
}
pub fn session_policy(c: &Connection, id: &str) -> rusqlite::Result<(String, i64, String)> {
    c.query_row(
        "SELECT p.id,p.version,p.mode FROM sessions s JOIN hosts h ON h.id=s.host_id JOIN security_policies p ON p.id=h.policy_id WHERE s.id=?",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
}
pub fn policy(c: &Connection, id: &str) -> rusqlite::Result<(i64, String, String)> {
    c.query_row(
        "SELECT version,allow_rules_json,mode FROM security_policies WHERE id=? AND is_active=1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
}
pub fn host_policy_exists(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM security_policies WHERE id=? AND is_active=1)",
        [id],
        |r| r.get(0),
    )
}
pub fn credential_host_exists(c: &Connection, host_id: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM hosts WHERE id=? AND deleted_at IS NULL)",
        [host_id],
        |r| r.get(0),
    )
}
pub fn update_session_observed(
    c: &Connection,
    id: &str,
    fingerprint: Option<&str>,
    identity: Option<&str>,
) -> rusqlite::Result<usize> {
    c.execute("UPDATE sessions SET observed_endpoint_fingerprint=?,observed_remote_identity_hmac=? WHERE id=?", params![fingerprint,identity,id])
}
pub fn has_active_session(c: &Connection, host_id: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE host_id=? AND status IN('connecting','ready','reconnecting'))",[host_id],|r|r.get(0))
}
pub fn active_session_count(c: &Connection) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT COUNT(*) FROM sessions WHERE status IN('connecting','ready','reconnecting')",
        [],
        |r| r.get(0),
    )
}
pub fn session_host_is_production(c: &Connection, session_id: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT COALESCE(h.is_production,0) FROM sessions s JOIN hosts h ON h.id=s.host_id WHERE s.id=?", [session_id], |r| Ok(r.get::<_, i64>(0)? != 0))
}
pub fn session_host_fingerprint(
    c: &Connection,
    session_id: &str,
) -> rusqlite::Result<Option<String>> {
    c.query_row(
        "SELECT h.endpoint_fingerprint FROM sessions s JOIN hosts h ON h.id=s.host_id WHERE s.id=?",
        [session_id],
        |r| r.get(0),
    )
}
pub fn resize_session(c: &Connection, id: &str, rows: i64, cols: i64) -> rusqlite::Result<usize> {
    let n=c.execute("UPDATE sessions SET pty_rows=?,pty_cols=? WHERE id=? AND status NOT IN('closed','disconnected','error')",params![rows,cols,id])?;
    if n == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(n)
}
pub fn disconnect_session(
    c: &Connection,
    id: &str,
    reason: Option<&str>,
) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE sessions SET status='closed',ended_at=?,disconnect_reason=? WHERE id=? AND status NOT IN('closed','disconnected')",params![Utc::now().to_rfc3339(),reason,id])? == 1)
}
pub fn close_all_sessions(c: &Connection, reason: &str) -> rusqlite::Result<usize> {
    c.execute("UPDATE sessions SET status='closed',ended_at=?,disconnect_reason=? WHERE status IN('connecting','ready','reconnecting')", params![Utc::now().to_rfc3339(),reason])
}
pub fn append_audit(
    c: &Connection,
    event_type: &str,
    severity: &str,
    actor: &str,
    target_host_id: Option<&str>,
    session_id: Option<&str>,
    payload: &Value,
) -> rusqlite::Result<String> {
    let prev: String = c
        .query_row(
            "SELECT hash FROM audit_logs ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| audit::GENESIS.to_owned());
    let event_id = uuid::Uuid::new_v4().to_string();
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let canonical = json!({"event_id":event_id,"event_type":event_type,"severity":severity,"actor":actor,"target_host_id":target_host_id,"session_id":session_id,"correlation_id":correlation_id,"payload":payload,"created_at":created_at});
    let hash = audit::chain_hash(&canonical, &prev);
    c.execute("INSERT INTO audit_logs(event_id,event_type,severity,actor,target_host_id,session_id,correlation_id,payload_json,prev_hash,hash,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)", params![event_id,event_type,severity,actor,target_host_id,session_id,correlation_id,serde_json::to_string(payload).unwrap_or_else(|_|"{}".into()),prev,hash,created_at])?;
    let record = AuditRecord {
        event_id,
        event_type: event_type.to_owned(),
        severity: severity.to_owned(),
        actor: actor.to_owned(),
        target_host_id: target_host_id.map(str::to_owned),
        session_id: session_id.map(str::to_owned),
        correlation_id,
        hash: hash.clone(),
        created_at,
    };
    let listener = AUDIT_LISTENER
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|current| current.clone()));
    if let Some(listener) = listener {
        listener(record);
    }
    Ok(hash)
}
pub fn insert_sftp(
    c: &Connection,
    id: &str,
    session_id: &str,
    op: &str,
    src: Option<&str>,
    dst: Option<&str>,
) -> rusqlite::Result<usize> {
    if !session_exists(c, session_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    c.execute("INSERT INTO sftp_operations(id,session_id,operation,source_path,destination_path,status,created_at) VALUES(?,?,?,?,?,'queued',?)",params![id,session_id,op,src,dst,Utc::now().to_rfc3339()])
}
pub fn sftp_operation(c: &Connection, id: &str) -> rusqlite::Result<SftpOperation> {
    c.query_row("SELECT session_id,operation,source_path,destination_path,status FROM sftp_operations WHERE id=?", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
}
pub fn update_sftp_progress(
    c: &Connection,
    id: &str,
    transferred: i64,
    size: Option<i64>,
    content_hash: Option<&str>,
    error_code: Option<&str>,
) -> rusqlite::Result<usize> {
    c.execute("UPDATE sftp_operations SET transferred_bytes=?,size_bytes=COALESCE(?,size_bytes),content_hash=COALESCE(?,content_hash),error_code=? WHERE id=?", params![transferred,size,content_hash,error_code,id])
}
pub fn update_sftp_status(c: &Connection, id: &str, status: &str) -> rusqlite::Result<usize> {
    let n=c.execute("UPDATE sftp_operations SET status=?,started_at=CASE WHEN ?='running' AND started_at IS NULL THEN ? ELSE started_at END,ended_at=CASE WHEN ? IN('completed','failed','cancelled') THEN ? ELSE ended_at END WHERE id=? AND status NOT IN('completed','failed','cancelled')",params![status,status,Utc::now().to_rfc3339(),status,Utc::now().to_rfc3339(),id])?;
    if n == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(n)
}
pub fn upsert(c: &Connection, h: &HostUpsert) -> rusqlite::Result<String> {
    let id =
        h.id.clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let now = Utc::now().to_rfc3339();
    c.execute("INSERT INTO hosts(id,name,connection_type,address,port,username,auth_method,group_name,is_production,endpoint_fingerprint,policy_id,notes,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,connection_type=excluded.connection_type,address=excluded.address,port=excluded.port,username=excluded.username,auth_method=excluded.auth_method,group_name=excluded.group_name,is_production=CASE WHEN hosts.is_production=1 THEN 1 ELSE excluded.is_production END,endpoint_fingerprint=COALESCE(excluded.endpoint_fingerprint,hosts.endpoint_fingerprint),policy_id=excluded.policy_id,notes=excluded.notes,updated_at=excluded.updated_at", params![id,h.name,h.connection_type,h.address,h.port,h.username,h.auth_method,h.group_name,h.is_production as i32,h.endpoint_fingerprint,h.policy_id,h.notes,now,now])?;
    Ok(id)
}
pub fn delete(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(c.execute(
        "UPDATE hosts SET deleted_at=? WHERE id=? AND deleted_at IS NULL",
        params![Utc::now().to_rfc3339(), id],
    )? == 1)
}

pub fn set_setting(
    c: &Connection,
    key: &str,
    value: &str,
    value_type: &str,
) -> rusqlite::Result<()> {
    if !matches!(value_type, "string" | "integer" | "boolean" | "json") {
        return Err(rusqlite::Error::InvalidParameterName("setting type".into()));
    }
    let lower = key.to_ascii_lowercase();
    if [
        "password",
        "token",
        "secret",
        "private_key",
        "api_key",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "secret setting".into(),
        ));
    }
    let value_lower = value.to_ascii_lowercase();
    if value_lower.contains("-----begin")
        || value_lower.contains("password=")
        || value_lower.contains("token=")
        || value_lower.contains("api_key=")
    {
        return Err(rusqlite::Error::InvalidParameterName("secret value".into()));
    }
    if value_type == "json" {
        if let Ok(parsed) = serde_json::from_str::<Value>(value) {
            if json_contains_secret_key(&parsed) {
                return Err(rusqlite::Error::InvalidParameterName("secret json".into()));
            }
        }
    }
    if key.is_empty() || key.len() > 128 || value.len() > 16 * 1024 {
        return Err(rusqlite::Error::InvalidParameterName("setting".into()));
    }
    match value_type {
        "integer" if value.parse::<i64>().is_err() => {
            return Err(rusqlite::Error::InvalidParameterName("integer".into()))
        }
        "boolean" if !matches!(value, "true" | "false") => {
            return Err(rusqlite::Error::InvalidParameterName("boolean".into()))
        }
        "json" if serde_json::from_str::<Value>(value).is_err() => {
            return Err(rusqlite::Error::InvalidParameterName("json".into()))
        }
        _ => {}
    }
    c.execute("INSERT INTO app_settings(key,value,value_type,updated_at) VALUES(?,?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,value_type=excluded.value_type,updated_at=excluded.updated_at", params![key,value,value_type,Utc::now().to_rfc3339()])?;
    Ok(())
}

fn json_contains_secret_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, child)| {
            let key = key.to_ascii_lowercase();
            [
                "password",
                "passwd",
                "token",
                "secret",
                "private_key",
                "api_key",
            ]
            .iter()
            .any(|needle| key.contains(needle))
                || json_contains_secret_key(child)
        }),
        Value::Array(values) => values.iter().any(json_contains_secret_key),
        _ => false,
    }
}
pub fn get_settings(c: &Connection) -> rusqlite::Result<Vec<(String, String, String, String)>> {
    let mut s =
        c.prepare("SELECT key,value,value_type,updated_at FROM app_settings ORDER BY key")?;
    let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect()
}
pub fn backup(c: &Connection, path: &Path) -> rusqlite::Result<()> {
    if path.extension().and_then(|v| v.to_str()) != Some("db") {
        return Err(rusqlite::Error::InvalidPath(path.to_path_buf()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| rusqlite::Error::InvalidPath(parent.to_path_buf()))?;
    }
    let escaped = path.to_string_lossy().replace('\'', "''");
    c.execute_batch(&format!(
        "PRAGMA wal_checkpoint(TRUNCATE); VACUUM INTO '{}';",
        escaped
    ))
}

/// Validate a backup before replacing any live tables. A backup is accepted
/// only when it is a readable SQLite database containing the current schema
/// tables; this prevents accidentally importing an arbitrary `.db` file and
/// leaving the application in a partially restored state.
pub fn validate_backup(path: &Path) -> rusqlite::Result<()> {
    let backup = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    let required = [
        "schema_migrations",
        "app_settings",
        "security_policies",
        "hosts",
        "credential_refs",
        "sessions",
        "agent_conversations",
        "agent_messages",
        "command_approvals",
        "execution_records",
        "sftp_operations",
        "audit_logs",
        "audit_exports",
    ];
    for table in required {
        let exists: bool = backup.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
    }
    let migration_version: i64 = backup.query_row(
        "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    if migration_version < 2 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let expected_checksums = [
        (
            1_i64,
            hex::encode(sha2::Sha256::digest(
                include_str!("../migrations/001_init.sql").as_bytes(),
            )),
        ),
        (
            2_i64,
            hex::encode(sha2::Sha256::digest(
                include_str!("../migrations/002_status_completed.sql").as_bytes(),
            )),
        ),
    ];
    for (version, expected) in expected_checksums {
        let checksum: String = backup.query_row(
            "SELECT checksum FROM schema_migrations WHERE version=?",
            [version],
            |r| r.get(0),
        )?;
        if checksum != expected && !(version == 1 && checksum == "001_init_sql_v2") {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_audit_chain_are_usable() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/001_init.sql"))
            .unwrap();
        let first = append_audit(
            &c,
            "test.one",
            "info",
            "user",
            None,
            None,
            &json!({"ok":true}),
        )
        .unwrap();
        let second =
            append_audit(&c, "test.two", "info", "user", None, None, &json!({"n":2})).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM audit_logs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            c.query_row(
                "SELECT prev_hash FROM audit_logs ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            first
        );
    }

    #[test]
    fn backup_validation_rejects_arbitrary_sqlite_files() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/001_init.sql"))
            .unwrap();
        c.execute_batch(include_str!("../migrations/002_status_completed.sql"))
            .unwrap();
        c.execute(
            "INSERT INTO schema_migrations(version,checksum,applied_at) VALUES(1,?,datetime('now'))",
            [hex::encode(sha2::Sha256::digest(include_str!("../migrations/001_init.sql").as_bytes()))],
        )
        .unwrap();
        c.execute(
            "INSERT INTO schema_migrations(version,checksum,applied_at) VALUES(2,?,datetime('now'))",
            [hex::encode(sha2::Sha256::digest(include_str!("../migrations/002_status_completed.sql").as_bytes()))],
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!("termpilot-test-{}.db", uuid::Uuid::new_v4()));
        backup(&c, &path).unwrap();
        assert!(validate_backup(&path).is_ok());
        let bad = path.with_file_name(format!("{}.bad.db", uuid::Uuid::new_v4()));
        Connection::open(&bad).unwrap();
        assert!(validate_backup(&bad).is_err());
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(bad);
    }

    #[test]
    fn settings_reject_secret_json() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../migrations/001_init.sql"))
            .unwrap();
        assert!(set_setting(&c, "model", r#"{"token":"abc"}"#, "json").is_err());
        assert!(set_setting(&c, "model", r#"{"provider":"ollama"}"#, "json").is_ok());
    }
}
