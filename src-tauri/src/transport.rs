//! Transport boundary. Concrete SSH/SFTP implementations must live behind
//! these traits so command handlers can validate policy before opening a
//! channel. The first milestone uses the mock implementation in tests.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("connection timed out")]
    Timeout,
    #[error("authentication failed")]
    Authentication,
    #[error("transport unavailable: {0}")]
    Unavailable(String),
}
pub trait SshTransport: Send + Sync {
    fn connect(&self, host: &str, port: u16, user: &str) -> Result<String, TransportError>;
    fn send_input(&self, session_id: &str, bytes: &[u8]) -> Result<usize, TransportError>;
    fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), TransportError>;
    fn close(&self, session_id: &str);
}
pub trait SftpTransport: Send + Sync {
    fn list(&self, session_id: &str, path: &str) -> Result<Vec<String>, TransportError>;
    fn cancel(&self, transfer_id: &str);
}

#[derive(Default)]
pub struct MockSshTransport;
impl SshTransport for MockSshTransport {
    fn connect(&self, host: &str, port: u16, user: &str) -> Result<String, TransportError> {
        if host.is_empty() || port == 0 || user.is_empty() {
            return Err(TransportError::Unavailable(
                "invalid connection parameters".into(),
            ));
        }
        Ok(format!("mock-ssh-{user}@{host}:{port}"))
    }
    fn send_input(&self, _session_id: &str, bytes: &[u8]) -> Result<usize, TransportError> {
        Ok(bytes.len())
    }
    fn resize(&self, _session_id: &str, _rows: u16, _cols: u16) -> Result<(), TransportError> {
        Ok(())
    }
    fn close(&self, _session_id: &str) {}
}

#[derive(Default)]
pub struct MockSftpTransport;
impl SftpTransport for MockSftpTransport {
    fn list(&self, _session_id: &str, path: &str) -> Result<Vec<String>, TransportError> {
        if path.is_empty() || path.contains('\0') || path.split(['/', '\\']).any(|p| p == "..") {
            return Err(TransportError::Unavailable("path escape".into()));
        }
        Ok(vec![
            "releases/".into(),
            "app.conf".into(),
            "release.tar.gz".into(),
            "README.md".into(),
        ])
    }
    fn cancel(&self, _transfer_id: &str) {}
}
