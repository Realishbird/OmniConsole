//! Commands behind the Tools panel: run a command and capture it, detect the
//! OS, run health checks with progress, back up and diff configs, search
//! them, save bundles. Everything here only sends `show` commands and the
//! one `terminal length 0` style command that turns off paging.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::cisco::checks::{self, CheckResult, Outputs};
use crate::cisco::configs::{self, Backup, BundleFile, DiffResult, SearchHit};
use crate::cisco::detect::{self, Facts, Os};
use crate::redact;
use crate::session::SessionManager;
use crate::store;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/// One cancel flag per session, for the check run in progress.
#[derive(Default)]
pub struct ToolState {
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ToolState {
    pub fn start(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.cancels.lock().unwrap().insert(id.into(), flag.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        flag
    }
    pub fn finish(&self, id: &str) {
        self.cancels.lock().unwrap().remove(id);
    }
}

pub fn config_root_pub(app: &AppHandle) -> PathBuf {
    config_root(app)
}

/// settings.json "configDir", or Documents/OmniConsole/Configs
fn config_root(app: &AppHandle) -> PathBuf {
    store::config_dir(app)
        .ok()
        .and_then(|d| store::data_load(&d, "settings").ok())
        .and_then(|v| v.get("configDir").and_then(|p| p.as_str()).map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            app.path()
                .document_dir()
                .or_else(|_| app.path().home_dir())
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("OmniConsole")
                .join("Configs")
        })
}

const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);
const CONFIG_TIMEOUT: Duration = Duration::from_secs(300);

/*
 *  running commands
 */

/// Send one command and return its output (the terminal shows it too).
#[tauri::command]
pub async fn run_command(sessions: State<'_, SessionManager>, id: String, command: String, timeout_secs: Option<u64>) -> CmdResult<String> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(90).clamp(5, 900));
    sessions.run_command(&id, &command, timeout).await.map_err(err)
}

/// `show version`, then the shell probe when that came back from a Linux
/// shell (Cumulus, SONiC). Nothing here changes the device.
async fn detect_facts(sessions: &SessionManager, id: &str) -> CmdResult<Facts> {
    let text = sessions.run_command(id, "show version", COMMAND_TIMEOUT).await.map_err(err)?;
    let mut facts = detect::parse_facts(&text);
    if facts.os.unwrap_or(Os::Unknown) == Os::Unknown && detect::looks_like_shell(&text) {
        if let Ok(probe) = sessions.run_command(id, detect::SHELL_PROBE, COMMAND_TIMEOUT).await {
            let f = detect::parse_facts(&probe);
            if f.os.unwrap_or(Os::Unknown) != Os::Unknown {
                facts = f;
                if facts.os == Some(Os::Cumulus) {
                    if let Ok(sys) = sessions.run_command(id, "nv show system", COMMAND_TIMEOUT).await {
                        let extra = detect::parse_facts(&format!("Cumulus Linux\n{sys}"));
                        if facts.hostname.is_empty() { facts.hostname = extra.hostname; }
                        if facts.uptime.is_empty() { facts.uptime = extra.uptime; }
                        if facts.model.is_empty() { facts.model = extra.model; }
                    }
                }
            }
        }
    }
    Ok(facts)
}

/// `show version`, then turn the pager off for the session.
#[tauri::command]
pub async fn device_detect(sessions: State<'_, SessionManager>, id: String) -> CmdResult<Facts> {
    let facts = detect_facts(&sessions, &id).await?;
    if let Some(cmd) = facts.os.unwrap_or(Os::Unknown).no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }
    Ok(facts)
}

/*
 *  health checks
 */

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CheckProgress {
    /// about to run a command
    Command { command: String, index: usize, total: usize },
    /// a command finished (or failed)
    Output { command: String, ok: bool, bytes: usize },
    Result { result: CheckResult },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub os: Os,
    pub facts: Facts,
    pub started: i64,
    pub finished: i64,
    pub cancelled: bool,
    pub results: Vec<CheckResult>,
    /// raw command output, for the bundle and the "show evidence" view
    pub outputs: Vec<(String, Result<String, String>)>,
}

