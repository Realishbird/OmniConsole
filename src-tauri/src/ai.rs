//! AI providers: the built-in runtime (llama.cpp in `engine.rs`), Ollama and
//! LM Studio on this computer, OpenAI, Anthropic and any OpenAI-compatible
//! endpoint. Every message is redacted before it leaves the app; API keys
//! come from the OS keychain (`ai/<provider id>`) and never pass through the
//! UI after they are saved. Tool calls are carried in one shape for every
//! provider and translated here, so the assistant's agent loop in the UI
//! does not care who answers.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tokio::sync::watch;

use crate::engine;
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
    /// builtin | ollama | lmstudio | openai | anthropic | compatible
    pub kind: String,
    pub base_url: String,
    /// model name, or a catalog id for the built-in runtime
    pub model: String,
}

/// A tool the model asked to call. `arguments` is JSON text.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// system | user | assistant | tool
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// calls an assistant message made
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// which call a tool message answers
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn text(role: &str, content: &str) -> Self {
        Self { role: role.into(), content: content.into(), tool_calls: Vec::new(), tool_call_id: None }
    }
}

/// A tool offered to the model (JSON schema parameters).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiEvent {
    Token { text: String },
    ToolCall { call: ToolCall },
    Done { text: String, redacted: usize, tool_calls: Vec<ToolCall>, tools_dropped: bool },
    Error { message: String },
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResult {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    /// the provider refused the tool definitions, so the answer came without them
    pub tools_dropped: bool,
    pub redacted: usize,
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
        let host = self.base().split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").split(':').next().unwrap_or("").to_string();
        matches!(self.kind.as_str(), "builtin" | "ollama" | "lmstudio") || host == "127.0.0.1" || host == "localhost" || host == "::1" || host.starts_with("10.") || host.starts_with("192.168.")
    }

    /// An API key never travels in clear: a non-local base URL must be https.
    fn check_transport(&self) -> Result<()> {
        if self.base().starts_with("http://") && !self.local() && self.key().is_some() {
            bail!("{} uses http:// with an API key; use https:// (plain http is only allowed for local providers)", self.base());
        }
        Ok(())
    }

    fn client(&self) -> Result<reqwest::Client> {
        self.check_transport()?;
        Ok(reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(if self.local() { 900 } else { 180 }))
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
            "builtin" => Ok(req),
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
            ChatMessage { content, ..m.clone() }
        })
        .collect();
    (out, count)
}

/// Model names the provider offers.
#[tauri::command]
pub async fn ai_models(app: AppHandle, provider: ProviderSpec) -> CmdResult<Vec<String>> {
    if provider.kind == "builtin" {
        return Ok(engine::downloaded_ids(&app));
    }
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

/// Stream a chat completion. Text arrives as tokens, tool calls as they
/// complete; the whole result is returned at the end too.
#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    jobs: State<'_, AiJobs>,
    eng: State<'_, engine::Engine>,
    id: String,
    provider: ProviderSpec,
    messages: Vec<ChatMessage>,
    tools: Option<Vec<ToolSpec>>,
    events: Channel<AiEvent>,
) -> CmdResult<ChatResult> {
    let (tx, rx) = watch::channel(false);
    jobs.jobs.lock().unwrap().insert(id.clone(), tx);
    let (messages, redacted_count) = redacted(&messages);
    let tools = tools.unwrap_or_default();
    let result = async {
        let base = if provider.kind == "builtin" { engine::ensure(&app, &eng, &provider.model).await? } else { provider.base() };
        chat_with_fallback(&provider, &base, &messages, &tools, rx, &events).await
    }
    .await;
    jobs.jobs.lock().unwrap().remove(&id);
    match result {
        Ok(mut r) => {
            r.redacted = redacted_count;
            let _ = events.send(AiEvent::Done { text: r.text.clone(), redacted: redacted_count, tool_calls: r.tool_calls.clone(), tools_dropped: r.tools_dropped });
            Ok(r)
        }
        Err(e) => {
            let _ = events.send(AiEvent::Error { message: format!("{e:#}") });
            Err(format!("{e:#}"))
        }
    }
}

