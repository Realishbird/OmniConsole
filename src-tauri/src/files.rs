//! File transfer: an SFTP browser for devices that have the subsystem
//! (IOS-XE, NX-OS, Linux) and SCP for the ones that do not (classic IOS
//! with `ip scp server enable`). Both ride a second SSH connection made
//! with the same login as the terminal tab.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

use crate::session::ssh::{self, Connected};
use crate::session::{ConnectSpec, SessionEvent, SessionManager};
use crate::store;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

struct Conn {
    ssh: Connected,
    sftp: Option<SftpSession>,
}

#[derive(Default)]
pub struct FilesState {
    conns: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Conn>>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub id: String,
    pub sftp: bool,
    pub home: String,
    pub user: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub permissions: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Transfer {
    Progress { done: u64, total: u64 },
    Done { path: String, bytes: u64 },
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') || dir.ends_with(':') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

/// Open the second connection. Prompts (host key, password) arrive on
/// `events` under `id` (chosen by the caller, so it can answer them with
/// session_answer before this returns).
#[tauri::command]
pub async fn files_open(app: AppHandle, sessions: State<'_, SessionManager>, files: State<'_, FilesState>, id: String, spec: ConnectSpec, events: Channel<SessionEvent>) -> CmdResult<Opened> {
    if spec.protocol != "ssh" {
        return Err("file transfer needs an SSH device".into());
    }
    if id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("bad id".into());
    }
    let config_dir = store::config_dir(&app).map_err(err)?;
    let io = sessions.register_aux(id.clone(), config_dir, events);
    let connected = match ssh::connect(&spec, &io).await {
        Ok(c) => c,
        Err(e) => {
            sessions.close(&id);
            return Err(format!("{e:#}"));
        }
    };
    // SFTP when the server has the subsystem, SCP otherwise
    let sftp = match connected.session.channel_open_session().await {
        Ok(ch) => match ch.request_subsystem(true, "sftp").await {
            Ok(()) => match tokio::time::timeout(std::time::Duration::from_secs(10), SftpSession::new(ch.into_stream())).await {
                Ok(Ok(s)) => Some(s),
                _ => None,
            },
            Err(_) => None,
        },
        Err(_) => None,
    };
    let home = match &sftp {
        Some(s) => s.canonicalize(".").await.unwrap_or_else(|_| "/".into()),
        None => String::new(),
    };
    let user = connected.user.clone();
    let has_sftp = sftp.is_some();
    files.conns.lock().unwrap().insert(id.clone(), Arc::new(tokio::sync::Mutex::new(Conn { ssh: connected, sftp })));
    io.status("connected", format!("{} {}", if has_sftp { "SFTP" } else { "SCP" }, spec.host));
    Ok(Opened { id, sftp: has_sftp, home, user })
}

fn conn(files: &FilesState, id: &str) -> CmdResult<Arc<tokio::sync::Mutex<Conn>>> {
    files.conns.lock().unwrap().get(id).cloned().ok_or_else(|| "the file connection is closed".to_string())
}

#[tauri::command]
pub async fn files_close(sessions: State<'_, SessionManager>, files: State<'_, FilesState>, id: String) -> CmdResult<()> {
    if let Some(c) = files.conns.lock().unwrap().remove(&id) {
        tokio::spawn(async move {
            let c = c.lock().await;
            if let Some(s) = &c.sftp {
                let _ = s.close().await;
            }
            let _ = c.ssh.session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
        });
    }
    sessions.close(&id);
    Ok(())
}

#[tauri::command]
pub async fn files_list(files: State<'_, FilesState>, id: String, path: String) -> CmdResult<Vec<Entry>> {
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    let sftp = c.sftp.as_ref().ok_or("no SFTP on this device; use SCP with a path")?;
    let dir = sftp.canonicalize(if path.is_empty() { "." } else { path.as_str() }).await.map_err(err)?;
    let mut out = Vec::new();
    for e in sftp.read_dir(&dir).await.map_err(err)? {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let m = e.metadata();
        out.push(Entry {
            path: join(&dir, &name),
            is_dir: m.is_dir(),
            size: m.size.unwrap_or(0),
            modified: m.mtime.map(|t| t as i64),
            permissions: m.permissions.map(|p| format!("{:o}", p & 0o7777)).unwrap_or_default(),
            name,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
pub async fn files_mkdir(files: State<'_, FilesState>, id: String, path: String) -> CmdResult<()> {
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    c.sftp.as_ref().ok_or("no SFTP")?.create_dir(path).await.map_err(err)
}

#[tauri::command]
pub async fn files_delete(files: State<'_, FilesState>, id: String, path: String, is_dir: bool) -> CmdResult<()> {
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    let s = c.sftp.as_ref().ok_or("no SFTP")?;
    if is_dir { s.remove_dir(path).await.map_err(err) } else { s.remove_file(path).await.map_err(err) }
}

#[tauri::command]
pub async fn files_rename(files: State<'_, FilesState>, id: String, from: String, to: String) -> CmdResult<()> {
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    c.sftp.as_ref().ok_or("no SFTP")?.rename(from, to).await.map_err(err)
}

async fn pick_open(app: &AppHandle) -> Option<PathBuf> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_file(move |p| {
        let _ = tx.send(p);
    });
    rx.await.ok().flatten().and_then(|p| p.into_path().ok())
}

/// Download `remote` to a file the user picks. SFTP when available, SCP otherwise.
#[tauri::command]
pub async fn files_download(app: AppHandle, files: State<'_, FilesState>, id: String, remote: String, events: Channel<Transfer>) -> CmdResult<Option<String>> {
    let name = remote.rsplit(['/', ':']).next().unwrap_or("file").to_string();
    let Some(local) = crate::commands::pick_save(&app, &name, ("All files", &["*"])).await else {
        return Ok(None);
    };
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    let mut out = tokio::fs::File::create(&local).await.map_err(err)?;
    let bytes = match &c.sftp {
        Some(s) => {
            let total = s.metadata(&remote).await.ok().and_then(|m| m.size).unwrap_or(0);
            let mut f = s.open(&remote).await.map_err(err)?;
            copy_with_progress(&mut f, &mut out, total, &events).await.map_err(err)?
        }
        None => scp_download(&c.ssh, &remote, &mut out, &events).await.map_err(err)?,
    };
    out.flush().await.map_err(err)?;
    let _ = events.send(Transfer::Done { path: local.display().to_string(), bytes });
    Ok(Some(local.display().to_string()))
}

/// Upload a file the user picks into `remote_dir` (SFTP) or to the exact
/// `remote_dir` path (SCP, e.g. `flash:/image.bin`).
#[tauri::command]
pub async fn files_upload(app: AppHandle, files: State<'_, FilesState>, id: String, remote_dir: String, events: Channel<Transfer>) -> CmdResult<Option<String>> {
    let Some(local) = pick_open(&app).await else {
        return Ok(None);
    };
    let name = local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let total = tokio::fs::metadata(&local).await.map_err(err)?.len();
    let mut input = tokio::fs::File::open(&local).await.map_err(err)?;
    let c = conn(&files, &id)?;
    let c = c.lock().await;
    let remote = match &c.sftp {
        Some(s) => {
            let remote = join(&remote_dir, &name);
            let mut f = s.create(&remote).await.map_err(err)?;
            copy_with_progress(&mut input, &mut f, total, &events).await.map_err(err)?;
            f.shutdown().await.map_err(err)?;
            remote
        }
        None => {
            // SCP: remote_dir is the destination path, or a directory to put the file in
            let remote = if remote_dir.ends_with('/') || remote_dir.ends_with(':') || remote_dir.is_empty() { join(&remote_dir, &name) } else { remote_dir.clone() };
            scp_upload(&c.ssh, &remote, &name, &mut input, total, &events).await.map_err(err)?;
            remote
        }
    };
    let _ = events.send(Transfer::Done { path: remote.clone(), bytes: total });
    Ok(Some(remote))
}

async fn copy_with_progress<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(r: &mut R, w: &mut W, total: u64, events: &Channel<Transfer>) -> Result<u64> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).await?;
        done += n as u64;
        if last.elapsed().as_millis() > 150 {
            let _ = events.send(Transfer::Progress { done, total });
            last = std::time::Instant::now();
        }
    }
    let _ = events.send(Transfer::Progress { done, total });
    Ok(done)
}

/* ------------------------------------------------------------------ scp */

/// Minimal SCP source protocol: `scp -f path` on the device.
/// SCP runs through the device's shell, so the path must be a plain path.
fn scp_path(remote: &str) -> Result<&str> {
    let r = remote.trim();
    if r.is_empty() || r.starts_with('-') || r.chars().any(|c| c.is_whitespace() || "|;&$`()<>'\"\\".contains(c) || c.is_control()) {
        anyhow::bail!("the remote path contains characters SCP cannot carry safely (spaces, quotes, ; | & $ ` ( ) < >): use SFTP or rename the file");
    }
    Ok(r)
}

async fn scp_download<W: tokio::io::AsyncWrite + Unpin>(ssh: &Connected, remote: &str, out: &mut W, events: &Channel<Transfer>) -> Result<u64> {
    let remote = scp_path(remote)?;
    let mut ch = ssh.session.channel_open_session().await?;
    ch.exec(true, format!("scp -f {remote}")).await?;
    ch.data(&b"\0"[..]).await?;
    let mut pending: Vec<u8> = Vec::new();
    // read the C header
    let header = loop {
        match ch.wait().await {
            Some(ChannelMsg::Data { data }) => {
                pending.extend_from_slice(&data);
                if let Some(pos) = pending.iter().position(|b| *b == b'\n') {
                    let line = String::from_utf8_lossy(&pending[..pos]).to_string();
                    pending.drain(..=pos);
                    break line;
                }
            }
            Some(ChannelMsg::ExtendedData { data, .. }) => bail!("{}", String::from_utf8_lossy(&data).trim()),
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => bail!("the device closed the SCP session (is `ip scp server enable` set, and the path right?)"),
            _ => {}
        }
    };
    if header.starts_with('\x01') || header.starts_with('\x02') {
        bail!("{}", header.trim_start_matches(['\x01', '\x02']).trim());
    }
    let mut parts = header.split(' ');
    let mode = parts.next().unwrap_or("");
    if !mode.starts_with('C') {
        bail!("unexpected SCP header: {header}");
    }
    let total: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow!("bad SCP header: {header}"))?;
    ch.data(&b"\0"[..]).await?;
    let mut done = 0u64;
    let mut last = std::time::Instant::now();
    // the body may have started arriving with the header
    let take = pending.len().min(total as usize);
    out.write_all(&pending[..take]).await?;
    done += take as u64;
    while done < total {
        match ch.wait().await {
            Some(ChannelMsg::Data { data }) => {
                let take = (data.len() as u64).min(total - done) as usize;
                out.write_all(&data[..take]).await?;
                done += take as u64;
                if last.elapsed().as_millis() > 150 {
                    let _ = events.send(Transfer::Progress { done, total });
                    last = std::time::Instant::now();
                }
            }
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => bail!("transfer ended early at {done} of {total} bytes"),
            _ => {}
        }
    }
    ch.data(&b"\0"[..]).await?;
    let _ = ch.eof().await;
    let _ = events.send(Transfer::Progress { done, total });
    Ok(done)
}

