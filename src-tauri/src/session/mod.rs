//! Terminal sessions. Each session runs as a task that owns its connection;
//! the UI sends input through `Input` and receives output bytes on a raw
//! channel and state changes / questions on an event channel.

mod serial;
mod ssh;
mod telnet;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectSpec {
    pub protocol: String, // "ssh" | "telnet" | "serial"
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// saved device: the password comes from the OS credential store
    #[serde(default)]
    pub device_id: Option<String>,
    /// quick connect: typed by the user, not saved
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub serial_port: String,
    #[serde(default)]
    pub baud: u32,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEvent {
    #[serde(rename_all = "camelCase")]
    Status { state: &'static str, message: String },
    /// the session waits for `session_answer`
    #[serde(rename_all = "camelCase")]
    Prompt {
        kind: &'static str, // "password" | "hostKeyNew" | "hostKeyChanged" | "keyboard"
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        fingerprint: Option<String>,
        echo: bool,
    },
}

pub enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
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
}

impl Io {
    pub fn output(&self, bytes: &[u8]) {
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
}

#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Handle>>,
}

impl SessionManager {
    pub fn open(
        &self,
        config_dir: PathBuf,
        spec: ConnectSpec,
        data: Channel<InvokeResponseBody>,
        events: Channel<SessionEvent>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let pending = Arc::new(Mutex::new(None));
        let io = Io { config_dir, data, events, pending: pending.clone() };
        self.sessions.lock().unwrap().insert(id.clone(), Handle { input: tx, pending });

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
        });
        id
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
        "serial" => format!("Serial {} {} baud", spec.serial_port, spec.baud),
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
        let (host, port, user, pass) = (parts[0], parts[1].parse().unwrap(), parts[2], parts[3].to_string());

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
        let io = Io { config_dir: dir.clone(), data, events, pending };
        let spec = ConnectSpec {
            protocol: "ssh".into(),
            host: host.into(),
            port,
            username: user.into(),
            device_id: None,
            password: None, // forces the password prompt path
            serial_port: String::new(),
            baud: 0,
            cols: 100,
            rows: 30,
        };
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
}
