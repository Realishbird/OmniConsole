//! Built-in model runtime. llama.cpp's server ships inside the app as a
//! resource (`llama/`, fetched by CI from the llama.cpp release that the
//! build used); GGUF models are downloaded on demand from Hugging Face into
//! the app data folder, verified against the sha256 the Hub publishes, and
//! served on a loopback port that only this app knows. Nothing here talks to
//! a device, and model files never leave the computer.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::watch;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/* ------------------------------------------------------------ catalog */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    /// approximate download size, until the Hub answers with the exact one
    pub size_bytes: u64,
    /// GPU memory (or Apple unified memory) the model needs to run well
    pub needs_gb: f32,
    pub blurb: &'static str,
}

const GB: u64 = 1_000_000_000;

/// Models the picker offers. Any other GGUF file can be added by hand.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "qwen3-4b",
        name: "Qwen3 4B",
        repo: "Qwen/Qwen3-4B-GGUF",
        file: "Qwen3-4B-Q4_K_M.gguf",
        size_bytes: 2_500_000_000,
        needs_gb: 4.0,
        blurb: "Runs without a GPU. Good at explaining output; weaker at multi-step troubleshooting.",
    },
    CatalogEntry {
        id: "qwen3-8b",
        name: "Qwen3 8B",
        repo: "Qwen/Qwen3-8B-GGUF",
        file: "Qwen3-8B-Q4_K_M.gguf",
        size_bytes: 5_030_000_000,
        needs_gb: 8.0,
        blurb: "The balanced choice for 8 GB GPUs: solid at reading show output and picking the next command.",
    },
    CatalogEntry {
        id: "qwen3-14b",
        name: "Qwen3 14B",
        repo: "Qwen/Qwen3-14B-GGUF",
        file: "Qwen3-14B-Q4_K_M.gguf",
        size_bytes: 9_000_000_000,
        needs_gb: 12.0,
        blurb: "Noticeably better at finding a cause across long logs and several commands.",
    },
    CatalogEntry {
        id: "qwen3-30b-a3b",
        name: "Qwen3 30B-A3B",
        repo: "Qwen/Qwen3-30B-A3B-GGUF",
        file: "Qwen3-30B-A3B-Q4_K_M.gguf",
        size_bytes: 18_600_000_000,
        needs_gb: 24.0,
        blurb: "The best local pick. Mixture of experts, so it answers fast for its size on 24 GB cards and 32 GB Macs.",
    },
];

/* ----------------------------------------------------------- hardware */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub ram_gb: f32,
    pub gpu_name: String,
    /// dedicated GPU memory; 0 when unknown or absent
    pub vram_gb: f32,
    pub apple_silicon: bool,
    /// one line for the UI
    pub summary: String,
}

async fn run_quiet(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_window(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(not(windows))]
    let _ = cmd;
}