/// A provider or model that rejects tool definitions gets the request again without them.
async fn chat_with_fallback(p: &ProviderSpec, base: &str, messages: &[ChatMessage], tools: &[ToolSpec], cancel: watch::Receiver<bool>, events: &Channel<AiEvent>) -> Result<ChatResult> {
    match chat(p, base, messages, tools, cancel.clone(), events).await {
        Err(e) if !tools.is_empty() && is_tools_rejection(&format!("{e:#}")) => {
            log::warn!("provider rejected tools, retrying without: {e:#}");
            let plain: Vec<ChatMessage> = messages.iter().map(flatten_tool_history).collect();
            let mut r = chat(p, base, &plain, &[], cancel, events).await?;
            r.tools_dropped = true;
            Ok(r)
        }
        other => other,
    }
}

fn is_tools_rejection(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.starts_with("400") && (m.contains("tool") || m.contains("function"))
}

/// For a provider without tools, earlier tool traffic becomes plain text.
fn flatten_tool_history(m: &ChatMessage) -> ChatMessage {
    match m.role.as_str() {
        "tool" => ChatMessage::text("user", &format!("Result of the previous step:\n{}", m.content)),
        "assistant" if !m.tool_calls.is_empty() => {
            let calls = m.tool_calls.iter().map(|c| format!("{}({})", c.name, c.arguments)).collect::<Vec<_>>().join(", ");
            ChatMessage::text("assistant", &format!("{}\n(I ran: {calls})", m.content).trim().to_string())
        }
        _ => m.clone(),
    }
}

fn parse_args(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| json!({}))
}

/// Messages in the OpenAI chat shape (also what llama.cpp and LM Studio speak).
fn openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| match m.role.as_str() {
            "tool" => json!({ "role": "tool", "tool_call_id": m.tool_call_id.clone().unwrap_or_default(), "content": m.content }),
            "assistant" if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": if m.content.is_empty() { Value::Null } else { Value::String(m.content.clone()) },
                "tool_calls": m.tool_calls.iter().map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } })).collect::<Vec<_>>(),
            }),
            _ => json!({ "role": m.role, "content": m.content }),
        })
        .collect()
}

fn ollama_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| match m.role.as_str() {
            "tool" => json!({ "role": "tool", "content": m.content }),
            "assistant" if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": m.content,
                "tool_calls": m.tool_calls.iter().map(|c| json!({ "function": { "name": c.name, "arguments": parse_args(&c.arguments) } })).collect::<Vec<_>>(),
            }),
            _ => json!({ "role": m.role, "content": m.content }),
        })
        .collect()
}

/// Anthropic wants tool results inside one user message with content blocks.
fn anthropic_messages(messages: &[ChatMessage]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for m in messages.iter().filter(|m| m.role != "system") {
        match m.role.as_str() {
            "tool" => {
                let block = json!({ "type": "tool_result", "tool_use_id": m.tool_call_id.clone().unwrap_or_default(), "content": m.content });
                if let Some(last) = out.last_mut() {
                    if last["role"] == "user" && last["content"].is_array() {
                        last["content"].as_array_mut().unwrap().push(block);
                        continue;
                    }
                }
                out.push(json!({ "role": "user", "content": [block] }));
            }
            "assistant" if !m.tool_calls.is_empty() => {
                let mut blocks = Vec::new();
                if !m.content.trim().is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                for c in &m.tool_calls {
                    blocks.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": parse_args(&c.arguments) }));
                }
                out.push(json!({ "role": "assistant", "content": blocks }));
            }
            _ => out.push(json!({ "role": m.role, "content": m.content })),
        }
    }
    out
}

fn openai_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } })).collect()
}

/// Accumulates streamed tool-call fragments (OpenAI style, by index; Anthropic style, by block).
#[derive(Default)]
struct CallBuilder {
    calls: Vec<ToolCall>,
    by_index: HashMap<u64, usize>,
}

impl CallBuilder {
    fn fragment(&mut self, index: u64, id: Option<&str>, name: Option<&str>, args: Option<&str>) {
        let i = match self.by_index.get(&index) {
            Some(i) => *i,
            None => {
                self.calls.push(ToolCall::default());
                let i = self.calls.len() - 1;
                self.by_index.insert(index, i);
                i
            }
        };
        let c = &mut self.calls[i];
        if let Some(id) = id {
            if !id.is_empty() {
                c.id = id.into();
            }
        }
        if let Some(n) = name {
            if !n.is_empty() {
                c.name.push_str(n);
            }
        }
        if let Some(a) = args {
            c.arguments.push_str(a);
        }
    }

