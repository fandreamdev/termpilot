use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct AppConfig {
    pub model: Option<ModelConfig>,
}
#[derive(Debug, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub endpoint_scope: Option<String>,
    pub timeout_seconds: Option<u64>,
}
pub fn load() -> AppConfig {
    let path = dirs_path().join(".termpilot").join("config.toml");
    fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<AppConfig>(&s).ok())
        .filter(|config| {
            config
                .model
                .as_ref()
                .map(valid_model_config)
                .unwrap_or(true)
        })
        .unwrap_or_default()
}
fn valid_model_config(config: &ModelConfig) -> bool {
    config.provider == "ollama"
        && config
            .endpoint
            .as_deref()
            .map(|v| v.starts_with("http://") || v.starts_with("https://"))
            .unwrap_or(false)
        && matches!(
            config.endpoint_scope.as_deref().unwrap_or("local"),
            "local" | "public" | "custom"
        )
        && config.timeout_seconds.unwrap_or(120) >= 5
        && config.timeout_seconds.unwrap_or(120) <= 600
}
fn dirs_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
