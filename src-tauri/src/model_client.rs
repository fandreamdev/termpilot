//! Single-provider model boundary. The provider is selected from the user
//! TOML file; no model receives credentials or raw terminal/file contents.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError { #[error("model unavailable")] Unavailable, #[error("model request cancelled")] Cancelled }
pub trait ModelClient: Send + Sync { fn complete(&self, system: &str, user: &str) -> Result<String, ModelError>; fn cancel(&self, task_id: &str); }