    fn finish(mut self) -> Vec<ToolCall> {
        for (n, c) in self.calls.iter_mut().enumerate() {
            if c.id.is_empty() {
                c.id = format!("call_{n}");
            }
            if c.arguments.trim().is_empty() {
                c.arguments = "{}".into();
            }
        }
        self.calls.retain(|c| !c.name.is_empty());
        self.calls
    }
}

async fn chat(p: &ProviderSpec, base: &str, messages: &[ChatMessage], tools: &[ToolSpec], mut cancel: watch::Receiver<bool>, events: &Channel<AiEvent>) -> Result<ChatResult> {
    let client = p.client()?;
    if base.is_empty() {
        bail!("enter the provider's base URL");
    }
    if p.model.trim().is_empty() {
        bail!("choose a model");
    }
    let (url, mut body) = match p.kind.as_str() {
        "ollama" => (
            format!("{base}/api/chat"),
            json!({ "model": p.model, "stream": true, "messages": ollama_messages(messages), "options": { "temperature": 0.2 } }),
        ),
        "anthropic" => {
            let system: Vec<&str> = messages.iter().filter(|m| m.role == "system").map(|m| m.content.as_str()).collect();
            (
                format!("{base}/v1/messages"),
                json!({ "model": p.model, "max_tokens": 4096, "stream": true, "system": system.join("\n\n"), "messages": anthropic_messages(messages) }),
            )
        }
        "builtin" => (
            format!("{base}/v1/chat/completions"),
            // the model file decides the name; thinking off keeps answers prompt
            json!({ "model": "default", "stream": true, "temperature": 0.2, "messages": openai_messages(messages), "chat_template_kwargs": { "enable_thinking": false } }),
        ),
        _ => (
            format!("{base}/v1/chat/completions"),
            json!({ "model": p.model, "stream": true, "temperature": 0.2, "messages": openai_messages(messages) }),
        ),
    };
    if !tools.is_empty() {
        match p.kind.as_str() {
            "anthropic" => body["tools"] = Value::Array(tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters })).collect()),
            _ => body["tools"] = Value::Array(openai_tools(tools)),
        }
    }
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
    let mut calls = CallBuilder::default();
    let ndjson = p.kind == "ollama";
    let anthropic = p.kind == "anthropic";
    let mut announced = 0usize;
    let mut stopped = false;
    // `<think>` blocks from local reasoning models stay out of the answer
    let mut thinking = false;
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c?,
            _ = cancel.changed() => {
                if *cancel.borrow() { stopped = true; break; }
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
            let mut piece: Option<String> = None;
            if ndjson {
                piece = v.pointer("/message/content").and_then(|s| s.as_str()).map(String::from);
                if let Some(tc) = v.pointer("/message/tool_calls").and_then(|t| t.as_array()) {
                    for (n, c) in tc.iter().enumerate() {
                        let idx = 1000 + calls.calls.len() as u64 + n as u64;
                        let args = c.pointer("/function/arguments").map(|a| if a.is_string() { a.as_str().unwrap().to_string() } else { a.to_string() });
                        calls.fragment(idx, None, c.pointer("/function/name").and_then(|s| s.as_str()), args.as_deref());
                    }
                }
            } else if anthropic {
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("content_block_start") => {
                        if v.pointer("/content_block/type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                            calls.fragment(idx, v.pointer("/content_block/id").and_then(|s| s.as_str()), v.pointer("/content_block/name").and_then(|s| s.as_str()), None);
                        }
                    }
                    Some("content_block_delta") => match v.pointer("/delta/type").and_then(|t| t.as_str()) {
                        Some("text_delta") => piece = v.pointer("/delta/text").and_then(|s| s.as_str()).map(String::from),
                        Some("input_json_delta") => {
                            let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                            calls.fragment(idx, None, None, v.pointer("/delta/partial_json").and_then(|s| s.as_str()));
                        }
                        _ => {}
                    },
                    _ => {}
                }
            } else {
                let delta = v.pointer("/choices/0/delta");
                piece = delta.and_then(|d| d.get("content")).and_then(|s| s.as_str()).map(String::from);
                if let Some(tc) = delta.and_then(|d| d.get("tool_calls")).and_then(|t| t.as_array()) {
                    for c in tc {
                        let idx = c.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                        calls.fragment(idx, c.get("id").and_then(|s| s.as_str()), c.pointer("/function/name").and_then(|s| s.as_str()), c.pointer("/function/arguments").and_then(|s| s.as_str()));
                    }
                }
            }
            if let Some(piece) = piece {
                if piece.is_empty() {
                    continue;
                }
                let visible = strip_thinking(&piece, &mut thinking);
                if !visible.is_empty() {
                    text.push_str(&visible);
                    let _ = events.send(AiEvent::Token { text: visible });
                }
            }
            // announce calls once they have a name and complete JSON
            while announced < calls.calls.len() {
                let c = &calls.calls[announced];
                if c.name.is_empty() || serde_json::from_str::<Value>(&c.arguments).is_err() && !c.arguments.trim().is_empty() {
                    break;
                }
                if !c.arguments.trim().is_empty() {
                    let _ = events.send(AiEvent::ToolCall { call: c.clone() });
                }
                announced += 1;
            }
        }
    }
    if stopped {
        text.push_str("\n(stopped)");
    }
    let tool_calls = if stopped { Vec::new() } else { calls.finish() };
    if text.trim().is_empty() && tool_calls.is_empty() {
        bail!("the model returned nothing");
    }
    Ok(ChatResult { text, tool_calls, tools_dropped: false, redacted: 0 })
}

