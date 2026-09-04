#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)] // Adapter and audit APIs are intentionally staged for the next milestones.
mod audit; mod config; mod db; mod model_client; mod models; mod policy; mod transport;
use std::sync::{Mutex, atomic::{AtomicBool, Ordering}};
use chrono::Utc;
use tauri::State;
use models::{Envelope, Host, HostUpsert, Session};

struct AppState { db: Mutex<rusqlite::Connection>, emergency_stop: AtomicBool, _config: config::AppConfig }

#[tauri::command]
fn host_list(state: State<'_, AppState>) -> Envelope<Vec<Host>> { let conn = state.db.lock().unwrap(); db::hosts(&conn).map(Envelope::ok).unwrap_or_else(|e| Envelope::err("INTERNAL", &e.to_string())) }
#[tauri::command]
fn host_upsert(state: State<'_, AppState>, request: HostUpsert) -> Envelope<String> { if let Err((c,m)) = policy::validate_host(&request.address,request.port,&request.username) { return Envelope::err(c,m); } db::upsert(&state.db.lock().unwrap(), &request).map(Envelope::ok).unwrap_or_else(|e| Envelope::err("CONFLICT",&e.to_string())) }
#[tauri::command]
fn host_delete(state: State<'_, AppState>, request: serde_json::Value) -> Envelope<bool> { let Some(id)=request.get("id").and_then(|v|v.as_str()) else { return Envelope::err("VALIDATION","缺少主机 id") }; db::delete(&state.db.lock().unwrap(),id).map(Envelope::ok).unwrap_or_else(|e|Envelope::err("INTERNAL",&e.to_string())) }
#[tauri::command]
fn session_connect(state: State<'_, AppState>, request: serde_json::Value) -> Envelope<Session> { if state.emergency_stop.load(Ordering::SeqCst) { return Envelope::err("EMERGENCY_STOP_ACTIVE","急停状态已启用") } let Some(host_id)=request.get("host_id").and_then(|v|v.as_str()) else { return Envelope::err("VALIDATION","缺少 host_id") }; let id=uuid::Uuid::new_v4().to_string(); let now=Utc::now().to_rfc3339(); let r = state.db.lock().unwrap().execute("INSERT INTO sessions(id,host_id,status,pty_rows,pty_cols,started_at) VALUES(?,?,?, ?,?,?)",rusqlite::params![id,host_id,"ready",30,120,now]); if let Err(e)=r { return Envelope::err("NOT_FOUND",&e.to_string()) } Envelope::ok(Session{id,host_id:host_id.into(),status:"ready".into(),started_at:now}) }
#[tauri::command]
fn emergency_stop(state: State<'_, AppState>, _request: serde_json::Value) -> Envelope<bool> { state.emergency_stop.store(true,Ordering::SeqCst); Envelope::ok(true) }
#[tauri::command]
fn emergency_stop_clear(state: State<'_, AppState>) -> Envelope<bool> { state.emergency_stop.store(false,Ordering::SeqCst); Envelope::ok(true) }
#[tauri::command]
fn policy_get() -> Envelope<serde_json::Value> { Envelope::ok(serde_json::json!({"mode":"ask_before_execute","version":1,"fixed_readonly":[["df","-h"],["pwd"],["whoami"]]})) }
#[tauri::command]
fn agent_message_send(state: State<'_, AppState>, request: serde_json::Value) -> Envelope<serde_json::Value> { if state.emergency_stop.load(Ordering::SeqCst) { return Envelope::err("EMERGENCY_STOP_ACTIVE","急停状态已启用") } let text=request.get("text").and_then(|v|v.as_str()).map(policy::sanitize_text).unwrap_or_default(); if text.is_empty() { return Envelope::err("VALIDATION","消息不能为空") } Envelope::ok(serde_json::json!({"status":"queued","message":text})) }

fn main() { let conn=db::open().expect("database initialization failed"); tauri::Builder::default().manage(AppState{db:Mutex::new(conn),emergency_stop:AtomicBool::new(false),_config:config::load()}).invoke_handler(tauri::generate_handler![host_list,host_upsert,host_delete,session_connect,emergency_stop,emergency_stop_clear,policy_get,agent_message_send]).run(tauri::generate_context!()).expect("error while running TermPilot"); }
