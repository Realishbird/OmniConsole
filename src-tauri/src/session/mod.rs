//! Terminal sessions. Each session runs as a task that owns its connection;
//! the UI sends input through `Input` and receives output bytes on a raw
//! channel and state changes / questions on an event channel.

pub mod autologin;
mod log;
pub mod runner;
mod serial;
pub mod ssh;
mod telnet;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::{mpsc, oneshot};

/// A jump host (bastion) the SSH connection is tunnelled through.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpSpec {
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default = "default_auth")]
    pub auth_method: String,
    #[serde(default)]
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectSpec {
    pub protocol: String, // "ssh" | "telnet" | "serial"
    /// tab title, used for the log file name
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// saved device: secrets come from the OS credential store
    #[serde(default)]
    pub device_id: Option<String>,
    /// quick connect: typed by the user, not saved
    #[serde(default)]
    pub password: Option<String>,
    /// Telnet and serial: answer the Username/Password questions
    #[serde(default)]
    pub auto_login: bool,

    // SSH
    /// "password" | "key" | "agent"
    #[serde(default = "default_auth")]
    pub auth_method: String,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub jump: Option<Box<JumpSpec>>,
    /// password prompts before the connection gives up
    #[serde(default = "default_retries")]
    pub retry_limit: u32,
    /// seconds, 0 = off
    #[serde(default = "default_keepalive")]
    pub keepalive: u32,

    // serial
    #[serde(default)]
    pub serial_port: String,
    #[serde(default)]
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default)]
    pub parity: String,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default)]
    pub flow_control: String,

    // logging
    #[serde(default)]
    pub log: bool,
    #[serde(default)]
    pub log_timestamps: bool,

    pub cols: u16,
    pub rows: u16,
}

fn default_retries() -> u32 {
    3
}

fn default_auth() -> String {
    "password".into()
}
fn default_keepalive() -> u32 {
    30
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEvent {
    #[serde(rename_all = "camelCase")]
    Status { state: &'static str, message: String },
    /// the session waits for `session_answer`
    #[serde(rename_all = "camelCase")]
    Prompt {
        kind: &'static str, // "password" | "username" | "passphrase" | "hostKeyNew" | "hostKeyChanged" | "keyboard"
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fingerprint: Option<String>,
        echo: bool,
    },
    /// the session is being written to this log file
    #[serde(rename_all = "camelCase")]
    Log { path: String },
}

pub enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    /// serial break / Telnet BRK (ROMMON and password recovery)
    Break,
    Close,
}

/// What a session task uses to talk to the UI.
#[derive(Clone)]
pub struct Io {
    /// settings folder (known host keys)
    pub config_dir: PathBuf,
    data: Channel<InvokeResponseBody>,
    events: Channel<SessionEvent>,
    pending: Arc<Mutex<Option<oneshot::Sender<Option<String>>>>>,
    log: Arc<Mutex<Option<log::SessionLog>>>,
    capture: Arc<runner::Capture>,
}

impl Io {
    pub fn output(&self, bytes: &[u8]) {
        if let Some(log) = self.log.lock().unwrap().as_mut() {
            log.feed(bytes);
        }
        self.capture.feed(bytes);
        let _ = self.data.send(InvokeResponseBody::Raw(bytes.to_vec()));
    }

    /// a line of text from OmniConsole itself (not from the device)
    pub fn notice(&self, text: &str) {
        self.output(format!("\r\n\x1b[90m{text}\x1b[0m\r\n").as_bytes());
    }

    pub fn status(&self, state: &'static str, message: impl Into<String>) {
        let _ = self.events.send(SessionEvent::Status { state, message: message.into() });
    }

    /// Ask the user something and wait for the answer (None = cancelled).
    pub async fn ask(
        &self,
        kind: &'static str,
        message: impl Into<String>,
        fingerprint: Option<String>,
        echo: bool,
    ) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        *self.pending.lock().unwrap() = Some(tx);
        let _ = self.events.send(SessionEvent::Prompt { kind, message: message.into(), fingerprint, echo });
        rx.await.ok().flatten()
    }
}

struct Handle {
    input: mpsc::UnboundedSender<Input>,
    pending: Arc<Mutex<Option<oneshot::Sender<Option<String>>>>>,
    capture: Arc<runner::Capture>,
    /// one command runs at a time per session
    busy: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Handle>>,
}

