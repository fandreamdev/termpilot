//! Transport boundary. Concrete SSH/SFTP implementations must live behind
//! these traits so command handlers can validate policy before opening a
//! channel. The first milestone uses the mock implementation in tests.
use base64::Engine;
use sha2::Digest;
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::PathBuf,
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
#[derive(Clone, Debug)]
pub enum CredentialMaterial {
    Password(String),
    PrivateKey(PathBuf),
    SshAgent,
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
        _credential: Option<&CredentialMaterial>,
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
    /// Execute a policy-validated argv and return bounded stdout plus exit code.
    /// Implementations may use a dedicated non-PTY channel; the default keeps
    /// compatibility with transports that can only dispatch into the shell.
    fn execute_structured_capture(
        &self,
        session_id: &str,
        argv: &[String],
        cwd: &str,
        _timeout: std::time::Duration,
        _max_output: usize,
    ) -> Result<(Vec<u8>, i32), TransportError> {
        self.execute_structured(session_id, argv, cwd)?;
        Ok((Vec::new(), 0))
    }
    fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), TransportError>;
    fn close(&self, session_id: &str);
    fn close_all(&self) {}
}
pub trait SftpTransport: Send + Sync {
    fn register_session(
        &self,
        _session_id: &str,
        _host: &str,
        _port: u16,
        _user: &str,
        _credential: Option<&CredentialMaterial>,
    ) {
    }
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
    fn is_cancelled(&self, _transfer_id: &str) -> bool {
        false
    }
}

