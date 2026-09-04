#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]
mod audit;
mod config;
mod db;
mod model_client;
mod models;
mod policy;
mod transport;

use chrono::Utc;
use model_client::ModelClient;
use models::{Envelope, Host, HostUpsert, Session};
use serde_json::{json, Value};
use sha2::Digest;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{AppHandle, Emitter, State};
use transport::{SftpTransport, SshTransport};

struct AppState {
    db: Mutex<rusqlite::Connection>,
    emergency_stop: AtomicBool,
    _config: config::AppConfig,
    ssh: transport::MockSshTransport,
    sftp: transport::MockSftpTransport,
    model: model_client::MockModelClient,
}
fn val_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}
fn err<T: serde::Serialize>(code: &'static str, message: &'static str) -> Envelope<T> {
    Envelope::err(code, message)
}
fn reject_if_stopped<T: serde::Serialize>(state: &AppState) -> Option<Envelope<T>> {
    state
        .emergency_stop
        .load(Ordering::SeqCst)
        .then(|| err("EMERGENCY_STOP_ACTIVE", "急停状态已启用"))
}
fn valid_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('\0') && !path.split(['/', '\\']).any(|part| part == "..")
}

#[tauri::command]
fn host_list(state: State<'_, AppState>, request: Value) -> Envelope<Vec<Host>> {
    let page_size = request
        .get("page_size")
        .and_then(Value::as_u64)
        .unwrap_or(200);
    if !(1..=200).contains(&page_size) {
        return err("VALIDATION", "page_size 必须在 1-200 范围内");
    }
    let query = val_str(&request, "query");
    let group = val_str(&request, "group");
    let conn = state.db.lock().unwrap();
    db::hosts_filtered(
        &conn,
        query.as_deref(),
        group.as_deref(),
        page_size as usize,
    )
    .map(Envelope::ok)
    .unwrap_or_else(|_| err("INTERNAL", "读取主机失败"))
}

#[tauri::command]
fn host_upsert(state: State<'_, AppState>, request: HostUpsert) -> Envelope<String> {
    if let Err((c, m)) = policy::validate_host(&request.address, request.port, &request.username) {
        return err(c, m);
    }
    if request.name.trim().is_empty() || request.name.len() > 128 {
        return err("VALIDATION", "主机名称不能为空且不能超过 128 个字符");
    }
    if !matches!(
        request.connection_type.as_str(),
        "direct_ssh" | "bastion_endpoint"
    ) {
        return err("VALIDATION", "不支持的连接类型");
    }
    if !matches!(
        request.auth_method.as_str(),
        "password" | "private_key" | "ssh_agent"
    ) {
        return err("VALIDATION", "不支持的认证方式");
    }
    db::upsert(&state.db.lock().unwrap(), &request)
        .map(Envelope::ok)
        .unwrap_or_else(|_| err("CONFLICT", "主机名称已存在或保存失败"))
}

#[tauri::command]
fn host_delete(state: State<'_, AppState>, request: Value) -> Envelope<bool> {
    let Some(id) = val_str(&request, "id") else {
        return err("VALIDATION", "缺少主机 id");
    };
    let conn = state.db.lock().unwrap();
    if db::has_active_session(&conn, &id).unwrap_or(false) {
        return err("HOST_IN_USE", "主机存在活动会话，请先断开");
    }
    db::delete(&conn, &id)
        .map(Envelope::ok)
        .unwrap_or_else(|_| err("INTERNAL", "删除主机失败"))
}