/// What this computer can run: total memory, and the GPU when one is found.
pub async fn probe_hardware() -> Hardware {
    let mut hw = Hardware::default();
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = fs::read_to_string("/proc/meminfo") {
            if let Some(kb) = text.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.trim().split_whitespace().next()).and_then(|v| v.parse::<f64>().ok()) {
                hw.ram_gb = (kb * 1024.0 / 1e9) as f32;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(bytes) = run_quiet("sysctl", &["-n", "hw.memsize"]).await.and_then(|s| s.trim().parse::<f64>().ok()) {
            hw.ram_gb = (bytes / 1e9) as f32;
        }
        let brand = run_quiet("sysctl", &["-n", "machdep.cpu.brand_string"]).await.unwrap_or_default();
        if brand.contains("Apple") {
            hw.apple_silicon = true;
            hw.gpu_name = format!("{} (unified memory)", brand.trim());
            hw.vram_gb = hw.ram_gb;
        }
    }
    #[cfg(windows)]
    {
        if let Some(text) = run_quiet("powershell", &["-NoProfile", "-NonInteractive", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"]).await {
            if let Ok(bytes) = text.trim().parse::<f64>() {
                hw.ram_gb = (bytes / 1e9) as f32;
            }
        }
    }
    if !hw.apple_silicon {
        // NVIDIA reports exactly; for other vendors the name is enough to say a GPU exists
        if let Some(text) = run_quiet("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).await {
            if let Some(line) = text.lines().find(|l| l.contains(',')) {
                let (name, mem) = line.rsplit_once(',').unwrap();
                hw.gpu_name = name.trim().to_string();
                hw.vram_gb = mem.trim().parse::<f32>().map(|mib| mib * 1.048_576 / 1000.0).unwrap_or(0.0);
            }
        }
        #[cfg(windows)]
        if hw.gpu_name.is_empty() {
            if let Some(text) = run_quiet("powershell", &["-NoProfile", "-NonInteractive", "-Command", "(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join ';'"]).await {
                let names: Vec<&str> = text.trim().split(';').map(str::trim).filter(|n| !n.is_empty() && !n.contains("Microsoft Basic")).collect();
                if let Some(n) = names.iter().find(|n| n.contains("NVIDIA") || n.contains("AMD") || n.contains("Radeon") || n.contains("Arc")).or(names.first()) {
                    hw.gpu_name = n.to_string();
                }
            }
        }
        #[cfg(target_os = "linux")]
        if hw.gpu_name.is_empty() {
            if let Some(text) = run_quiet("sh", &["-c", "lspci 2>/dev/null | grep -iE 'vga|3d|display' | head -1"]).await {
                if let Some((_, name)) = text.trim().split_once(": ") {
                    hw.gpu_name = name.trim().to_string();
                }
            }
        }
    }
    hw.summary = describe(&hw);
    hw
}

fn describe(hw: &Hardware) -> String {
    let ram = if hw.ram_gb > 0.0 { format!("{:.0} GB memory", hw.ram_gb) } else { "memory size unknown".to_string() };
    if hw.apple_silicon {
        format!("{} · {ram} shared with the GPU", hw.gpu_name)
    } else if hw.vram_gb > 0.0 {
        format!("{} with {:.0} GB · {ram}", hw.gpu_name, hw.vram_gb)
    } else if !hw.gpu_name.is_empty() {
        format!("{} (memory size unknown) · {ram}", hw.gpu_name)
    } else {
        format!("No GPU found · {ram}")
    }
}

/// The catalog model that fits the hardware best.
pub fn recommend(hw: &Hardware) -> &'static str {
    let gpu_mem = if hw.apple_silicon { hw.ram_gb * 0.7 } else { hw.vram_gb };
    if gpu_mem >= 22.0 {
        "qwen3-30b-a3b"
    } else if gpu_mem >= 12.0 {
        "qwen3-14b"
    } else if gpu_mem >= 7.0 {
        "qwen3-8b"
    } else if gpu_mem > 0.0 || !hw.gpu_name.is_empty() {
        "qwen3-4b"
    } else if hw.ram_gb >= 24.0 {
        // CPU only: 8B answers slowly but noticeably better
        "qwen3-8b"
    } else {
        "qwen3-4b"
    }
}

/* ------------------------------------------------------------ folders */

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_local_data_dir().or_else(|_| app.path().app_data_dir()).context("no data folder")?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    let d = data_dir(app)?.join("models");
    fs::create_dir_all(&d)?;
    Ok(d)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomModel {
    id: String,
    name: String,
    path: String,
}

fn custom_file(app: &AppHandle) -> Result<PathBuf> {
    Ok(models_dir(app)?.join("custom.json"))
}

fn load_custom(app: &AppHandle) -> Vec<CustomModel> {
    custom_file(app).ok().and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_custom(app: &AppHandle, list: &[CustomModel]) -> Result<()> {
    crate::store::write_atomic(&custom_file(app)?, &serde_json::to_string_pretty(list)?)
}

/* ------------------------------------------------------------ runtime */

fn server_name() -> &'static str {
    if cfg!(windows) { "llama-server.exe" } else { "llama-server" }
}