/// Optional adapter backed by the OpenSSH binaries installed on Windows.
/// It is intentionally opt-in (`TERMPILOT_TRANSPORT=openssh`) so local tests
/// never contact an unintended server. Authentication is delegated to the
/// user's OpenSSH agent/key configuration; passwords are never passed on a
/// command line.
#[derive(Clone)]
struct SshEndpoint {
    host: String,
    port: u16,
    user: String,
    credential: Option<CredentialMaterial>,
}
pub struct OpenSshTransport {
    children: Mutex<HashMap<String, Child>>,
    endpoints: Mutex<HashMap<String, SshEndpoint>>,
}
impl Default for OpenSshTransport {
    fn default() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
            endpoints: Mutex::new(HashMap::new()),
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
        let key_blob = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.split_whitespace().nth(2))
            .and_then(|key| base64::engine::general_purpose::STANDARD.decode(key).ok());
        let Some(key_blob) = key_blob else {
            return Ok(None);
        };
        let digest = sha2::Sha256::digest(key_blob);
        Ok(Some(format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
        )))
    }
    fn connect(&self, host: &str, port: u16, user: &str) -> Result<String, TransportError> {
        let id = format!("{user}@{host}:{port}");
        self.connect_for_session(&id, host, port, user, None)
            .map(|_| id)
    }
    fn connect_for_session(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        user: &str,
        credential: Option<&CredentialMaterial>,
    ) -> Result<String, TransportError> {
        let password = match credential {
            Some(CredentialMaterial::Password(value)) if !value.is_empty() => Some(value),
            Some(CredentialMaterial::Password(_)) => return Err(TransportError::Authentication),
            _ => None,
        };
        let mut args = vec![
            "-tt".to_owned(),
            "-o".to_owned(),
            if password.is_some() {
                "BatchMode=no".to_owned()
            } else {
                "BatchMode=yes".to_owned()
            },
            "-o".to_owned(),
            "StrictHostKeyChecking=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-p".to_owned(),
            port.to_string(),
        ];
        if let Some(CredentialMaterial::PrivateKey(path)) = credential {
            args.push("-i".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        args.push(format!("{user}@{host}"));
        let mut command = Command::new("ssh");
        command.args(args);
        if let Some(password) = password {
            // OpenSSH's askpass hook keeps the secret in process memory and
            // avoids placing it in argv, SQLite, logs, or a temporary file.
            // The helper is only enabled for this child process.
            command
                .env("TERMPILOT_PASSWORD", password)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(
                    "SSH_ASKPASS",
                    "powershell.exe -NoProfile -NonInteractive -Command [Console]::Write($env:TERMPILOT_PASSWORD)",
                );
        }
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| TransportError::Unavailable(format!("ssh: {e}")))?;
        self.children
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .insert(session_id.to_owned(), child);
        if let Ok(mut endpoints) = self.endpoints.lock() {
            endpoints.insert(
                session_id.to_owned(),
                SshEndpoint {
                    host: host.to_owned(),
                    port,
                    user: user.to_owned(),
                    credential: credential.cloned(),
                },
            );
        }
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
    fn execute_structured_capture(
        &self,
        session_id: &str,
        argv: &[String],
        cwd: &str,
        timeout: std::time::Duration,
        max_output: usize,
    ) -> Result<(Vec<u8>, i32), TransportError> {
        if argv.is_empty() {
            return Err(TransportError::Unavailable("empty command".into()));
        }
        // Use a dedicated non-PTY channel so command output and exit status do
        // not interleave with the user's interactive shell. Authentication is
        // delegated to the same OpenSSH agent/configuration as the session.
        let SshEndpoint {
            host,
            port,
            user,
            credential,
        } = self
            .endpoints
            .lock()
            .map_err(|_| TransportError::Unavailable("lock".into()))?
            .get(session_id)
            .cloned()
            .ok_or(TransportError::Unavailable("session not found".into()))?;
        let command = format!(
            "cd {} && {}",
            shell_quote(cwd),
            argv.iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut args = vec![
            "-T".to_owned(),
            "-o".to_owned(),
            if matches!(credential, Some(CredentialMaterial::Password(_))) {
                "BatchMode=no".to_owned()
            } else {
                "BatchMode=yes".to_owned()
            },
            "-o".to_owned(),
            "StrictHostKeyChecking=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-p".to_owned(),
            port.to_string(),
        ];
        if let Some(CredentialMaterial::PrivateKey(path)) = credential.as_ref() {
            args.push("-i".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        args.push(format!("{user}@{host}"));
        args.push(command);
        let mut command_process = Command::new("ssh");
        command_process.args(args);
        if let Some(CredentialMaterial::Password(password)) = credential.as_ref() {
            command_process
                .env("TERMPILOT_PASSWORD", password)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(
                    "SSH_ASKPASS",
                    "powershell.exe -NoProfile -NonInteractive -Command [Console]::Write($env:TERMPILOT_PASSWORD)",
                );
        }
        let child = command_process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::Unavailable(format!("ssh: {e}")))?;
        let pid = child.id();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });
        let output = match receiver.recv_timeout(timeout) {
            Ok(result) => result.map_err(|e| TransportError::Unavailable(e.to_string()))?,
            Err(_) => {
                #[cfg(windows)]
                {
                    let _ = Command::new("taskkill")
                        .args(["/PID", &pid.to_string(), "/T", "/F"])
                        .output();
                }
                #[cfg(not(windows))]
                let _ = pid;
                return Err(TransportError::Timeout);
            }
        };
        let mut stdout = output.stdout;
        stdout.truncate(max_output);
        Ok((stdout, output.status.code().unwrap_or(1)))
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
        if let Ok(mut endpoints) = self.endpoints.lock() {
            endpoints.remove(session_id);
        }
    }
    fn close_all(&self) {
        if let Ok(mut children) = self.children.lock() {
            for (_, mut child) in children.drain() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Ok(mut endpoints) = self.endpoints.lock() {
            endpoints.clear();
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
    credential: Option<CredentialMaterial>,
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
        let mut args = vec![
            "-o".to_owned(),
            if matches!(endpoint.credential, Some(CredentialMaterial::Password(_))) {
                "BatchMode=no".to_owned()
            } else {
                "BatchMode=yes".to_owned()
            },
            "-o".to_owned(),
            "StrictHostKeyChecking=yes".to_owned(),
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-P".to_owned(),
            endpoint.port.to_string(),
        ];
        if let Some(CredentialMaterial::PrivateKey(path)) = endpoint.credential.as_ref() {
            args.push("-i".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        args.push(format!("{}@{}", endpoint.user, endpoint.host));
        let mut command = Command::new("sftp");
        command.args(args);
        if let Some(CredentialMaterial::Password(password)) = endpoint.credential.as_ref() {
            command
                .env("TERMPILOT_PASSWORD", password)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env(
                    "SSH_ASKPASS",
                    "powershell.exe -NoProfile -NonInteractive -Command [Console]::Write($env:TERMPILOT_PASSWORD)",
                );
        }
        let mut child = command
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
    fn remote_exists(&self, session_id: &str, path: &str) -> bool {
        self.batch(session_id, &[format!("ls {}", sftp_quote(path))])
            .map(|output| {
                output
                    .lines()
                    .any(|line| !line.trim().is_empty() && !line.trim().starts_with("sftp>"))
            })
            .unwrap_or(false)
    }
}
impl SftpTransport for OpenSftpTransport {
    fn register_session(
        &self,
        session_id: &str,
        host: &str,
        port: u16,
        user: &str,
        credential: Option<&CredentialMaterial>,
    ) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(
                session_id.to_owned(),
                SftpEndpoint {
                    host: host.to_owned(),
                    port,
                    user: user.to_owned(),
                    credential: credential.cloned(),
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
        if !overwrite && !resume && self.remote_exists(session_id, remote) {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        self.batch(
            session_id,
            &[format!(
                "{command} {} {}",
                sftp_quote(&local.to_string_lossy()),
                sftp_quote(remote)
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
                "{command} {} {}",
                sftp_quote(remote),
                sftp_quote(&local.to_string_lossy())
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
        let output = self.batch(session_id, &[format!("ls -1 {}", sftp_quote(path))])?;
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
        let output = self.batch(session_id, &[format!("realpath {}", sftp_quote(path))])?;
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
            &[format!(
                "get {} {}",
                sftp_quote(path),
                sftp_quote(&temp.to_string_lossy())
            )],
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
        if !overwrite && self.remote_exists(session_id, path) {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        let temp =
            std::env::temp_dir().join(format!("termpilot-write-{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temp, bytes).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let command = if overwrite {
            format!(
                "put {} {}",
                sftp_quote(&temp.to_string_lossy()),
                sftp_quote(path)
            )
        } else {
            format!(
                "put -p {} {}",
                sftp_quote(&temp.to_string_lossy()),
                sftp_quote(path)
            )
        };
        let result = self.batch(session_id, &[command]);
        let _ = std::fs::remove_file(temp);
        result.map(|_| ())
    }
    fn delete(&self, session_id: &str, path: &str) -> Result<(), TransportError> {
        self.batch(session_id, &[format!("rm {}", sftp_quote(path))])
            .map(|_| ())
    }
    fn rename(
        &self,
        session_id: &str,
        source: &str,
        destination: &str,
        overwrite: bool,
    ) -> Result<(), TransportError> {
        if !overwrite && self.remote_exists(session_id, destination) {
            return Err(TransportError::Unavailable("destination exists".into()));
        }
        self.batch(
            session_id,
            &[format!(
                "rename {} {}",
                sftp_quote(source),
                sftp_quote(destination)
            )],
        )
        .map(|_| ())
    }
    fn mkdir(&self, session_id: &str, path: &str) -> Result<(), TransportError> {
        self.batch(session_id, &[format!("mkdir {}", sftp_quote(path))])
            .map(|_| ())
    }
    fn cancel(&self, transfer_id: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(transfer_id.to_owned());
        }
    }
    fn is_cancelled(&self, transfer_id: &str) -> bool {
        self.cancelled
            .lock()
            .map(|items| items.contains(transfer_id))
            .unwrap_or(true)
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

/// Quote a path for the OpenSSH sftp command language. Remote paths are
/// validated by the command layer as well, but quoting here prevents spaces or
/// wildcard characters from being interpreted as multiple operands.
fn sftp_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sftp_paths_are_quoted_as_single_operands() {
        assert_eq!(sftp_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(sftp_quote("/tmp/it's"), "'/tmp/it'\\''s'");
    }

    #[test]
    fn mock_fingerprint_is_stable_and_standard_shaped() {
        let transport = MockSshTransport;
        let first = transport.fingerprint("localhost", 22).unwrap().unwrap();
        let second = transport.fingerprint("localhost", 22).unwrap().unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("SHA256:"));
    }
}

#[derive(Default)]
pub struct MockSshTransport;
impl SshTransport for MockSshTransport {
    fn fingerprint(&self, host: &str, port: u16) -> Result<Option<String>, TransportError> {
        let digest = sha2::Sha256::digest(format!("mock:{host}:{port}").as_bytes());
        Ok(Some(format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
        )))
    }
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
    fn execute_structured_capture(
        &self,
        _session_id: &str,
        argv: &[String],
        _cwd: &str,
        _timeout: std::time::Duration,
        max_output: usize,
    ) -> Result<(Vec<u8>, i32), TransportError> {
        let output = match argv {
            [program, flag] if program == "df" && flag == "-h" => {
                "/dev/sda2 80G 52G 25G 68% /var\n"
            }
            [program] if program == "pwd" => "/home/ops\n",
            [program] if program == "whoami" => "ops\n",
            _ => "command dispatched to remote shell\n",
        };
        let bytes = output.as_bytes()[..output.len().min(max_output)].to_vec();
        Ok((bytes, 0))
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
        if !metadata.is_file() || metadata.len() > 20 * 1024 * 1024 * 1024 {
            return Err(TransportError::Unavailable(
                "file too large or not a file".into(),
            ));
        }
        let mut file =
            std::fs::File::open(local).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let mut bytes = Vec::with_capacity(metadata.len().min(16 * 1024 * 1024) as usize);
        file.read_to_end(&mut bytes)
            .map_err(|e| TransportError::Unavailable(e.to_string()))?;
        let hash = hex::encode(sha2::Sha256::digest(&bytes));
        if resume {
            let existing = self
                .files
                .lock()
                .map_err(|_| TransportError::Unavailable("lock".into()))?
                .get(remote)
                .cloned()
                .unwrap_or_default();
            if existing.len() > bytes.len() || !bytes.starts_with(&existing) {
                return Err(TransportError::Unavailable("resume prefix mismatch".into()));
            }
        }
        self.write_file(session_id, remote, &bytes, overwrite)?;
        Ok((metadata.len(), hash))
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
        let bytes = self.read_file(session_id, remote, 20 * 1024 * 1024 * 1024)?;
        if resume && local.exists() {
            let existing =
                std::fs::read(local).map_err(|e| TransportError::Unavailable(e.to_string()))?;
            if existing.len() > bytes.len() || !bytes.starts_with(&existing) {
                return Err(TransportError::Unavailable("resume prefix mismatch".into()));
            }
        }
        std::fs::write(local, &bytes).map_err(|e| TransportError::Unavailable(e.to_string()))?;
        Ok((
            bytes.len() as u64,
            hex::encode(sha2::Sha256::digest(&bytes)),
        ))
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
    fn is_cancelled(&self, transfer_id: &str) -> bool {
        self.cancelled
            .lock()
            .map(|items| items.contains(transfer_id))
            .unwrap_or(true)
    }
}