async fn wait_ack(ch: &mut russh::Channel<russh::client::Msg>) -> Result<()> {
    loop {
        match ch.wait().await {
            Some(ChannelMsg::Data { data }) => {
                if data.first() == Some(&0) {
                    return Ok(());
                }
                bail!("{}", String::from_utf8_lossy(&data[1.min(data.len())..]).trim());
            }
            Some(ChannelMsg::ExtendedData { data, .. }) => bail!("{}", String::from_utf8_lossy(&data).trim()),
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => bail!("the device closed the SCP session (is `ip scp server enable` set?)"),
            _ => {}
        }
    }
}

/// Minimal SCP sink protocol: `scp -t path` on the device.
async fn scp_upload<R: tokio::io::AsyncRead + Unpin>(ssh: &Connected, remote: &str, name: &str, input: &mut R, total: u64, events: &Channel<Transfer>) -> Result<()> {
    let remote = scp_path(remote)?;
    let mut ch = ssh.session.channel_open_session().await?;
    ch.exec(true, format!("scp -t {remote}")).await?;
    wait_ack(&mut ch).await.context("SCP start")?;
    ch.data(format!("C0644 {total} {name}\n").as_bytes()).await?;
    wait_ack(&mut ch).await.context("SCP header")?;
    let mut buf = vec![0u8; 32 * 1024];
    let mut done = 0u64;
    let mut last = std::time::Instant::now();
    loop {
        let n = input.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        ch.data(&buf[..n]).await?;
        done += n as u64;
        if last.elapsed().as_millis() > 150 {
            let _ = events.send(Transfer::Progress { done, total });
            last = std::time::Instant::now();
        }
    }
    ch.data(&b"\0"[..]).await?;
    wait_ack(&mut ch).await.context("SCP finish")?;
    let _ = ch.eof().await;
    let _ = events.send(Transfer::Progress { done, total });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::join;

    #[test]
    fn joins_unix_and_ios_paths() {
        assert_eq!(join("/home/x", "a.txt"), "/home/x/a.txt");
        assert_eq!(join("/", "a.txt"), "/a.txt");
        assert_eq!(join("flash:", "img.bin"), "flash:img.bin");
        assert_eq!(join("flash:/", "img.bin"), "flash:/img.bin");
    }
}
