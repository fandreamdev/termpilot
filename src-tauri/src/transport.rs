//! Transport boundary. Concrete SSH/SFTP implementations must live behind
//! these traits so command handlers can validate policy before opening a
//! channel. The first milestone uses the mock implementation in tests.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError { #[error("connection timed out")] Timeout, #[error("authentication failed")] Authentication, #[error("transport unavailable: {0}")] Unavailable(String) }
pub trait SshTransport: Send + Sync { fn connect(&self, host: &str, port: u16, user: &str) -> Result<String, TransportError>; fn close(&self, session_id: &str); }
pub trait SftpTransport: Send + Sync { fn list(&self, session_id: &str, path: &str) -> Result<Vec<String>, TransportError>; fn cancel(&self, transfer_id: &str); }