#[tauri::command]
fn credential_store(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(host_id) = val_str(&request, "host_id") else {
        return err("VALIDATION", "缺少 host_id");
    };
    let Some(kind) = val_str(&request, "kind") else {
        return err("VALIDATION", "缺少凭据类型");
    };
    if !matches!(kind.as_str(), "password" | "private_key" | "ssh_agent") {
        return err("VALIDATION", "不支持的凭据类型");
    }
    let retention = val_str(&request, "retention_mode").unwrap_or_else(|| "app_session".into());
    if !matches!(retention.as_str(), "never" | "app_session") {
        return err("VALIDATION", "不支持的保存模式");
    }
    let target =
        val_str(&request, "target_name").unwrap_or_else(|| format!("termpilot-{host_id}-{kind}"));
    if target.len() > 256 {
        return err("VALIDATION", "凭据目标名称过长");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let location = if kind == "private_key" {
        "user_file"
    } else if kind == "ssh_agent" {
        "ssh_agent"
    } else {
        "windows_credential_manager"
    };
    let conn = state.db.lock().unwrap();
    let r=conn.execute("INSERT INTO credential_refs(id,host_id,kind,target_name,secret_location,retention_mode,created_at) VALUES(?,?,?,?,?,?,?)",rusqlite::params![id,host_id,kind,target,location,retention,Utc::now().to_rfc3339()]);
    if r.is_err() {
        return err("CONFLICT", "凭据引用保存失败");
    };
    Envelope::ok(json!({"credential_ref":id,"kind":kind,"retention_mode":retention}))
}

#[tauri::command]
fn session_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Value,
) -> Envelope<Session> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(host_id) = val_str(&request, "host_id") else {
        return err("VALIDATION", "缺少 host_id");
    };
    let rows = request
        .pointer("/pty/rows")
        .and_then(Value::as_i64)
        .unwrap_or(30);
    let cols = request
        .pointer("/pty/cols")
        .and_then(Value::as_i64)
        .unwrap_or(120);
    if !(1..=1000).contains(&rows) || !(1..=1000).contains(&cols) {
        return err("VALIDATION", "PTY rows/cols 必须在 1-1000 范围内");
    };
    let conn = state.db.lock().unwrap();
    if !db::host_exists(&conn, &host_id).unwrap_or(false) {
        return err("NOT_FOUND", "主机不存在");
    };
    let endpoint: Result<(String, u16, String, Option<String>), _> = conn.query_row(
        "SELECT address,port,username,endpoint_fingerprint FROM hosts WHERE id=? AND deleted_at IS NULL",
        [&host_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    );
    let Ok((address, port, username, stored_fingerprint)) = endpoint else {
        return err("NOT_FOUND", "主机不存在");
    };
    if let Some(stored) = stored_fingerprint {
        let confirmed = request
            .get("fingerprint_confirmation")
            .and_then(Value::as_str)
            .map(|v| v == stored)
            .unwrap_or(false)
            || request
                .get("fingerprint_confirmation")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        if !confirmed {
            return err("SSH_HOSTKEY_CHANGED", "主机指纹未确认或已发生变化");
        }
    }
    if db::append_audit(
        &conn,
        "session.connect",
        "info",
        "user",
        Some(&host_id),
        None,
        &json!({"status":"authorized","pty_rows":rows,"pty_cols":cols}),
    )
    .is_err()
    {
        return err("AUDIT_UNAVAILABLE", "审计不可用，已阻止连接");
    }
    if state.ssh.connect(&address, port, &username).is_err() {
        return err("SSH_TIMEOUT", "SSH 连接失败");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    if conn.execute("INSERT INTO sessions(id,host_id,status,pty_rows,pty_cols,started_at) VALUES(?,?,?,?,?,?)",rusqlite::params![id,host_id,"ready",rows,cols,now]).is_err(){return err("INTERNAL","创建会话失败")};
    let session = Session {
        id,
        host_id,
        status: "ready".into(),
        started_at: now.clone(),
    };
    let _ = app.emit("session.status", json!({"event":"session.status","version":1,"seq":1,"session_id":session.id,"occurred_at":now,"data":{"status":"ready"}}));
    Envelope::ok(session)
}

#[tauri::command]
fn session_send_input(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    let Some(bytes) = val_str(&request, "bytes_base64") else {
        return err("VALIDATION", "缺少 bytes_base64");
    };
    if bytes.len() > 2_000_000 {
        return err("VALIDATION", "单次终端输入过大");
    };
    if !db::session_exists(&state.db.lock().unwrap(), &id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    };
    let accepted = state.ssh.send_input(&id, bytes.as_bytes()).unwrap_or(0);
    Envelope::ok(json!({"session_id":id,"accepted_bytes":accepted}))
}
#[tauri::command]
fn session_resize(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    let rows = request.get("rows").and_then(Value::as_i64).unwrap_or(0);
    let cols = request.get("cols").and_then(Value::as_i64).unwrap_or(0);
    if !(1..=1000).contains(&rows) || !(1..=1000).contains(&cols) {
        return err("VALIDATION", "rows/cols 必须在 1-1000 范围内");
    };
    if state.ssh.resize(&id, rows as u16, cols as u16).is_err() {
        return err("SESSION_CLOSED", "会话 resize 失败");
    }
    db::resize_session(&state.db.lock().unwrap(), &id, rows, cols)
        .map(|_| Envelope::ok(json!({"status":"ready"})))
        .unwrap_or_else(|_| err("SESSION_CLOSED", "会话不存在或已关闭"))
}
#[tauri::command]
fn session_disconnect(state: State<'_, AppState>, request: Value) -> Envelope<bool> {
    let Some(id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    state.ssh.close(&id);
    db::disconnect_session(
        &state.db.lock().unwrap(),
        &id,
        val_str(&request, "reason").as_deref(),
    )
    .map(Envelope::ok)
    .unwrap_or_else(|_| err("INTERNAL", "断开会话失败"))
}
#[tauri::command]
fn session_cancel(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    state.ssh.close(&id);
    match db::disconnect_session(&state.db.lock().unwrap(), &id, Some("cancelled")) {
        Ok(true) => Envelope::ok(json!({"session_id":id,"status":"cancelled"})),
        Ok(false) => err("SESSION_CLOSED", "会话不存在或已关闭"),
        Err(_) => err("INTERNAL", "取消会话失败"),
    }
}

#[tauri::command]
fn sftp_list(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(path) = val_str(&request, "path") else {
        return err("VALIDATION", "缺少 path");
    };
    if !valid_path(&path) {
        return err("PATH_ESCAPE", "远端路径越界");
    };
    let limit = request.get("limit").and_then(Value::as_u64).unwrap_or(200);
    if !(1..=1000).contains(&limit) {
        return err("VALIDATION", "limit 必须在 1-1000 范围内");
    };
    let Some(session_id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    if !db::session_exists(&state.db.lock().unwrap(), &session_id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    let entries = state.sftp.list(&session_id, &path).unwrap_or_default();
    Envelope::ok(
        json!({"path":path,"entries":entries.into_iter().take(limit as usize).collect::<Vec<_>>(),"next_cursor":Value::Null}),
    )
}
#[tauri::command]
fn sftp_transfer_start(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(session) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    let Some(op) = val_str(&request, "op") else {
        return err("VALIDATION", "缺少 op");
    };
    if !matches!(
        op.as_str(),
        "upload" | "download" | "delete" | "rename" | "mkdir"
    ) {
        return err("VALIDATION", "不支持的 SFTP 操作");
    };
    for key in ["src", "dst"] {
        if let Some(p) = val_str(&request, key) {
            if !valid_path(&p) {
                return err("PATH_ESCAPE", "路径越界");
            }
        }
    }
    let conn = state.db.lock().unwrap();
    if !db::session_exists(&conn, &session).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    let production = db::session_host_is_production(&conn, &session).unwrap_or(false);
    let confirmed = request
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || request.get("approval_id").and_then(Value::as_str).is_some();
    if production && matches!(op.as_str(), "upload" | "delete" | "rename") && !confirmed {
        return err("APPROVAL_REQUIRED", "生产主机的写操作需要人工确认");
    }
    let id = uuid::Uuid::new_v4().to_string();
    if db::insert_sftp(
        &conn,
        &id,
        &session,
        &op,
        val_str(&request, "src").as_deref(),
        val_str(&request, "dst").as_deref(),
    )
    .is_err()
    {
        return err("NOT_FOUND", "会话不存在");
    };
    Envelope::ok(json!({"transfer_id":id,"status":"queued"}))
}
fn transfer_status(
    state: State<'_, AppState>,
    request: Value,
    status: &'static str,
) -> Envelope<Value> {
    let Some(id) = val_str(&request, "transfer_id") else {
        return err("VALIDATION", "缺少 transfer_id");
    };
    db::update_sftp_status(&state.db.lock().unwrap(), &id, status)
        .map(|_| Envelope::ok(json!({"transfer_id":id,"status":status})))
        .unwrap_or_else(|_| err("NOT_FOUND", "传输任务不存在"))
}
#[tauri::command]
fn transfer_pause(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    transfer_status(state, request, "paused")
}
#[tauri::command]
fn transfer_resume(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    transfer_status(state, request, "running")
}
#[tauri::command]
fn transfer_cancel(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(id) = val_str(&request, "transfer_id") {
        state.sftp.cancel(&id);
    }
    transfer_status(state, request, "cancelled")
}

#[tauri::command]
fn policy_get(state: State<'_, AppState>) -> Envelope<Value> {
    let conn = state.db.lock().unwrap();
    let mut s=match conn.prepare("SELECT id,mode,version,allow_rules_json,limits_json FROM security_policies WHERE is_active=1 LIMIT 1"){Ok(x)=>x,Err(_)=>return err("INTERNAL","读取策略失败")};
    let row=s.query_row([],|r|Ok(json!({"policy_id":r.get::<_,String>(0)?,"mode":r.get::<_,String>(1)?,"version":r.get::<_,i64>(2)?,"allow_rules":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"limits":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(json!({})),"fixed_readonly":[["df","-h"],["pwd"],["whoami"]]})));
    row.map(Envelope::ok)
        .unwrap_or_else(|_| err("NOT_FOUND", "没有活动策略"))
}
#[tauri::command]
fn policy_allow_rule_upsert(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(id) = val_str(&request, "policy_id") else {
        return err("VALIDATION", "缺少 policy_id");
    };
    let Some(rule) = request.get("rule") else {
        return err("VALIDATION", "缺少 rule");
    };
    if !rule.is_object()
        || rule.get("program").and_then(Value::as_str).is_none()
        || rule.get("args").and_then(Value::as_array).is_none()
        || rule.get("host_id").and_then(Value::as_str).is_none()
        || rule.get("remote_user").and_then(Value::as_str).is_none()
        || rule.get("cwd").and_then(Value::as_str).is_none()
        || rule
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .is_none()
        || rule.get("output_limit").and_then(Value::as_u64).is_none()
    {
        return err(
            "VALIDATION",
            "规则必须绑定 program、args、host_id、remote_user、cwd、超时和输出上限",
        );
    };
    let timeout = rule
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_limit = rule
        .get("output_limit")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if !(5..=600).contains(&timeout) || output_limit == 0 || output_limit > 64 * 1024 * 1024 {
        return err(
            "VALIDATION",
            "超时必须在 5-600 秒，输出上限必须在 1-67108864 字节",
        );
    }
    let conn = state.db.lock().unwrap();
    let old: Result<(i64, String), _> = conn.query_row(
        "SELECT version,allow_rules_json FROM security_policies WHERE id=? AND is_active=1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    let Ok((version, raw)) = old else {
        return err("NOT_FOUND", "策略不存在");
    };
    let mut rules = serde_json::from_str::<Vec<Value>>(&raw).unwrap_or_default();
    rules.push(rule.clone());
    if conn
        .execute(
            "UPDATE security_policies SET allow_rules_json=?,version=?,updated_at=? WHERE id=?",
            rusqlite::params![
                serde_json::to_string(&rules).unwrap_or_else(|_| "[]".into()),
                version + 1,
                Utc::now().to_rfc3339(),
                id
            ],
        )
        .is_err()
    {
        return err("INTERNAL", "更新策略失败");
    };
    Envelope::ok(json!({"policy_id":id,"version":version+1,"rule":rule}))
}

#[tauri::command]
fn get_terminal_context(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(session_id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    if !db::session_exists(&state.db.lock().unwrap(), &session_id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    Envelope::ok(
        json!({"session_id":session_id,"cwd":"~","user":"ops","shell":"posix","redacted":true,"output":"终端上下文已脱敏"}),
    )
}
#[tauri::command]
fn run_read_only_command(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    }
    let Some(session_id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    let Some(argv) = request.get("argv").and_then(Value::as_array) else {
        return err("VALIDATION", "缺少结构化 argv");
    };
    let args: Vec<String> = argv
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if args.len() != argv.len() || !policy::is_fixed_readonly(&args) {
        return err("POLICY_BLOCKED", "命令不在固定只读白名单");
    }
    if !db::session_exists(&state.db.lock().unwrap(), &session_id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    let output = match args.as_slice() {
        [p, a] if p == "df" && a == "-h" => "/dev/sda2 80G 52G 25G 68% /var",
        [p] if p == "pwd" => "/home/ops",
        [p] if p == "whoami" => "ops",
        _ => "",
    };
    Envelope::ok(
        json!({"session_id":session_id,"argv":args,"status":"completed","stdout":output,"risk":"low"}),
    )
}
#[tauri::command]
fn propose_command(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(session_id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    let Some(argv) = request.get("argv").and_then(Value::as_array) else {
        return err("VALIDATION", "缺少结构化 argv");
    };
    if argv.is_empty() || argv.iter().any(|v| v.as_str().is_none()) {
        return err("VALIDATION", "argv 必须为字符串数组");
    }
    let args: Vec<String> = argv
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let approval_id = uuid::Uuid::new_v4().to_string();
    let policy_id = "default";
    let now = Utc::now();
    let expires = (now + chrono::Duration::minutes(5)).to_rfc3339();
    let conn = state.db.lock().unwrap();
    if !db::session_exists(&conn, &session_id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    let inserted = conn.execute("INSERT INTO command_approvals(id,session_id,policy_id,argv_json,cwd,command_hash,risk,policy_version,status,created_at,expires_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)", rusqlite::params![approval_id,session_id,policy_id,serde_json::to_string(&args).unwrap_or_else(|_|"[]".into()),"~",hex::encode(sha2::Sha256::digest(args.join("\0").as_bytes())),if policy::is_fixed_readonly(&args){"low"}else{"medium"},1,"pending",now.to_rfc3339(),expires]).is_ok();
    if !inserted {
        return err("INTERNAL", "创建审批失败");
    }
    Envelope::ok(
        json!({"approval_id":approval_id,"status":"pending","expires_at":expires,"argv":args}),
    )
}
#[tauri::command]
fn execute_approved_command(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    }
    let Some(id) = val_str(&request, "approval_id") else {
        return err("VALIDATION", "缺少 approval_id");
    };
    let conn = state.db.lock().unwrap();
    let row: Result<(String,String),_> = conn.query_row("SELECT session_id,argv_json FROM command_approvals WHERE id=? AND status='approved' AND expires_at>?", rusqlite::params![id,Utc::now().to_rfc3339()], |r| Ok((r.get(0)?,r.get(1)?)));
    let Ok((session_id, argv_json)) = row else {
        return err("APPROVAL_EXPIRED", "审批不存在、未批准或已过期");
    };
    if conn.execute("UPDATE command_approvals SET status='consumed',decided_at=? WHERE id=? AND status='approved'",rusqlite::params![Utc::now().to_rfc3339(),id]).unwrap_or(0)!=1{return err("APPROVAL_EXPIRED","审批票据不可重放")};
    Envelope::ok(
        json!({"approval_id":id,"session_id":session_id,"argv":serde_json::from_str::<Value>(&argv_json).unwrap_or(json!([])),"status":"completed"}),
    )
}
#[tauri::command]
fn agent_message_send(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(text) = val_str(&request, "text").map(|s| policy::sanitize_text(&s)) else {
        return err("VALIDATION", "消息不能为空");
    };
    if text.trim().is_empty() {
        return err("VALIDATION", "消息不能为空");
    };
    let mode = val_str(&request, "mode").unwrap_or_else(|| "ask_before_execute".into());
    if !matches!(
        mode.as_str(),
        "readonly" | "ask_before_execute" | "allow_safe_commands" | "manual_only"
    ) {
        return err("VALIDATION", "不支持的 Agent 模式");
    }
    let Some(session_id) = val_str(&request, "session_id") else {
        return err("VALIDATION", "缺少 session_id");
    };
    if !db::session_exists(&state.db.lock().unwrap(), &session_id).unwrap_or(false) {
        return err("SESSION_CLOSED", "会话不存在或已关闭");
    }
    let task_id = uuid::Uuid::new_v4().to_string();
    let response = state
        .model
        .complete("你是受策略约束的远程运维助手。", &text)
        .unwrap_or_else(|_| "模型暂不可用，请使用终端手动操作。".into());
    Envelope::ok(
        json!({"status":"queued","message":text,"response":response,"mode":mode,"session_id":session_id,"task_id":task_id}),
    )
}
#[tauri::command]
fn agent_cancel(_state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    let Some(id) = val_str(&request, "task_id") else {
        return err("VALIDATION", "缺少 task_id");
    };
    Envelope::ok(json!({"task_id":id,"status":"cancelled"}))
}
#[tauri::command]
fn approval_decide(state: State<'_, AppState>, request: Value) -> Envelope<Value> {
    if let Some(x) = reject_if_stopped(&state) {
        return x;
    };
    let Some(id) = val_str(&request, "approval_id") else {
        return err("VALIDATION", "缺少 approval_id");
    };
    let Some(decision) = val_str(&request, "decision") else {
        return err("VALIDATION", "缺少 decision");
    };
    if !matches!(decision.as_str(), "approve" | "reject") {
        return err("VALIDATION", "decision 必须为 approve 或 reject");
    };
    let status = if decision == "approve" {
        "approved"
    } else {
        "rejected"
    };
    let conn = state.db.lock().unwrap();
    let n=conn.execute("UPDATE command_approvals SET status=?,decided_at=? WHERE id=? AND status='pending' AND expires_at>?",rusqlite::params![status,Utc::now().to_rfc3339(),id,Utc::now().to_rfc3339()]).unwrap_or(0);
    if n == 0 {
        return err("APPROVAL_EXPIRED", "审批不存在、已处理或已过期");
    };
    Envelope::ok(json!({"approval_id":id,"status":status}))
}

#[tauri::command]
fn audit_export(state: State<'_, AppState>, _request: Value) -> Envelope<Value> {
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = dir.join("TermPilot").join("exports");
    if fs::create_dir_all(&dir).is_err() {
        return err("AUDIT_UNAVAILABLE", "无法创建导出目录");
    };
    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("audit-{id}.jsonl"));
    let conn = state.db.lock().unwrap();
    let mut stmt=match conn.prepare("SELECT event_id,event_type,severity,actor,target_host_id,session_id,correlation_id,payload_json,prev_hash,hash,created_at FROM audit_logs ORDER BY id"){Ok(x)=>x,Err(_)=>return err("AUDIT_UNAVAILABLE","读取审计失败")};
    let mut out = String::new();
    let rows=stmt.query_map([],|r|Ok(json!({"event_id":r.get::<_,String>(0)?,"event_type":r.get::<_,String>(1)?,"severity":r.get::<_,String>(2)?,"actor":r.get::<_,String>(3)?,"target_host_id":r.get::<_,Option<String>>(4)?,"session_id":r.get::<_,Option<String>>(5)?,"correlation_id":r.get::<_,Option<String>>(6)?,"payload":serde_json::from_str::<Value>(&r.get::<_,String>(7)?).unwrap_or(json!({})),"prev_hash":r.get::<_,Option<String>>(8)?,"hash":r.get::<_,String>(9)?,"created_at":r.get::<_,String>(10)?})));
    let mut count = 0;
    if let Ok(rows) = rows {
        for row in rows {
            if let Ok(v) = row {
                out.push_str(&serde_json::to_string(&v).unwrap_or_default());
                out.push('\n');
                count += 1;
            }
        }
    }
    if fs::write(&path, &out).is_err() {
        return err("AUDIT_UNAVAILABLE", "写入审计导出失败");
    };
    let hash = hex::encode(sha2::Sha256::digest(out.as_bytes()));
    let manifest =
        json!({"format":"jsonl","event_count":count,"file_hash":hash,"genesis":audit::GENESIS});
    let manifest_path = path.with_extension("manifest.json");
    if fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    )
    .is_err()
    {
        return err("AUDIT_UNAVAILABLE", "写入 manifest 失败");
    };
    Envelope::ok(
        json!({"export_id":id,"path":path,"manifest_path":manifest_path,"event_count":count,"file_hash":hash}),
    )
}
#[tauri::command]
fn audit_export_verify(request: Value) -> Envelope<Value> {
    let Some(path) = val_str(&request, "path") else {
        return err("VALIDATION", "缺少 path");
    };
    let Ok(bytes) = fs::read(&path) else {
        return err("NOT_FOUND", "导出文件不存在");
    };
    let hash = hex::encode(sha2::Sha256::digest(&bytes));
    let jsonl_ok = String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| serde_json::from_str::<Value>(line).is_ok());
    let mut previous = audit::GENESIS.to_owned();
    let mut chain_ok = jsonl_ok;
    for line in String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            chain_ok = false;
            break;
        };
        if event.get("prev_hash").and_then(Value::as_str).unwrap_or("") != previous {
            chain_ok = false;
            break;
        }
        previous = event
            .get("hash")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    let manifest_path = PathBuf::from(&path).with_extension("manifest.json");
    let expected = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| {
            v.get("file_hash")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let valid = chain_ok && expected.as_deref().map(|v| v == hash).unwrap_or(false);
    Envelope::ok(
        json!({"valid":valid,"file_hash":hash,"bytes":bytes.len(),"manifest_path":manifest_path}),
    )
}

#[tauri::command]
fn emergency_stop(app: AppHandle, state: State<'_, AppState>, _request: Value) -> Envelope<bool> {
    state.emergency_stop.store(true, Ordering::SeqCst);
    let _ = db::append_audit(
        &state.db.lock().unwrap(),
        "system.emergency_stop",
        "critical",
        "user",
        None,
        None,
        &json!({"scope":"all"}),
    );
    let _ = app.emit("system.emergency_stop", json!({"event":"system.emergency_stop","version":1,"seq":1,"occurred_at":Utc::now().to_rfc3339(),"data":{"scope":"all"}}));
    Envelope::ok(true)
}
#[tauri::command]
fn emergency_stop_clear(state: State<'_, AppState>, request: Value) -> Envelope<bool> {
    if !request
        .get("confirmed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return err("VALIDATION", "解除急停需要当前 Windows 用户确认");
    }
    state.emergency_stop.store(false, Ordering::SeqCst);
    Envelope::ok(true)
}

fn main() {
    let conn = db::open().expect("database initialization failed");
    tauri::Builder::default()
        .manage(AppState {
            db: Mutex::new(conn),
            emergency_stop: AtomicBool::new(false),
            _config: config::load(),
            ssh: transport::MockSshTransport,
            sftp: transport::MockSftpTransport,
            model: model_client::MockModelClient,
        })
        .invoke_handler(tauri::generate_handler![
            host_list,
            host_upsert,
            host_delete,
            credential_store,
            session_connect,
            session_send_input,
            session_resize,
            session_disconnect,
            session_cancel,
            sftp_list,
            sftp_transfer_start,
            transfer_pause,
            transfer_resume,
            transfer_cancel,
            policy_get,
            policy_allow_rule_upsert,
            get_terminal_context,
            run_read_only_command,
            propose_command,
            execute_approved_command,
            agent_message_send,
            agent_cancel,
            approval_decide,
            audit_export,
            audit_export_verify,
            emergency_stop,
            emergency_stop_clear
        ])
        .run(tauri::generate_context!())
        .expect("error while running TermPilot");
}
