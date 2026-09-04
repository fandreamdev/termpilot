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
    let Some(endpoint) = config.endpoint.as_deref() else {
        return false;
    };
    let scope = config.endpoint_scope.as_deref().unwrap_or("local");
    config.provider == "ollama"
        && (endpoint.starts_with("http://") || endpoint.starts_with("https://"))
        && matches!(scope, "local" | "public" | "custom")
        && (scope != "local" || is_local_endpoint(endpoint))
        && (scope == "local" || endpoint.starts_with("https://"))
        && config.timeout_seconds.unwrap_or(120) >= 5
        && config.timeout_seconds.unwrap_or(120) <= 600
}
fn is_local_endpoint(endpoint: &str) -> bool {
    let authority = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_matches(['[', ']']);
    matches!(authority, "localhost" | "127.0.0.1" | "::1")
}
fn dirs_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(scope: &str, endpoint: &str) -> ModelConfig {
        ModelConfig {
            provider: "ollama".into(),
            endpoint: Some(endpoint.into()),
            model: Some("qwen2.5:7b".into()),
            endpoint_scope: Some(scope.into()),
            timeout_seconds: Some(30),
        }
    }

    #[test]
    fn local_scope_cannot_point_to_public_endpoint() {
        assert!(valid_model_config(&model(
            "local",
            "http://127.0.0.1:11434"
        )));
        assert!(!valid_model_config(&model(
            "local",
            "https://models.example.test"
        )));
        assert!(valid_model_config(&model(
            "public",
            "https://models.example.test"
        )));
    }
}
