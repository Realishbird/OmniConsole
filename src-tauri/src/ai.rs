//! AI providers: Ollama and LM Studio on this computer, OpenAI, Anthropic
//! and any OpenAI-compatible endpoint. Every message is redacted before it
//! leaves the app; API keys come from the OS keychain (`ai/<provider id>`)
//! and never pass through the UI after they are saved.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::watch;

use crate::redact;
use crate::store;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpec {
    /// uuid; the API key lives in the keychain under ai/<id>
    pub id: String,
    /// ollama | lmstudio | openai | anthropic | compatible
    pub kind: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiEvent {
    Token { text: String },
    Done { text: String, redacted: usize },
    Error { message: String },
}

#[derive(Default)]
pub struct AiJobs {
    jobs: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl ProviderSpec {
    fn base(&self) -> String {
        let b = self.base_url.trim().trim_end_matches('/');
        if !b.is_empty() {
            return b.to_string();
        }
        match self.kind.as_str() {
            "ollama" => "http://127.0.0.1:11434".into(),
            "lmstudio" => "http://127.0.0.1:1234".into(),
            "openai" => "https://api.openai.com".into(),
            "anthropic" => "https://api.anthropic.com".into(),
            _ => String::new(),
        }
    }

    fn key(&self) -> Option<String> {
        if uuid::Uuid::parse_str(&self.id).is_err() {
            return None;
        }
        store::secret_get(&format!("ai/{}", self.id)).filter(|k| !k.is_empty())
    }

    fn local(&self) -> bool {
        matches!(self.kind.as_str(), "ollama" | "lmstudio") || self.base().contains("127.0.0.1") || self.base().contains("localhost")
    }

    fn client(&self) -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(if self.local() { 600 } else { 180 }))
            .build()?)
    }

    fn authorize(&self, req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let key = self.key();
        match self.kind.as_str() {
            "anthropic" => {
                let key = key.ok_or_else(|| anyhow!("no API key saved for this provider"))?;
                Ok(req.header("x-api-key", key).header("anthropic-version", "2023-06-01"))
            }
            "openai" => {
                let key = key.ok_or_else(|| anyhow!("no API key saved for this provider"))?;
                Ok(req.bearer_auth(key))
            }
            _ => Ok(match key {
                Some(k) => req.bearer_auth(k),
                None => req,
            }),
        }
    }
}

/// Redact every message; returns the messages and how many lines were masked.
fn redacted(messages: &[ChatMessage]) -> (Vec<ChatMessage>, usize) {
    let mut count = 0;
    let out = messages
        .iter()
        .map(|m| {
            let content = redact::text(&m.content);
            count += content.matches(redact::MASK).count().saturating_sub(m.content.matches(redact::MASK).count());
            ChatMessage { role: m.role.clone(), content }
        })
        .collect();
    (out, count)
}

/// Model names the provider offers.
#[tauri::command]
pub async fn ai_models(provider: ProviderSpec) -> CmdResult<Vec<String>> {
    list_models(&provider).await.map_err(err)
}

async fn list_models(p: &ProviderSpec) -> Result<Vec<String>> {
    let client = p.client()?;
    let base = p.base();
    if base.is_empty() {
        bail!("enter the provider's base URL");
    }
    let (url, path) = match p.kind.as_str() {
        "ollama" => (format!("{base}/api/tags"), "models.#.name"),
        "anthropic" => (format!("{base}/v1/models"), "data.#.id"),
        _ => (format!("{base}/v1/models"), "data.#.id"),
    };
    let resp = p.authorize(client.get(&url))?.send().await.with_context(|| format!("connecting to {url}"))?;
    if !resp.status().is_success() {
        bail!("{} returned {}: {}", url, resp.status(), resp.text().await.unwrap_or_default().chars().take(300).collect::<String>());
    }
    let v: Value = resp.json().await?;
    let (arr, field) = path.split_once(".#.").unwrap();
    let mut names: Vec<String> = v.get(arr).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|m| m.get(field).and_then(|s| s.as_str()).map(String::from)).collect()).unwrap_or_default();
    names.sort();
    Ok(names)
}

#[tauri::command]
pub fn ai_cancel(jobs: State<'_, AiJobs>, id: String) {
    if let Some(tx) = jobs.jobs.lock().unwrap().get(&id) {
        let _ = tx.send(true);
    }
}

/// Stream a chat completion. The full text is returned at the end too.
#[tauri::command]
pub async fn ai_chat(jobs: State<'_, AiJobs>, id: String, provider: ProviderSpec, messages: Vec<ChatMessage>, events: Channel<AiEvent>) -> CmdResult<String> {
    let (tx, rx) = watch::channel(false);
    jobs.jobs.lock().unwrap().insert(id.clone(), tx);
    let (messages, redacted_count) = redacted(&messages);
    let result = chat(&provider, &messages, rx, &events).await;
    jobs.jobs.lock().unwrap().remove(&id);
    match result {
        Ok(text) => {
            let _ = events.send(AiEvent::Done { text: text.clone(), redacted: redacted_count });
            Ok(text)
        }
        Err(e) => {
            let _ = events.send(AiEvent::Error { message: format!("{e:#}") });
            Err(format!("{e:#}"))
        }
    }
}

