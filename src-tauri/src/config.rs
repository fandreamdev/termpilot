use std::{fs, path::PathBuf};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct AppConfig { pub model: Option<ModelConfig> }
#[derive(Debug, Deserialize)]
pub struct ModelConfig { pub provider: String, pub endpoint: Option<String>, pub model: Option<String>, pub endpoint_scope: Option<String>, pub timeout_seconds: Option<u64> }
pub fn load() -> AppConfig { let path = dirs_path().join(".termpilot").join("config.toml"); fs::read_to_string(path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default() }
fn dirs_path() -> PathBuf { std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")) }