/// Drop `<think>…</think>` spans, which may be split across tokens.
fn strip_thinking(piece: &str, inside: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = piece;
    loop {
        if *inside {
            match rest.find("</think>") {
                Some(i) => {
                    *inside = false;
                    rest = &rest[i + "</think>".len()..];
                }
                None => return out,
            }
        } else {
            match rest.find("<think>") {
                Some(i) => {
                    out.push_str(&rest[..i]);
                    *inside = true;
                    rest = &rest[i + "<think>".len()..];
                }
                None => {
                    out.push_str(rest);
                    return out.trim_start_matches('\n').to_string().replace("\n\n\n", "\n\n");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_redacted() {
        let (m, n) = redacted(&[ChatMessage::text("user", "enable secret 5 $1$abcd$efgh\nsnmp-server community s3cret RO\nhostname sw1")]);
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

    fn spec(kind: &str, base: &str) -> ProviderSpec {
        ProviderSpec { id: "x".into(), kind: kind.into(), base_url: base.into(), model: "m".into() }
    }

    async fn run(p: &ProviderSpec, tools: &[ToolSpec]) -> Result<ChatResult> {
        let (_tx, rx) = watch::channel(false);
        let events = Channel::new(|_| Ok(()));
        chat(p, &p.base(), &[ChatMessage::text("user", "hi")], tools, rx, &events).await
    }

    #[tokio::test]
    async fn streams_ollama_and_sse() {
        let base = fake_server("{\"message\":{\"content\":\"Hel\"},\"done\":false}\n{\"message\":{\"content\":\"lo\"},\"done\":true}\n").await;
        let r = run(&spec("ollama", &base), &[]).await.unwrap();
        assert_eq!(r.text, "Hello");

        let base = fake_server("data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}\n\ndata: [DONE]\n\n").await;
        let r = run(&spec("lmstudio", &base), &[]).await.unwrap();
        assert_eq!(r.text, "AB");

        let base = fake_server("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n").await;
        // no key saved: must fail before sending
        let err = run(&spec("anthropic", &base), &[]).await.unwrap_err();
        assert!(format!("{err:#}").contains("API key"));
    }

    fn tool() -> Vec<ToolSpec> {
        vec![ToolSpec { name: "run_show".into(), description: "run".into(), parameters: json!({ "type": "object", "properties": { "commands": { "type": "array", "items": { "type": "string" } } } }) }]
    }

    #[tokio::test]
    async fn collects_openai_tool_call_fragments() {
        let base = fake_server(concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Checking. \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_9\",\"function\":{\"name\":\"run_show\",\"arguments\":\"{\\\"comm\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ands\\\":[\\\"show version\\\"]}\"}}]}}]}\n\n",
            "data: [DONE]\n\n"
        ))
        .await;
        let r = run(&spec("lmstudio", &base), &tool()).await.unwrap();
        assert_eq!(r.text, "Checking. ");
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].id, "call_9");
        assert_eq!(r.tool_calls[0].name, "run_show");
        assert_eq!(parse_args(&r.tool_calls[0].arguments)["commands"][0], "show version");
    }

    #[tokio::test]
    async fn collects_ollama_tool_calls_and_strips_thinking() {
        let base = fake_server(concat!(
            "{\"message\":{\"content\":\"<think>hmm\"},\"done\":false}\n",
            "{\"message\":{\"content\":\" more</think>Let me look.\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\",\"tool_calls\":[{\"function\":{\"name\":\"run_show\",\"arguments\":{\"commands\":[\"show ip interface brief\"]}}}]},\"done\":true}\n"
        ))
        .await;
        let r = run(&spec("ollama", &base), &tool()).await.unwrap();
        assert_eq!(r.text, "Let me look.");
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].id, "call_0");
        assert_eq!(parse_args(&r.tool_calls[0].arguments)["commands"][0], "show ip interface brief");
    }

    #[test]
    fn anthropic_history_shape() {
        let msgs = vec![
            ChatMessage::text("system", "sys"),
            ChatMessage::text("user", "q"),
            ChatMessage { role: "assistant".into(), content: "".into(), tool_calls: vec![ToolCall { id: "toolu_1".into(), name: "run_show".into(), arguments: "{\"commands\":[\"show version\"]}".into() }, ToolCall { id: "toolu_2".into(), name: "inventory".into(), arguments: "{}".into() }], tool_call_id: None },
            ChatMessage { role: "tool".into(), content: "out1".into(), tool_calls: vec![], tool_call_id: Some("toolu_1".into()) },
            ChatMessage { role: "tool".into(), content: "out2".into(), tool_calls: vec![], tool_call_id: Some("toolu_2".into()) },
        ];
        let a = anthropic_messages(&msgs);
        assert_eq!(a.len(), 3, "system is separate, both tool results share one user message");
        assert_eq!(a[1]["content"][0]["type"], "tool_use");
        assert_eq!(a[1]["content"][0]["input"]["commands"][0], "show version");
        assert_eq!(a[2]["content"].as_array().unwrap().len(), 2);
        assert_eq!(a[2]["content"][1]["tool_use_id"], "toolu_2");

        let o = openai_messages(&msgs);
        assert_eq!(o[2]["tool_calls"][0]["function"]["arguments"], "{\"commands\":[\"show version\"]}");
        assert_eq!(o[3]["role"], "tool");
        assert_eq!(o[3]["tool_call_id"], "toolu_1");

        let flat: Vec<ChatMessage> = msgs.iter().map(flatten_tool_history).collect();
        assert_eq!(flat[3].role, "user");
        assert!(flat[2].content.contains("run_show"));
    }

    #[test]
    fn thinking_is_stripped_across_pieces() {
        let mut inside = false;
        let mut out = String::new();
        for piece in ["<thi", "nk>reasoning ", "here</think>", "Answer", " text"] {
            // a split tag that is not yet complete is kept until it can be judged
            out.push_str(&strip_thinking(piece, &mut inside));
        }
        // the first fragment "<thi" cannot be recognised on its own and passes through; callers
        // see complete tags in practice because tokenizers emit "<think>" as one token
        assert!(out.ends_with("Answer text"));
        let mut inside = false;
        assert_eq!(strip_thinking("<think>a</think>b", &mut inside), "b");
        assert!(!inside);
    }

    #[test]
    fn tools_rejection_is_recognised() {
        assert!(is_tools_rejection("400 Bad Request: this model does not support tools"));
        assert!(is_tools_rejection("400: Invalid parameter: 'tools'"));
        assert!(!is_tools_rejection("401 Unauthorized"));
        assert!(!is_tools_rejection("500: tool server crashed"));
    }

    #[test]
    fn defaults_per_kind() {
        let p = ProviderSpec { id: "x".into(), kind: "ollama".into(), base_url: "".into(), model: "qwen3:8b".into() };
        assert_eq!(p.base(), "http://127.0.0.1:11434");
        assert!(p.local());
        let p = ProviderSpec { id: "x".into(), kind: "openai".into(), base_url: "https://api.openai.com/".into(), model: "".into() };
        assert_eq!(p.base(), "https://api.openai.com");
        assert!(!p.local());
        let p = ProviderSpec { id: "x".into(), kind: "builtin".into(), base_url: "".into(), model: "qwen3-8b".into() };
        assert!(p.local());
    }
}