/// The bundled runtime: where it is and which llama.cpp release it came from.
fn bundled_runtime(app: &AppHandle) -> Option<(PathBuf, String)> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("llama"));
    }
    // `cargo tauri dev` runs from src-tauri; the CI download lands in binaries/llama
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("binaries").join("llama"));
        candidates.push(cwd.join("src-tauri").join("binaries").join("llama"));
    }
    for dir in candidates {
        if dir.join(server_name()).is_file() {
            let version = fs::read_to_string(dir.join("VERSION")).map(|v| v.trim().to_string()).unwrap_or_else(|_| "unknown".into());
            return Some((dir, version));
        }
    }
    None
}

/// Copy the bundled runtime into the data folder once per version: the install
/// folder may be read-only, and fresh copies carry no download quarantine.
fn prepared_runtime(app: &AppHandle) -> Result<(PathBuf, String)> {
    let (src, version) = bundled_runtime(app).ok_or_else(|| anyhow!("this build has no built-in runtime for this platform; use Ollama or LM Studio, or a cloud provider"))?;
    let dest = data_dir(app)?.join("runtime").join(sanitize(&version));
    let marker = dest.join(".ready");
    if !marker.exists() {
        if dest.exists() {
            let _ = fs::remove_dir_all(&dest);
        }
        fs::create_dir_all(&dest)?;
        copy_files(&src, &dest)?;
        // the CPU build, when CI added one, is the fallback for a broken GPU driver
        if src.join("cpu").join(server_name()).is_file() {
            fs::create_dir_all(dest.join("cpu"))?;
            copy_files(&src.join("cpu"), &dest.join("cpu"))?;
        }
        fs::write(&marker, "ok")?;
    }
    Ok((dest, version))
}

fn copy_files(src: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let to = dest.join(entry.file_name());
            fs::copy(entry.path(), &to).with_context(|| format!("copying {}", entry.path().display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&to, fs::Permissions::from_mode(0o755));
            }
        }
    }
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect()
}

/* -------------------------------------------------------------- state */

struct Running {
    model_id: String,
    port: u16,
    child: Child,
    /// last lines of the server's log, for error messages
    log: Arc<Mutex<Vec<String>>>,
}

#[derive(Default)]
pub struct Engine {
    running: tokio::sync::Mutex<Option<Running>>,
    hardware: Mutex<Option<Hardware>>,
    downloads: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl Engine {
    /// Kill the server when the app exits (also happens on drop, but exit is not a drop).
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.running.try_lock() {
            if let Some(mut r) = guard.take() {
                let _ = r.child.start_kill();
            }
        }
    }
}

/// The hardware probe runs once per app start.
async fn cached_hardware(engine: &Engine) -> Hardware {
    let known = engine.hardware.lock().unwrap().clone();
    match known {
        Some(h) => h,
        None => {
            let h = probe_hardware().await;
            *engine.hardware.lock().unwrap() = Some(h.clone());
            h
        }
    }
}

/* ------------------------------------------------------------- status */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub file: String,
    pub size_bytes: u64,
    pub needs_gb: f32,
    pub blurb: String,
    pub downloaded: bool,
    pub bytes_on_disk: u64,
    pub custom: bool,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub available: bool,
    pub version: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub running: bool,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub runtime: RuntimeInfo,
    pub hardware: Hardware,
    pub recommended: String,
    pub models: Vec<LocalModel>,
    pub engine: EngineInfo,
    pub models_dir: String,
}

fn model_path(app: &AppHandle, id: &str) -> Result<PathBuf> {
    if let Some(e) = CATALOG.iter().find(|e| e.id == id) {
        return Ok(models_dir(app)?.join(e.file));
    }
    if let Some(c) = load_custom(app).into_iter().find(|c| c.id == id) {
        return Ok(PathBuf::from(c.path));
    }
    bail!("unknown model {id}")
}