impl SessionManager {
    pub fn open(
        &self,
        config_dir: PathBuf,
        log_dir: &Path,
        spec: ConnectSpec,
        data: Channel<InvokeResponseBody>,
        events: Channel<SessionEvent>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let pending = Arc::new(Mutex::new(None));
        let capture = Arc::new(runner::Capture::default());
        let io = Io { config_dir, data, events, pending: pending.clone(), log: Arc::new(Mutex::new(None)), capture: capture.clone() };
        self.sessions
            .lock()
            .unwrap()
            .insert(id.clone(), Handle { input: tx, pending, capture, busy: Arc::new(tokio::sync::Mutex::new(())) });

        if spec.log {
            let label = if spec.label.is_empty() { describe(&spec) } else { spec.label.clone() };
            match log::SessionLog::create(log_dir, &label, &describe(&spec), spec.log_timestamps) {
                Ok(l) => {
                    let _ = io.events.send(SessionEvent::Log { path: l.path.display().to_string() });
                    *io.log.lock().unwrap() = Some(l);
                }
                Err(e) => io.notice(&format!("Session log not started: {e:#}")),
            }
        }

        tauri::async_runtime::spawn(async move {
            io.status("connecting", describe(&spec));
            let result = match spec.protocol.as_str() {
                "ssh" => ssh::run(&spec, &io, rx).await,
                "telnet" => telnet::run(&spec, &io, rx).await,
                "serial" => serial::run(&spec, &io, rx).await,
                other => Err(anyhow::anyhow!("unknown protocol {other}")),
            };
            match result {
                Ok(()) => {
                    io.notice("Session closed.");
                    io.status("closed", "Disconnected");
                }
                Err(e) => {
                    io.notice(&format!("Connection failed: {e:#}"));
                    io.status("error", format!("{e:#}"));
                }
            }
            // closes the log file
            io.log.lock().unwrap().take();
        });
        id
    }

    /// A prompt-only session for a file transfer connection: it gets an id
    /// the UI can answer prompts on, and an `Io` whose terminal output goes
    /// nowhere. Close it with `close` like any session.
    pub fn register_aux(&self, id: String, config_dir: PathBuf, events: Channel<SessionEvent>) -> Io {
        let (tx, _rx) = mpsc::unbounded_channel();
        let pending = Arc::new(Mutex::new(None));
        let capture = Arc::new(runner::Capture::default());
        let io = Io { config_dir, data: Channel::new(|_| Ok(())), events, pending: pending.clone(), log: Arc::new(Mutex::new(None)), capture: capture.clone() };
        self.sessions.lock().unwrap().insert(id, Handle { input: tx, pending, capture, busy: Arc::new(tokio::sync::Mutex::new(())) });
        io
    }

    /// Send raw bytes (no CR added) and collect what comes back until the
    /// device is quiet for `quiet`. For `?` completion.
    pub async fn probe(&self, id: &str, bytes: &[u8], quiet: std::time::Duration, timeout: std::time::Duration) -> anyhow::Result<String> {
        let (input, capture, busy) = {
            let sessions = self.sessions.lock().unwrap();
            let h = sessions.get(id).ok_or_else(|| anyhow::anyhow!("the session is closed"))?;
            (h.input.clone(), h.capture.clone(), h.busy.clone())
        };
        let _guard = busy.lock().await;
        capture.start();
        let _ = input.send(Input::Data(bytes.to_vec()));
        let deadline = tokio::time::Instant::now() + timeout;
        let mut last_len = 0;
        let mut last_change = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            let len = capture.text().len();
            if len != last_len {
                last_len = len;
                last_change = tokio::time::Instant::now();
            } else if len > 0 && last_change.elapsed() >= quiet {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&capture.stop()).into_owned())
    }

    fn send(&self, id: &str, input: Input) {
        if let Some(h) = self.sessions.lock().unwrap().get(id) {
            let _ = h.input.send(input);
        }
    }

    pub fn write(&self, id: &str, data: Vec<u8>) {
        self.send(id, Input::Data(data));
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) {
        self.send(id, Input::Resize(cols, rows));
    }

    /// Run a command on the session and return its cleaned output.
    pub async fn run_command(&self, id: &str, command: &str, timeout: std::time::Duration) -> anyhow::Result<String> {
        self.run_command_with(id, command, timeout, None).await
    }

    /// `run_command`, answering the command's questions when `answers` is given.
    pub async fn run_command_with(&self, id: &str, command: &str, timeout: std::time::Duration, answers: Option<runner::Answers>) -> anyhow::Result<String> {
        let (input, capture, busy) = {
            let sessions = self.sessions.lock().unwrap();
            let h = sessions.get(id).ok_or_else(|| anyhow::anyhow!("the session is closed"))?;
            (h.input.clone(), h.capture.clone(), h.busy.clone())
        };
        let _guard = busy.lock().await;
        let send = move |bytes: Vec<u8>| {
            let _ = input.send(Input::Data(bytes));
        };
        runner::Runner { capture: &capture, send: &send }.run_with(command, timeout, answers).await
    }

    pub fn send_break(&self, id: &str) {
        self.send(id, Input::Break);
    }

    pub fn close(&self, id: &str) {
        if let Some(h) = self.sessions.lock().unwrap().remove(id) {
            // a session waiting on a prompt gives up
            if let Some(p) = h.pending.lock().unwrap().take() {
                let _ = p.send(None);
            }
            let _ = h.input.send(Input::Close);
        }
    }

    pub fn answer(&self, id: &str, answer: Option<String>) {
        if let Some(h) = self.sessions.lock().unwrap().get(id) {
            if let Some(p) = h.pending.lock().unwrap().take() {
                let _ = p.send(answer);
            }
        }
    }
}

