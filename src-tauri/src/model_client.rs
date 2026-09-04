//! Single-provider model boundary. The provider is selected from the user
//! TOML file; no model receives credentials or raw terminal/file contents.
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model unavailable")]
    Unavailable,
    #[error("model request cancelled")]
    Cancelled,
}
pub trait ModelClient: Send + Sync {
    fn complete(&self, system: &str, user: &str) -> Result<String, ModelError>;
    fn cancel(&self, task_id: &str);
}

pub struct OllamaModelClient {
    endpoint: String,
    model: String,
    timeout: Duration,
}

impl OllamaModelClient {
    pub fn new(endpoint: String, model: String, timeout_seconds: u64) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            model,
            timeout: Duration::from_secs(timeout_seconds.clamp(5, 600)),
        }
    }
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}
#[derive(Deserialize)]
struct OllamaResponse {
    response: Option<String>,
}

impl ModelClient for OllamaModelClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, ModelError> {
        let prompt = format!("System: {system}\nUser: {user}");
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let response: OllamaResponse = agent
            .post(&format!("{}/api/generate", self.endpoint))
            .send_json(ureq::json!(OllamaRequest {
                model: &self.model,
                prompt,
                stream: false
            }))
            .map_err(|_| ModelError::Unavailable)?
            .into_json()
            .map_err(|_| ModelError::Unavailable)?;
        response
            .response
            .filter(|v| !v.trim().is_empty())
            .ok_or(ModelError::Unavailable)
    }
    fn cancel(&self, _task_id: &str) {}
}

#[derive(Default)]
pub struct MockModelClient;
impl ModelClient for MockModelClient {
    fn complete(&self, _system: &str, user: &str) -> Result<String, ModelError> {
        if user.trim().is_empty() {
            return Err(ModelError::Unavailable);
        }
        Ok("我会先读取脱敏终端上下文，并仅建议固定只读命令。".into())
    }
    fn cancel(&self, _task_id: &str) {}
}

pub fn from_config(config: &crate::config::AppConfig) -> Box<dyn ModelClient> {
    if let Some(model) = config.model.as_ref() {
        if model.provider == "ollama" {
            return Box::new(OllamaModelClient::new(
                model
                    .endpoint
                    .clone()
                    .unwrap_or_else(|| "http://127.0.0.1:11434".into()),
                model.model.clone().unwrap_or_else(|| "qwen2.5:7b".into()),
                model.timeout_seconds.unwrap_or(120),
            ));
        }
    }
    Box::new(MockModelClient)
}