fn model_name(app: &AppHandle, id: &str) -> String {
    CATALOG.iter().find(|e| e.id == id).map(|e| e.name.to_string()).or_else(|| load_custom(app).into_iter().find(|c| c.id == id).map(|c| c.name)).unwrap_or_else(|| id.to_string())
}

fn list_models(app: &AppHandle) -> Result<Vec<LocalModel>> {
    let dir = models_dir(app)?;
    let mut out: Vec<LocalModel> = CATALOG
        .iter()
        .map(|e| {
            let p = dir.join(e.file);
            let meta = fs::metadata(&p).ok();
            LocalModel {
                id: e.id.into(),
                name: e.name.into(),
                repo: e.repo.into(),
                file: e.file.into(),
                size_bytes: e.size_bytes,
                needs_gb: e.needs_gb,
                blurb: e.blurb.into(),
                downloaded: meta.is_some(),
                bytes_on_disk: meta.map(|m| m.len()).unwrap_or(0),
                custom: false,
                path: p.display().to_string(),
            }
        })
        .collect();
    for c in load_custom(app) {
        let meta = fs::metadata(&c.path).ok();
        out.push(LocalModel {
            id: c.id,
            name: c.name,
            repo: String::new(),
            file: Path::new(&c.path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            needs_gb: meta.as_ref().map(|m| (m.len() as f32 / GB as f32 * 1.25).ceil()).unwrap_or(0.0),
            blurb: "Your own GGUF file.".into(),
            downloaded: meta.is_some(),
            bytes_on_disk: meta.map(|m| m.len()).unwrap_or(0),
            custom: true,
            path: c.path,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn local_status(app: AppHandle, engine: State<'_, Engine>) -> CmdResult<LocalStatus> {
    let hardware = cached_hardware(&engine).await;
    let runtime = match bundled_runtime(&app) {
        Some((_, version)) => RuntimeInfo { available: true, version, note: String::new() },
        None => RuntimeInfo {
            available: false,
            version: String::new(),
            note: "This build has no built-in runtime for this platform. Local models still work through Ollama or LM Studio.".into(),
        },
    };
    let running = engine.running.lock().await;
    let info = EngineInfo { running: running.is_some(), model: running.as_ref().map(|r| r.model_id.clone()).unwrap_or_default() };
    drop(running);
    Ok(LocalStatus {
        recommended: recommend(&hardware).to_string(),
        runtime,
        hardware,
        models: list_models(&app).map_err(err)?,
        engine: info,
        models_dir: models_dir(&app).map_err(err)?.display().to_string(),
    })
}

/* ---------------------------------------------------------- downloads */

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadEvent {
    Progress { id: String, received: u64, total: u64, bytes_per_sec: u64 },
    Done { id: String, path: String },
    Error { id: String, message: String },
    Cancelled { id: String },
}

const EVENT: &str = "model-download";

fn hub_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(6 * 3600)).user_agent("OmniConsole").build()?)
}

/// Size and sha256 of a file in a Hub repo, from the tree listing.
async fn hub_file_info(client: &reqwest::Client, repo: &str, file: &str) -> Result<(u64, Option<String>)> {
    let url = format!("https://huggingface.co/api/models/{repo}/tree/main");
    let resp = client.get(&url).send().await.with_context(|| format!("asking the Hub about {repo}"))?;
    if !resp.status().is_success() {
        bail!("the Hub returned {} for {repo}", resp.status());
    }
    let v: serde_json::Value = resp.json().await?;
    let entry = v.as_array().and_then(|a| a.iter().find(|e| e.get("path").and_then(|p| p.as_str()) == Some(file))).ok_or_else(|| anyhow!("{file} is not in {repo}"))?;
    let size = entry.pointer("/lfs/size").or(entry.get("size")).and_then(|s| s.as_u64()).unwrap_or(0);
    let sha = entry.pointer("/lfs/oid").and_then(|s| s.as_str()).map(String::from);
    Ok((size, sha))
}

/// Download a catalog model (resumable, verified). Progress arrives as `model-download` events.
#[tauri::command]
pub async fn model_download(app: AppHandle, engine: State<'_, Engine>, id: String) -> CmdResult<String> {
    let entry = CATALOG.iter().find(|e| e.id == id).ok_or_else(|| format!("unknown model {id}"))?;
    let (tx, rx) = watch::channel(false);
    if engine.downloads.lock().unwrap().insert(id.clone(), tx).is_some() {
        return Err("this model is already downloading".into());
    }
    let result = download(&app, entry, rx).await;
    engine.downloads.lock().unwrap().remove(&id);
    match result {
        Ok(Some(path)) => {
            let _ = app.emit(EVENT, DownloadEvent::Done { id: id.clone(), path: path.display().to_string() });
            Ok(path.display().to_string())
        }
        Ok(None) => {
            let _ = app.emit(EVENT, DownloadEvent::Cancelled { id: id.clone() });
            Err("cancelled".into())
        }
        Err(e) => {
            let _ = app.emit(EVENT, DownloadEvent::Error { id: id.clone(), message: format!("{e:#}") });
            Err(format!("{e:#}"))
        }
    }
}

async fn download(app: &AppHandle, entry: &CatalogEntry, mut cancel: watch::Receiver<bool>) -> Result<Option<PathBuf>> {
    let client = hub_client()?;
    let dest = models_dir(app)?.join(entry.file);
    let part = dest.with_extension("gguf.part");
    let (total_hint, sha) = hub_file_info(&client, entry.repo, entry.file).await.unwrap_or((entry.size_bytes, None));
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let url = format!("https://huggingface.co/{}/resolve/main/{}", entry.repo, entry.file);
    let mut req = client.get(&url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let resp = req.send().await.with_context(|| format!("downloading {}", entry.file))?;
    let status = resp.status();
    if !(status.is_success() || status.as_u16() == 206) {
        bail!("the Hub returned {status} for {}", url);
    }
    let resuming = status.as_u16() == 206;
    let mut received = if resuming { have } else { 0 };
    let total = if resuming { total_hint.max(received) } else { resp.content_length().unwrap_or(total_hint) };
    let mut file = tokio::fs::OpenOptions::new().create(true).append(resuming).write(true).truncate(!resuming).open(&part).await?;
    let mut resp = resp;
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    loop {
        let chunk = tokio::select! {
            c = resp.chunk() => c?,
            _ = cancel.changed() => {
                if *cancel.borrow() { file.flush().await?; return Ok(None); }
                continue;
            }
        };
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(400) {
            last_emit = Instant::now();
            let secs = started.elapsed().as_secs_f64().max(0.001);
            let fresh = received.saturating_sub(if resuming { have } else { 0 });
            let _ = app.emit(EVENT, DownloadEvent::Progress { id: entry.id.into(), received, total, bytes_per_sec: (fresh as f64 / secs) as u64 });
        }
    }
    file.flush().await?;
    drop(file);
    if total > 0 && received < total {
        bail!("the download stopped at {received} of {total} bytes; start it again to resume");
    }
    if let Some(expected) = sha {
        let part2 = part.clone();
        let actual = tokio::task::spawn_blocking(move || sha256_file(&part2)).await??;
        if actual != expected.to_lowercase() {
            let _ = fs::remove_file(&part);
            bail!("checksum mismatch after download (expected {expected}, got {actual}); the file was deleted, try again");
        }
    }
    fs::rename(&part, &dest)?;
    Ok(Some(dest))
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[tauri::command]
pub fn model_cancel(engine: State<'_, Engine>, id: String) {
    if let Some(tx) = engine.downloads.lock().unwrap().get(&id) {
        let _ = tx.send(true);
    }
}

/// Remove a downloaded model file (or forget a custom one; its file stays).
#[tauri::command]
pub async fn model_delete(app: AppHandle, engine: State<'_, Engine>, id: String) -> CmdResult<()> {
    {
        let mut running = engine.running.lock().await;
        if running.as_ref().map(|r| r.model_id == id).unwrap_or(false) {
            if let Some(mut r) = running.take() {
                let _ = r.child.start_kill();
            }
        }
    }
    if let Some(e) = CATALOG.iter().find(|e| e.id == id) {
        let dir = models_dir(&app).map_err(err)?;
        for p in [dir.join(e.file), dir.join(e.file).with_extension("gguf.part")] {
            if p.exists() {
                fs::remove_file(&p).map_err(err)?;
            }
        }
        return Ok(());
    }
    let list: Vec<CustomModel> = load_custom(&app).into_iter().filter(|c| c.id != id).collect();
    save_custom(&app, &list).map_err(err)
}

/// Pick a GGUF file from disk and add it to the list.
#[tauri::command]
pub async fn model_add_custom(app: AppHandle) -> CmdResult<Option<LocalModel>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().add_filter("GGUF model", &["gguf"]).pick_file(move |p| {
        let _ = tx.send(p);
    });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
    let id = format!("custom-{}", uuid::Uuid::new_v4());
    let mut list = load_custom(&app);
    list.push(CustomModel { id: id.clone(), name, path: path.display().to_string() });
    save_custom(&app, &list).map_err(err)?;
    Ok(list_models(&app).map_err(err)?.into_iter().find(|m| m.id == id))
}

/* ------------------------------------------------------------- server */

async fn free_port() -> Result<u16> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    Ok(l.local_addr()?.port())
}

/// Make sure the server runs with `model_id`; returns its base URL.
pub async fn ensure(app: &AppHandle, engine: &Engine, model_id: &str) -> Result<String> {
    let mut guard = engine.running.lock().await;
    if let Some(r) = guard.as_mut() {
        if r.model_id == model_id {
            match r.child.try_wait() {
                Ok(None) => return Ok(format!("http://127.0.0.1:{}", r.port)),
                _ => {
                    let tail = r.log.lock().unwrap().iter().rev().take(5).cloned().collect::<Vec<_>>().join(" | ");
                    log::warn!("built-in runtime exited; restarting. last log: {tail}");
                }
            }
        }
        if let Some(mut old) = guard.take() {
            let _ = old.child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), old.child.wait()).await;
        }
    }
    let path = model_path(app, model_id)?;
    if !path.is_file() {
        bail!("{} is not downloaded yet", model_name(app, model_id));
    }
    let (runtime, _version) = prepared_runtime(app)?;
    let hw = cached_hardware(engine).await;
    // all layers on the GPU with a roomy context first, then smaller, then CPU
    let gpu = hw.apple_silicon || hw.vram_gb > 0.0 || !hw.gpu_name.is_empty();
    let mut ladder: Vec<(PathBuf, u32, u32)> = if gpu {
        vec![(runtime.clone(), 99, 16384), (runtime.clone(), 99, 8192), (runtime.clone(), 0, 8192)]
    } else {
        vec![(runtime.clone(), 0, 8192), (runtime.clone(), 0, 4096)]
    };
    let cpu = runtime.join("cpu");
    if cpu.join(server_name()).is_file() {
        ladder.push((cpu, 0, 8192));
    }
    let mut last_err = String::new();
    for (dir, ngl, ctx) in ladder {
        match start_server(&dir, &path, ngl, ctx).await {
            Ok((child, port, log)) => {
                *guard = Some(Running { model_id: model_id.into(), port, child, log });
                return Ok(format!("http://127.0.0.1:{port}"));
            }
            Err(e) => {
                last_err = format!("{e:#}");
                log::warn!("built-in runtime (ngl {ngl}, ctx {ctx}) failed: {last_err}");
            }
        }
    }
    bail!("the built-in runtime could not start: {last_err}")
}