pub fn describe(spec: &ConnectSpec) -> String {
    match spec.protocol.as_str() {
        "serial" => format!("Serial {} {}", spec.serial_port, serial::settings_label(spec)),
        p => format!("{} {}:{}", p.to_uppercase(), spec.host, spec.port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end SSH login and shell against a real server:
    /// OMNI_SSH_TEST=host:port:user:password cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn ssh_login_and_shell() {
        let target = std::env::var("OMNI_SSH_TEST").expect("set OMNI_SSH_TEST=host:port:user:password");
        let parts: Vec<&str> = target.splitn(4, ':').collect();
        let (host, port, user, pass) = (parts[0], parts[1].parse::<u16>().unwrap(), parts[2], parts[3].to_string());

        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let pending: Arc<Mutex<Option<oneshot::Sender<Option<String>>>>> = Arc::new(Mutex::new(None));
        let out = output.clone();
        let data = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                out.lock().unwrap().extend(bytes);
            }
            Ok(())
        });
        let answer_slot = pending.clone();
        let events = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let v: serde_json::Value = serde_json::from_str(&json).unwrap();
                if v["type"] == "prompt" {
                    let answer = match v["kind"].as_str() {
                        Some("hostKeyNew") => Some("yes".to_string()),
                        Some("password") => Some(pass.clone()),
                        _ => None,
                    };
                    if let Some(tx) = answer_slot.lock().unwrap().take() {
                        let _ = tx.send(answer);
                    }
                }
            }
            Ok(())
        });
        let dir = std::env::temp_dir().join(format!("omni-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let io = Io { config_dir: dir.clone(), data, events, pending, log: Arc::new(Mutex::new(None)), capture: Default::default() };
        let mut spec: ConnectSpec = serde_json::from_value(serde_json::json!({
            "protocol": "ssh", "host": host, "port": port, "username": user, "cols": 100, "rows": 30,
        }))
        .unwrap();
        spec.password = None; // forces the password prompt path
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(Input::Resize(120, 40)).unwrap();
        tx.send(Input::Data(b"echo omni-$((40+2))\r".to_vec())).unwrap();
        tx.send(Input::Data(b"exit\r".to_vec())).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(20), ssh::run(&spec, &io, rx))
            .await
            .expect("timed out")
            .expect("ssh session failed");
        let text = String::from_utf8_lossy(&output.lock().unwrap()).to_string();
        assert!(text.contains("omni-42"), "output was: {text}");
        // the host key was remembered
        assert!(std::fs::read_to_string(dir.join("known_hosts.json")).unwrap().contains(&format!("{host}:{port}")));
    }

    /// Key login through a jump host (the same server twice is fine):
    /// OMNI_SSH_KEY_TEST=host:port:user:/path/to/key cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn ssh_key_through_jump_host() {
        let target = std::env::var("OMNI_SSH_KEY_TEST").expect("set OMNI_SSH_KEY_TEST=host:port:user:keypath");
        let parts: Vec<&str> = target.splitn(4, ':').collect();
        let (host, port, user, key) = (parts[0], parts[1].parse::<u16>().unwrap(), parts[2], parts[3]);

        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let pending: Arc<Mutex<Option<oneshot::Sender<Option<String>>>>> = Arc::new(Mutex::new(None));
        let out = output.clone();
        let data = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                out.lock().unwrap().extend(bytes);
            }
            Ok(())
        });
        let answer_slot = pending.clone();
        let events = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let v: serde_json::Value = serde_json::from_str(&json).unwrap();
                if v["type"] == "prompt" {
                    // only host keys may be asked; a password prompt means key auth failed
                    let answer = (v["kind"] == "hostKeyNew").then(|| "yes".to_string());
                    if let Some(tx) = answer_slot.lock().unwrap().take() {
                        let _ = tx.send(answer);
                    }
                }
            }
            Ok(())
        });
        let dir = std::env::temp_dir().join(format!("omni-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let io = Io { config_dir: dir.clone(), data, events, pending, log: Arc::new(Mutex::new(None)), capture: Default::default() };
        // OMNI_SSH_AUTH=agent uses the running ssh-agent instead of the key file
        let method = std::env::var("OMNI_SSH_AUTH").unwrap_or_else(|_| "key".into());
        let spec: ConnectSpec = serde_json::from_value(serde_json::json!({
            "protocol": "ssh", "host": "localhost", "port": port, "username": user,
            "authMethod": method, "keyPath": key, "cols": 100, "rows": 30,
            "jump": { "host": host, "port": port, "username": user, "authMethod": method, "keyPath": key },
        }))
        .unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(Input::Data(b"echo omni-$((40+2))\r".to_vec())).unwrap();
        tx.send(Input::Data(b"exit\r".to_vec())).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(20), ssh::run(&spec, &io, rx))
            .await
            .expect("timed out")
            .expect("ssh session failed");
        let text = String::from_utf8_lossy(&output.lock().unwrap()).to_string();
        assert!(text.contains("omni-42"), "output was: {text}");
        assert!(text.contains("jump host"), "output was: {text}");
    }
}
