use crate::audit;
use crate::models::{Host, HostUpsert};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

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
    c.execute_batch(include_str!("../migrations/001_init.sql"))?;
    c.execute(
        "UPDATE sftp_operations SET status='completed' WHERE status='succeeded'",
        [],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO schema_migrations(version,checksum,applied_at) VALUES(?,?,?)",
        params![1, "001_init_sql_v2", Utc::now().to_rfc3339()],
    )?;
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
pub fn has_active_session(c: &Connection, host_id: &str) -> rusqlite::Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE host_id=? AND status IN('connecting','ready','reconnecting'))",[host_id],|r|r.get(0))
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
    c.execute("INSERT INTO hosts(id,name,connection_type,address,port,username,auth_method,group_name,is_production,policy_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,connection_type=excluded.connection_type,address=excluded.address,port=excluded.port,username=excluded.username,auth_method=excluded.auth_method,group_name=excluded.group_name,is_production=excluded.is_production,policy_id=excluded.policy_id,updated_at=excluded.updated_at", params![id,h.name,h.connection_type,h.address,h.port,h.username,h.auth_method,h.group_name,h.is_production as i32,h.policy_id,now,now])?;
    Ok(id)
}
pub fn delete(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(c.execute(
        "UPDATE hosts SET deleted_at=? WHERE id=? AND deleted_at IS NULL",
        params![Utc::now().to_rfc3339(), id],
    )? == 1)
}