async fn start_server(runtime: &Path, model: &Path, ngl: u32, ctx: u32) -> Result<(Child, u16, Arc<Mutex<Vec<String>>>)> {
    let port = free_port().await?;
    let exe = runtime.join(server_name());
    let mut cmd = Command::new(&exe);
    cmd.args(["--model"]).arg(model);
    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string(), "-ngl", &ngl.to_string(), "-c", &ctx.to_string(), "-np", "1", "--jinja", "--no-webui"]);
    cmd.current_dir(runtime).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(target_os = "linux")]
    cmd.env("LD_LIBRARY_PATH", runtime);
    hide_window(&mut cmd);
    let mut child = cmd.spawn().with_context(|| format!("starting {}", exe.display()))?;
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    if let Some(stderr) = child.stderr.take() {
        let log2 = log.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut l = log2.lock().unwrap();
                l.push(line);
                if l.len() > 60 {
                    l.remove(0);
                }
            }
        });
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build()?;
    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = log.lock().unwrap().iter().rev().take(6).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            bail!("exited ({status}): {tail}");
        }
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                return Ok((child, port, log));
            }
        }
        if Instant::now() > deadline {
            let _ = child.start_kill();
            bail!("the model did not finish loading within 4 minutes");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tauri::command]
pub async fn engine_stop(engine: State<'_, Engine>) -> CmdResult<()> {
    let mut guard = engine.running.lock().await;
    if let Some(mut r) = guard.take() {
        let _ = r.child.start_kill();
    }
    Ok(())
}