/// Run the selected checks (all when `ids` is empty). Commands run one at a
/// time on the session; progress goes out on `events`.
#[tauri::command]
pub async fn checks_run(
    sessions: State<'_, SessionManager>,
    tools: State<'_, ToolState>,
    id: String,
    os: Option<Os>,
    facts: Option<Facts>,
    ids: Vec<String>,
    tier: Option<checks::Tier>,
    events: Channel<CheckProgress>,
) -> CmdResult<CheckReport> {
    let cancel = tools.start(&id);
    let started = chrono::Utc::now().timestamp();
    // detect first when the UI has not
    let facts = match facts {
        Some(f) => f,
        None => {
            let f = detect_facts(&sessions, &id).await?;
            if let Some(cmd) = f.os.unwrap_or(Os::Unknown).no_paging() {
                let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
            }
            f
        }
    };
    let os = os.or(facts.os).unwrap_or(Os::Unknown);
    let os = if os == Os::Unknown { Os::Ios } else { os };
    let commands = checks::commands_for(os, &ids, tier);
    let total = commands.len();
    let mut outputs: Outputs = Outputs::new();
    let mut cancelled = false;
    for (index, command) in commands.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let _ = events.send(CheckProgress::Command { command: command.clone(), index, total });
        let result = sessions.run_command(&id, command, COMMAND_TIMEOUT).await.map_err(|e| format!("{e:#}"));
        let _ = events.send(CheckProgress::Output { command: command.clone(), ok: result.is_ok(), bytes: result.as_ref().map(|s| s.len()).unwrap_or(0) });
        if let Err(e) = &result {
            if e.contains("session is closed") {
                tools.finish(&id);
                return Err(e.clone());
            }
        }
        outputs.insert(command.clone(), result);
    }
    tools.finish(&id);
    let results = checks::evaluate(os, &ids, tier, &outputs);
    for r in &results {
        let _ = events.send(CheckProgress::Result { result: r.clone() });
    }
    Ok(CheckReport {
        os,
        facts,
        started,
        finished: chrono::Utc::now().timestamp(),
        cancelled,
        results,
        outputs: outputs.into_iter().collect(),
    })
}

#[tauri::command]
pub fn checks_cancel(tools: State<'_, ToolState>, id: String) {
    if let Some(flag) = tools.cancels.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// The check catalog, so the UI can offer a selection.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckInfo {
    pub id: String,
    pub title: String,
    pub category: String,
    pub quick: bool,
    pub commands: Vec<String>,
}

#[tauri::command]
pub fn checks_catalog(os: Option<Os>) -> Vec<CheckInfo> {
    let os = os.unwrap_or(Os::Ios);
    checks::catalog()
        .iter()
        .map(|d| CheckInfo { id: d.id.into(), title: d.title.into(), category: d.category.into(), quick: d.quick, commands: checks::commands_of(d, os).iter().map(|s| s.to_string()).collect() })
        .collect()
}

/*
 *  config backups
 */

/// Save running and/or startup config of the session's device.
#[tauri::command]
pub async fn config_backup(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    id: String,
    os: Option<Os>,
    site: String,
    device: String,
    kinds: Vec<String>,
) -> CmdResult<Vec<Backup>> {
    let root = config_root(&app);
    let os = os.unwrap_or(Os::Ios);
    if let Some(cmd) = os.no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }
    let mut out = Vec::new();
    for kind in kinds {
        // the platform profile says what each kind is called; "running" and
        // "startup" fall back to the profile's first (live) and second entries
        let profile = crate::platforms::profile(os);
        let picked = profile.configs.iter().find(|c| c.kind == kind)
            .or_else(|| if kind == "running" { profile.configs.first() } else if kind == "startup" { profile.configs.get(1) } else { None })
            .ok_or_else(|| format!("{} has no {kind} configuration", os.label()))?;
        let command = picked.command.as_str();
        let text = sessions.run_command(&id, command, CONFIG_TIMEOUT).await.map_err(err)?;
        if text.trim().is_empty() {
            return Err(format!("`{command}` returned nothing"));
        }
        out.push(configs::save(&root, &site, &device, &kind, &text).map_err(err)?);
    }
    Ok(out)
}

/// Save a check report next to the device's configs (a baseline).
#[tauri::command]
pub fn report_save(app: AppHandle, site: String, device: String, report: serde_json::Value) -> CmdResult<Backup> {
    let text = serde_json::to_string_pretty(&report).map_err(err)?;
    configs::save(&config_root(&app), &site, &device, "checks", &text).map_err(err)
}

#[tauri::command]
pub fn configs_list(app: AppHandle, device: Option<String>) -> Vec<Backup> {
    configs::list(&config_root(&app), device.as_deref())
}

