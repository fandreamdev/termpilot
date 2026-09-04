//! Transport boundary. Concrete SSH/SFTP implementations must live behind
//! these traits so command handlers can validate policy before opening a
//! channel. The first milestone uses the mock implementation in tests.
use base64::Engine;
use sha2::Digest;
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter};
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
    fn fingerprint(&self, _host: &str, _port: u16) -> Result<Option<String>, TransportError> {
        Ok(None)
    }
    fn connect_for_session(
        &self,
        _session_id: &str,
        host: &str,
        port: u16,
        user: &str,
    ) -> Result<String, TransportError> {
        self.connect(host, port, user)
    }
    fn start_output_pump(&self, _session_id: &str, _app: AppHandle) {}
    fn send_input(&self, session_id: &str, bytes: &[u8]) -> Result<usize, TransportError>;
    fn execute_structured(
        &self,
        _session_id: &str,
        _argv: &[String],
        _cwd: &str,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unavailable(
            "structured execution unavailable".into(),
        ))
    }
    fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), TransportError>;
    fn close(&self, session_id: &str);
    fn close_all(&self) {}
}
pub trait SftpTransport: Send + Sync {
    fn register_session(&self, _session_id: &str, _host: &str, _port: u16, _user: &str) {}
    fn unregister_session(&self, _session_id: &str) {}
    fn close_all(&self) {}
    fn realpath(&self, _session_id: &str, path: &str) -> Result<String, TransportError> {
        Ok(path.to_owned())
    }
    fn supports_safe_append(&self) -> bool {
        false
    }
    fn upload_from_path(
        &self,
        _session_id: &str,
        _local: &std::path::Path,
        _remote: &str,
        _overwrite: bool,
        _resume: bool,
    ) -> Result<(u64, String), TransportError> {
        Err(TransportError::Unavailable(
            "streaming upload unavailable".into(),
        ))
    }
    fn download_to_path(
        &self,
        _session_id: &str,
        _remote: &str,
        _local: &std::path::Path,
        _overwrite: bool,
        _resume: bool,
    ) -> Result<(u64, String), TransportError> {
        Err(TransportError::Unavailable(
            "streaming download unavailable".into(),
        ))
    }
    fn list(&self, session_id: &str, path: &str) -> Result<Vec<String>, TransportError>;
    fn read_file(
        &self,
        session_id: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, TransportError>;
    fn write_file(
        &self,
        session_id: &str,
        path: &str,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<(), TransportError>;
    fn delete(&self, session_id: &str, path: &str) -> Result<(), TransportError>;
    fn rename(
        &self,
        session_id: &str,
        source: &str,
        destination: &str,
        overwrite: bool,
    ) -> Result<(), TransportError>;
    fn mkdir(&self, session_id: &str, path: &str) -> Result<(), TransportError>;
    fn cancel(&self, transfer_id: &str);
}

/// Optional adapter backed by the OpenSSH binaries installed on Windows.
/// It is intentionally opt-in (`TERMPILOT_TRANSPORT=openssh`) so local tests
/// never contact an unintended server. Authentication is delegated to the
/// user's OpenSSH agent/key configuration; passwords are never passed on a
/// command line.
pub struct OpenSshTransport {
    children: Mutex<HashMap<String, Child>>,
}
impl Default for OpenSshTransport {
    fn default() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
        }
    }
}
impl SshTransport for OpenSshTransport {
    fn fingerprint(&self, host: &str, port: u16) -> Result<Option<String>, TransportError> {
        let output = Command::new("ssh-keyscan")
            .args(["-T", "10", "-p", &port.to_string(), host])
            .output()
            .map_err(|e| TransportError::Unavailable(format!("ssh-keyscan: {e}")))?;
        if !output.status.success() || output.stdout.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "SHA256:{}",
            hex::encode(sha2::Sha256::digest(&output.stdout))
        )))
    }
    fn connect(&self, host: &str, port: u16, user: &str) -> Result<String, TransportError> {
        let id = format!("{user}@{host}:{port}");
        self.connect_for_session(&id, host, port, user).map(|_| id)
    }
    fn connect_for_session(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        user: &str,
    ) -> Result<String, TransportError> {
        let child = Command::new("ssh")
            .args([
                "-tt",
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-p",
                &port.to_string(),
                &format!("{user}@{host}"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| TransportError::Unavailable(format!("ssh: {e}")))?;
        self.children
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .insert(session_id.to_owned(), child);
        Ok(session_id.to_owned())
    }
    fn send_input(&self, session_id: &str, bytes: &[u8]) -> Result<usize, TransportError> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        let child = children
            .get_mut(session_id)
            .ok_or(TransportError::Unavailable("session not found".into()))?;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(TransportError::Unavailable("stdin unavailable".into()))?;
        stdin
            .write_all(bytes)
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        stdin.flush().ok();
        Ok(bytes.len())
    }
    fn execute_structured(
        &self,
        session_id: &str,
        argv: &[String],
        cwd: &str,
    ) -> Result<(), TransportError> {
        if argv.is_empty() {
            return Err(TransportError::Unavailable("empty command".into()));
        }
        let command = format!(
            "cd {} && {}\n",
            shell_quote(cwd),
            argv.iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );
        self.send_input(session_id, command.as_bytes()).map(|_| ())
    }
    fn start_output_pump(&self, session_id: &str, app: AppHandle) {
        let Ok(mut children) = self.children.lock() else {
            return;
        };
        let Some(child) = children.get_mut(session_id) else {
            return;
        };
        let Some(mut stdout) = child.stdout.take() else {
            return;
        };
        let id = session_id.to_owned();
        let sequence = AtomicU64::new(1);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) | Err(_) => {
                        let seq = sequence.fetch_add(1, Ordering::SeqCst) + 1;
                        let _ = app.emit("session.status", serde_json::json!({"event":"session.status","version":1,"seq":seq,"session_id":id,"correlation_id":id,"occurred_at":chrono::Utc::now().to_rfc3339(),"data":{"status":"disconnected"}}));
                        break;
                    }
                    Ok(n) => {
                        let seq = sequence.fetch_add(1, Ordering::SeqCst) + 1;
                        let _ = app.emit("session.output", serde_json::json!({"event":"session.output","version":1,"seq":seq,"session_id":id,"correlation_id":id,"occurred_at":chrono::Utc::now().to_rfc3339(),"data":{"bytes_base64":base64::engine::general_purpose::STANDARD.encode(&buffer[..n])}}));
                    }
                }
            }
        });
    }
    fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), TransportError> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        let child = children
            .get_mut(session_id)
            .ok_or(TransportError::Unavailable("session not found".into()))?;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(TransportError::Unavailable("stdin unavailable".into()))?;
        writeln!(stdin, "stty rows {rows} cols {cols}")
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        stdin.flush().ok();
        Ok(())
    }
    fn close(&self, session_id: &str) {
        if let Ok(mut children) = self.children.lock() {
            if let Some(mut child) = children.remove(session_id) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    fn close_all(&self) {
        if let Ok(mut children) = self.children.lock() {
            for (_, mut child) in children.drain() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Clone)]
struct SftpEndpoint {
    host: String,
    port: u16,
    user: String,
}
pub struct OpenSftpTransport {
    sessions: Mutex<HashMap<String, SftpEndpoint>>,
    cancelled: Mutex<HashSet<String>>,
}
impl Default for OpenSftpTransport {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            cancelled: Mutex::new(HashSet::new()),
        }
    }
}
impl OpenSftpTransport {
    fn batch(&self, session_id: &str, commands: &[String]) -> Result<String, TransportError> {
        let endpoint = self
            .sessions
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .get(session_id)
            .cloned()
            .ok_or(TransportError::Unavailable("session not found".into()))?;
        let mut child = Command::new("sftp")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-P",
                &endpoint.port.to_string(),
                &format!("{}@{}", endpoint.user, endpoint.host),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::Unavailable(format!("sftp: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(commands.join("\n").as_bytes())
                .map_err(|e| TransportError::Unavailable(e.to_string()))?;
            stdin.write_all(b"\nquit\n").ok();
        }
        let output = child
            .wait_with_output()
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        if !output.status.success() {
            return Err(TransportError::Unavailable(
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(256)
                    .collect(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
impl SftpTransport for OpenSftpTransport {
    fn register_session(&self, session_id: &str, host: &str, port: u16, user: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                session_id.to_owned(),
                SftpEndpoint {
                    host: host.to_owned(),
                    port,
                    user: user.to_owned(),
                },
            );
        }
    }
    fn unregister_session(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(session_id);
        }
    }
    fn close_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
    }
    fn supports_safe_append(&self) -> bool {
        false
    }
    fn upload_from_path(
        &self,
        session_id: &str,
        local: &std::path::Path,
        remote: &str,
        overwrite: bool,
        resume: bool,
    ) -> Result<(u64, String), TransportError> {
        let metadata =
            std::fs::metadata(local).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        if metadata.len() > 20 * 1024 * 1024 * 1024 {
            return Err(TransportError::Unavailable("file too large".into()));
        }
        let command = if resume { "reput" } else { "put" };
        let _ = overwrite; // OpenSSH sftp applies its own overwrite policy.
        self.batch(
            session_id,
            &[format!(
                "{command} \"{}\" {remote}",
                local.to_string_lossy()
            )],
        )?;
        Ok((metadata.len(), hash_file(local)?))
    }
    fn download_to_path(
        &self,
        session_id: &str,
        remote: &str,
        local: &std::path::Path,
        overwrite: bool,
        resume: bool,
    ) -> Result<(u64, String), TransportError> {
        if local.exists() && !overwrite && !resume {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        let command = if resume { "reget" } else { "get" };
        let _ = self.batch(
            session_id,
            &[format!(
                "{command} {remote} \"{}\"",
                local.to_string_lossy()
            )],
        )?;
        let metadata =
            std::fs::metadata(local).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        if metadata.len() > 20 * 1024 * 1024 * 1024 {
            return Err(TransportError::Unavailable("file too large".into()));
        }
        Ok((metadata.len(), hash_file(local)?))
    }
    fn list(&self, session_id: &str, path: &str) -> Result<Vec<String>, TransportError> {
        let output = self.batch(session_id, &[format!("ls -1 {path}")])?;
        Ok(output
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty() && !line.starts_with("sftp>") && !line.contains("Fetching")
            })
            .map(str::to_owned)
            .collect())
    }
    fn realpath(&self, session_id: &str, path: &str) -> Result<String, TransportError> {
        let output = self.batch(session_id, &[format!("realpath {path}")])?;
        output
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("sftp>"))
            .map(str::to_owned)
            .ok_or(TransportError::Unavailable("realpath unavailable".into()))
    }
    fn read_file(
        &self,
        session_id: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let temp =
            std::env::temp_dir().join(format!("termpilot-read-{}.tmp", uuid::Uuid::new_v4()));
        let _ = self.batch(
            session_id,
            &[format!("get {path} \"{}\"", temp.to_string_lossy())],
        )?;
        let file =
            std::fs::File::open(&temp).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let mut bytes = Vec::new();
        file.take(max_bytes as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let _ = std::fs::remove_file(temp);
        Ok(bytes)
    }
    fn write_file(
        &self,
        session_id: &str,
        path: &str,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<(), TransportError> {
        let temp =
            std::env::temp_dir().join(format!("termpilot-write-{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temp, bytes).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let command = if overwrite {
            format!("put \"{}\" {path}", temp.to_string_lossy())
        } else {
            format!("put -p \"{}\" {path}", temp.to_string_lossy())
        };
        let result = self.batch(session_id, &[command]);
        let _ = std::fs::remove_file(temp);
        result.map(|_| ())
    }
    fn delete(&self, session_id: &str, path: &str) -> Result<(), TransportError> {
        self.batch(session_id, &[format!("rm {path}")]).map(|_| ())
    }
    fn rename(
        &self,
        session_id: &str,
        source: &str,
        destination: &str,
        _overwrite: bool,
    ) -> Result<(), TransportError> {
        self.batch(session_id, &[format!("rename {source} {destination}")])
            .map(|_| ())
    }
    fn mkdir(&self, session_id: &str, path: &str) -> Result<(), TransportError> {
        self.batch(session_id, &[format!("mkdir {path}")])
            .map(|_| ())
    }
    fn cancel(&self, transfer_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(transfer_id.to_owned());
        }
    }
}

fn hash_file(path: &std::path::Path) -> Result<String, TransportError> {
    let mut file =
        std::fs::File::open(path).map_err(|e| TransportError::Unavailable(e.to_string()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
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
    fn execute_structured(
        &self,
        _session_id: &str,
        _argv: &[String],
        _cwd: &str,
    ) -> Result<(), TransportError> {
        Ok(())
    }
    fn resize(&self, _session_id: &str, _rows: u16, _cols: u16) -> Result<(), TransportError> {
        Ok(())
    }
    fn close(&self, _session_id: &str) {}
}

pub struct MockSftpTransport {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    directories: Arc<Mutex<HashSet<String>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
}

impl Default for MockSftpTransport {
    fn default() -> Self {
        let mut files = HashMap::new();
        files.insert(
            "~/app.conf".into(),
            b"# mock configuration\nPORT=8080\n".to_vec(),
        );
        files.insert(
            "~/README.md".into(),
            b"TermPilot isolated mock SFTP file\n".to_vec(),
        );
        files.insert("~/release.tar.gz".into(), vec![0u8; 1024]);
        let directories = ["~".to_owned(), "~/releases".to_owned()]
            .into_iter()
            .collect();
        Self {
            files: Arc::new(Mutex::new(files)),
            directories: Arc::new(Mutex::new(directories)),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

impl SftpTransport for MockSftpTransport {
    fn supports_safe_append(&self) -> bool {
        true
    }
    fn list(&self, _session_id: &str, path: &str) -> Result<Vec<String>, TransportError> {
        if path.is_empty() || path.contains('\0') || path.split(['/', '\\']).any(|p| p == "..") {
            return Err(TransportError::Unavailable("path escape".into()));
        }
        let prefix = if path == "~" {
            "~/".to_owned()
        } else {
            format!("{}/", path.trim_end_matches('/'))
        };
        let mut names = HashSet::new();
        for directory in self
            .directories
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .iter()
        {
            if let Some(name) = directory.strip_prefix(&prefix) {
                if !name.is_empty() && !name.contains('/') {
                    names.insert(format!("{name}/"));
                }
            }
        }
        for file in self
            .files
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .keys()
        {
            if let Some(name) = file.strip_prefix(&prefix) {
                if !name.is_empty() && !name.contains('/') {
                    names.insert(name.to_owned());
                }
            }
        }
        let mut result: Vec<_> = names.into_iter().collect();
        result.sort();
        Ok(result)
    }

    fn read_file(
        &self,
        _session_id: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, TransportError> {
        let files = self
            .files
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        let Some(bytes) = files.get(path) else {
            return Err(TransportError::Unavailable("file not found".into()));
        };
        Ok(bytes[..bytes.len().min(max_bytes)].to_vec())
    }

    fn write_file(
        &self,
        _session_id: &str,
        path: &str,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<(), TransportError> {
        let mut files = self
            .files
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        if files.contains_key(path) && !overwrite {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        files.insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn delete(&self, _session_id: &str, path: &str) -> Result<(), TransportError> {
        let mut files = self
            .files
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        if files.remove(path).is_none() {
            return Err(TransportError::Unavailable("file not found".into()));
        }
        Ok(())
    }

    fn rename(
        &self,
        _session_id: &str,
        source: &str,
        destination: &str,
        overwrite: bool,
    ) -> Result<(), TransportError> {
        let mut files = self
            .files
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        if files.contains_key(destination) && !overwrite {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        let Some(bytes) = files.remove(source) else {
            return Err(TransportError::Unavailable("file not found".into()));
        };
        files.insert(destination.to_owned(), bytes);
        Ok(())
    }

    fn mkdir(&self, _session_id: &str, path: &str) -> Result<(), TransportError> {
        let mut dirs = self
            .directories
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?;
        if !dirs.insert(path.to_owned()) {
            return Err(TransportError::Unavailable("directory exists".into()));
        }
        Ok(())
    }
    fn cancel(&self, transfer_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(transfer_id.to_owned());
        }
    }
}