/// Start the server for a model now, so the first question does not wait.
#[tauri::command]
pub async fn engine_start(app: AppHandle, engine: State<'_, Engine>, id: String) -> CmdResult<String> {
    ensure(&app, &engine, &id).await.map_err(err)
}

/// Downloaded model ids, for the provider's model list.
pub fn downloaded_ids(app: &AppHandle) -> Vec<String> {
    list_models(app).map(|l| l.into_iter().filter(|m| m.downloaded).map(|m| m.id).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_by_memory() {
        let hw = |ram: f32, vram: f32, apple: bool, gpu: &str| Hardware { ram_gb: ram, vram_gb: vram, apple_silicon: apple, gpu_name: gpu.into(), summary: String::new() };
        assert_eq!(recommend(&hw(32.0, 24.0, false, "NVIDIA GeForce RTX 4090")), "qwen3-30b-a3b");
        assert_eq!(recommend(&hw(32.0, 16.0, false, "NVIDIA GeForce RTX 4080")), "qwen3-14b");
        assert_eq!(recommend(&hw(16.0, 8.0, false, "NVIDIA GeForce RTX 3070")), "qwen3-8b");
        assert_eq!(recommend(&hw(16.0, 0.0, false, "AMD Radeon RX 6600")), "qwen3-4b");
        assert_eq!(recommend(&hw(16.0, 16.0, true, "Apple M2")), "qwen3-8b");
        assert_eq!(recommend(&hw(36.0, 36.0, true, "Apple M3 Max")), "qwen3-30b-a3b");
        assert_eq!(recommend(&hw(8.0, 0.0, false, "")), "qwen3-4b");
        assert_eq!(recommend(&hw(32.0, 0.0, false, "")), "qwen3-8b");
    }

    #[test]
    fn describes_hardware() {
        let hw = Hardware { ram_gb: 32.0, vram_gb: 12.0, apple_silicon: false, gpu_name: "NVIDIA GeForce RTX 3080".into(), summary: String::new() };
        assert_eq!(describe(&hw), "NVIDIA GeForce RTX 3080 with 12 GB · 32 GB memory");
        let hw = Hardware { ram_gb: 16.0, ..Default::default() };
        assert_eq!(describe(&hw), "No GPU found · 16 GB memory");
    }

    #[test]
    fn catalog_ids_are_unique_and_sized() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for e in CATALOG {
            assert!(e.file.ends_with(".gguf"));
            assert!(e.size_bytes > GB);
            assert!(e.needs_gb > 0.0);
        }
    }

    #[test]
    fn sha256_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.bin");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