#[tauri::command]
pub fn config_read(app: AppHandle, path: String) -> CmdResult<String> {
    configs::read(&config_root(&app), &path).map_err(err)
}

#[tauri::command]
pub fn config_delete(app: AppHandle, path: String) -> CmdResult<()> {
    configs::delete(&config_root(&app), &path).map_err(err)
}

/// Diff two texts (the UI reads backups with config_read first).
#[tauri::command]
pub fn config_diff(old: String, new: String, ignore_volatile: bool) -> DiffResult {
    configs::diff(&old, &new, ignore_volatile)
}

#[tauri::command]
pub fn configs_search(app: AppHandle, query: String, regex: bool, all_versions: bool) -> CmdResult<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    configs::search(&config_root(&app), &query, regex, all_versions).map_err(err)
}

#[tauri::command]
pub fn config_folder(app: AppHandle) -> String {
    config_root(&app).display().to_string()
}

#[tauri::command]
pub fn open_config_folder(app: AppHandle) -> CmdResult<()> {
    let dir = config_root(&app);
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}

#[tauri::command]
pub fn reveal_config(app: AppHandle, path: String) -> CmdResult<()> {
    let p = configs::inside(&config_root(&app), &path).map_err(err)?;
    app.opener().reveal_item_in_dir(p).map_err(err)
}

/*
 *  bundles
 */

/// Zip the given files to a path the user picks. Secrets are masked when
/// asked. Returns the path, or None when cancelled.
#[tauri::command]
pub async fn save_bundle(app: AppHandle, name: String, files: Vec<BundleFile>, mask_secrets: bool) -> CmdResult<Option<String>> {
    let Some(path) = crate::commands::pick_save(&app, &name, ("Zip archive", &["zip"])).await else {
        return Ok(None);
    };
    let files: Vec<BundleFile> = files
        .into_iter()
        .map(|f| BundleFile { name: f.name, contents: if mask_secrets { redact::text(&f.contents) } else { f.contents } })
        .collect();
    configs::write_bundle(&path, &files).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

/// What each proposed command would do, so the UI can gate it.
#[tauri::command]
pub fn classify_commands(commands: Vec<String>) -> Vec<crate::cisco::classify::Classified> {
    crate::cisco::classify::classify(&commands)
}

#[derive(Debug, Clone, Serialize)]
pub struct Completion {
    pub word: String,
    pub help: String,
}

/// Ask the device what can follow `partial` with its own `?` help. The
/// typed text is restored afterwards (Ctrl-U clears the line, then the
/// partial is typed again), so the terminal shows a brief flicker only.
#[tauri::command]
pub async fn complete(sessions: State<'_, SessionManager>, id: String, partial: String) -> CmdResult<Vec<Completion>> {
    let partial = partial.trim_start().to_string();
    if partial.contains(['\r', '\n', '?']) {
        return Err("nothing to complete".into());
    }
    // the text already typed is on the device's line; send only the ?
    let raw = sessions.probe(&id, b"?", Duration::from_millis(400), Duration::from_secs(4)).await.map_err(err)?;
    // clear the re-echoed line and type the partial again
    let mut restore = vec![0x15u8];
    restore.extend_from_slice(partial.as_bytes());
    sessions.write(&id, restore);
    let text = crate::session::runner::strip_controls(&raw);
    let mut out = Vec::new();
    let last_word_partial = !partial.is_empty() && !partial.ends_with(' ');
    let prefix = if last_word_partial { partial.rsplit(' ').next().unwrap_or("").to_lowercase() } else { String::new() };
    for line in text.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() || l.contains('%') || l.trim_start().starts_with('^') || crate::session::runner::prompt_regex().is_match(l) {
            continue;
        }
        // "  interface    Select an interface to configure" or just "  WORD"
        let trimmed = l.trim_start();
        let (word, help) = match trimmed.find("  ") {
            Some(pos) => (&trimmed[..pos], trimmed[pos..].trim()),
            None => (trimmed, ""),
        };
        let word = word.trim();
        if word.is_empty() || word.contains(' ') && help.is_empty() && word.len() > 24 {
            continue;
        }
        if last_word_partial && !word.to_lowercase().starts_with(&prefix) && word != "<cr>" {
            continue;
        }
        out.push(Completion { word: word.to_string(), help: help.to_string() });
    }
    Ok(out)
}
