//! Single-provider model boundary. The provider is selected from the user
//! TOML file; no model receives credentials or raw terminal/file contents.
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