async fn chat(p: &ProviderSpec, messages: &[ChatMessage], mut cancel: watch::Receiver<bool>, events: &Channel<AiEvent>) -> Result<String> {
    let client = p.client()?;
    let base = p.base();
    if base.is_empty() {
        bail!("enter the provider's base URL");
    }
    if p.model.trim().is_empty() {
        bail!("choose a model");
    }
    let (url, body) = match p.kind.as_str() {
        "ollama" => (
            format!("{base}/api/chat"),
            json!({ "model": p.model, "stream": true, "messages": messages, "options": { "temperature": 0.2 } }),
        ),
        "anthropic" => {
            let system: Vec<&str> = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect();
            let rest: Vec<Value> = messages.iter().filter(|m| m.role != "system").map(|m| json!({ "role": m.role, "content": m.content })).collect();
            (
                format!("{base}/v1/messages"),
                json!({ "model": p.model, "max_tokens": 4096, "stream": true, "temperature": 0.2, "system": system.join("\n\n"), "messages": rest }),
            )
        }
        _ => (
            format!("{base}/v1/chat/completions"),
            json!({ "model": p.model, "stream": true, "temperature": 0.2, "messages": messages }),
        ),
    };
    let resp = p.authorize(client.post(&url))?.json(&body).send().await.with_context(|| format!("connecting to {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").or(v.get("error")).and_then(|m| m.as_str().map(String::from))).unwrap_or(text);
        bail!("{status}: {}", msg.chars().take(400).collect::<String>());
    }
    let mut resp = resp;
    let mut buf = String::new();
    let mut text = String::new();
    let ndjson = p.kind == "ollama";
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c?,
            _ = cancel.changed() => {
                if *cancel.borrow() { text.push_str("\n(stopped)"); break; }
                continue;
            }
        };
        let Some(chunk) = chunk else { break };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            let payload = if ndjson { line.as_str() } else if let Some(d) = line.strip_prefix("data:") { d.trim() } else { continue };
            if payload == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(payload) else { continue };
            if let Some(e) = v.pointer("/error/message").and_then(|m| m.as_str()) {
                bail!("{e}");
            }
            let piece = if ndjson {
                v.pointer("/message/content").and_then(|s| s.as_str()).map(String::from)
            } else if p.kind == "anthropic" {
                v.pointer("/delta/text").and_then(|s| s.as_str()).map(String::from)
            } else {
                v.pointer("/choices/0/delta/content").and_then(|s| s.as_str()).map(String::from)
            };
            if let Some(piece) = piece {
                if !piece.is_empty() {
                    text.push_str(&piece);
                    let _ = events.send(AiEvent::Token { text: piece });
                }
            }
        }
    }
    if text.trim().is_empty() {
        bail!("the model returned nothing");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_redacted() {
        let (m, n) = redacted(&[ChatMessage { role: "user".into(), content: "enable secret 5 $1$abcd$efgh\nsnmp-server community s3cret RO\nhostname sw1".into() }]);
        assert_eq!(n, 2);
        assert!(!m[0].content.contains("s3cret"));
        assert!(m[0].content.contains("hostname sw1"));
    }

    /// A one-shot HTTP server that answers with `body`; returns its base URL.
    async fn fake_server(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
            let _ = sock.read(&mut buf).await;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn streams_ollama_and_sse() {
        let base = fake_server("{\"message\":{\"content\":\"Hel\"},\"done\":false}\n{\"message\":{\"content\":\"lo\"},\"done\":true}\n").await;
        let p = ProviderSpec { id: "x".into(), kind: "ollama".into(), base_url: base, model: "m".into() };
        let (_tx, rx) = watch::channel(false);
        let events = Channel::new(|_| Ok(()));
        let text = chat(&p, &[ChatMessage { role: "user".into(), content: "hi".into() }], rx, &events).await.unwrap();
        assert_eq!(text, "Hello");

        let base = fake_server("data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}\n\ndata: [DONE]\n\n").await;
        let p = ProviderSpec { id: "x".into(), kind: "lmstudio".into(), base_url: base, model: "m".into() };
        let (_tx, rx) = watch::channel(false);
        let events = Channel::new(|_| Ok(()));
        let text = chat(&p, &[ChatMessage { role: "user".into(), content: "hi".into() }], rx, &events).await.unwrap();
        assert_eq!(text, "AB");

        let base = fake_server("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n").await;
        let p = ProviderSpec { id: "x".into(), kind: "anthropic".into(), base_url: base, model: "m".into() };
        // no key saved: must fail before sending
        let (_tx, rx) = watch::channel(false);
        let events = Channel::new(|_| Ok(()));
        let err = chat(&p, &[ChatMessage { role: "user".into(), content: "hi".into() }], rx, &events).await.unwrap_err();
        assert!(format!("{err:#}").contains("API key"));
    }

    #[test]
    fn defaults_per_kind() {
        let p = ProviderSpec { id: "x".into(), kind: "ollama".into(), base_url: "".into(), model: "qwen3:8b".into() };
        assert_eq!(p.base(), "http://127.0.0.1:11434");
        assert!(p.local());
        let p = ProviderSpec { id: "x".into(), kind: "openai".into(), base_url: "https://api.openai.com/".into(), model: "".into() };
        assert_eq!(p.base(), "https://api.openai.com");
        assert!(!p.local());
    }
}
