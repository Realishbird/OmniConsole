//! Config backups on disk: `<root>/<site>/<device>/<YYYY-MM-DD_HH-MM-SS>_<kind>.cfg`,
//! plus check reports beside them as `_checks.json`. Diff and search read
//! from the same tree. Paths handed to the UI are always inside `root`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub path: String,
    pub site: String,
    pub device: String,
    /// running, startup or checks
    pub kind: String,
    /// unix seconds
    pub timestamp: i64,
    pub size: u64,
}

fn safe_name(s: &str) -> String {
    let cleaned: String = s.trim().chars().map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' }).collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    if cleaned.is_empty() { "unnamed".into() } else { cleaned }
}

fn stamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0).map(|t| t.format("%Y-%m-%d_%H-%M-%S").to_string()).unwrap_or_else(|| ts.to_string())
}

pub fn device_dir(root: &Path, site: &str, device: &str) -> PathBuf {
    root.join(safe_name(if site.trim().is_empty() { "no-site" } else { site })).join(safe_name(device))
}

/// Write `text` as a new backup file and describe it.
pub fn save(root: &Path, site: &str, device: &str, kind: &str, text: &str) -> Result<Backup> {
    let kind = safe_name(kind);
    let dir = device_dir(root, site, device);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let ts = chrono::Utc::now().timestamp();
    let ext = if kind == "checks" { "json" } else { "cfg" };
    let mut path = dir.join(format!("{}_{kind}.{ext}", stamp(ts)));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{}_{kind}-{n}.{ext}", stamp(ts)));
        n += 1;
    }
    crate::store::write_atomic(&path, text)?;
    Ok(Backup { path: path.display().to_string(), site: site.into(), device: device.into(), kind, timestamp: ts, size: text.len() as u64 })
}

fn parse_name(name: &str) -> Option<(i64, String)> {
    // 2026-10-14_09-30-00_running.cfg  /  ..._running-2.cfg
    let re = Regex::new(r"^(\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2})_([a-z]+)(?:-\d+)?\.(cfg|json)$").ok()?;
    let c = re.captures(name)?;
    let ts = chrono::NaiveDateTime::parse_from_str(&c[1], "%Y-%m-%d_%H-%M-%S").ok()?.and_utc().timestamp();
    Some((ts, c[2].to_string()))
}

/// Every backup under root, newest first. `device` narrows to one device
/// (matched by folder name, any site).
pub fn list(root: &Path, device: Option<&str>) -> Vec<Backup> {
    let mut out = Vec::new();
    let Ok(sites) = fs::read_dir(root) else { return out };
    for site in sites.flatten().filter(|e| e.path().is_dir()) {
        let Ok(devices) = fs::read_dir(site.path()) else { continue };
        for dev in devices.flatten().filter(|e| e.path().is_dir()) {
            let dev_name = dev.file_name().to_string_lossy().to_string();
            if device.is_some_and(|d| safe_name(d) != dev_name) {
                continue;
            }
            let Ok(files) = fs::read_dir(dev.path()) else { continue };
            for f in files.flatten().filter(|e| e.path().is_file()) {
                let name = f.file_name().to_string_lossy().to_string();
                let Some((timestamp, kind)) = parse_name(&name) else { continue };
                out.push(Backup {
                    path: f.path().display().to_string(),
                    site: site.file_name().to_string_lossy().to_string(),
                    device: dev_name.clone(),
                    kind,
                    timestamp,
                    size: f.metadata().map(|m| m.len()).unwrap_or(0),
                });
            }
        }
    }
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(a.path.cmp(&b.path)));
    out
}

/// Resolve `path` and make sure it stays inside `root`.
pub fn inside(root: &Path, path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("path escapes the config folder");
    }
    let root_c = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let full = fs::canonicalize(&p).unwrap_or(p);
    if !full.starts_with(&root_c) {
        bail!("{} is outside the config folder", full.display());
    }
    Ok(full)
}

pub fn read(root: &Path, path: &str) -> Result<String> {
    let p = inside(root, path)?;
    fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))
}

pub fn delete(root: &Path, path: &str) -> Result<()> {
    let p = inside(root, path)?;
    fs::remove_file(&p).with_context(|| format!("deleting {}", p.display()))
}

/* ------------------------------------------------------------------ diff */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    /// " ", "+" or "-"
    pub tag: String,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub added: usize,
    pub removed: usize,
    pub lines: Vec<DiffLine>,
    pub unified: String,
}

/// Lines that change on every save without meaning anything.
fn volatile() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(!\s*(Last configuration change|NVRAM config last updated|No configuration change since|Time:|Running configuration last done|Startup config saved|Command: show)|Current configuration\s*:|Building configuration|ntp clock-period|!Startup-config last modified|: Written by|: Saved|: Serial Number|: Hardware:|crypto pki certificate chain|\s+certificate|\s+[0-9A-F]{8} )").unwrap()
    })
}

pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_cert = false;
    for line in text.lines() {
        let l = line.trim_end();
        if l.starts_with("crypto pki certificate chain") {
            in_cert = true;
            continue;
        }
        if in_cert {
            // certificate bodies are hex blocks ending in "quit"
            if l.trim() == "quit" || l == "!" {
                in_cert = false;
            }
            continue;
        }
        if volatile().is_match(l) {
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

pub fn diff(old: &str, new: &str, ignore_volatile: bool) -> DiffResult {
    let (a, b) = if ignore_volatile { (normalize(old), normalize(new)) } else { (old.to_string(), new.to_string()) };
    let d = TextDiff::from_lines(&a, &b);
    let mut lines = Vec::new();
    let (mut added, mut removed) = (0, 0);
    for change in d.iter_all_changes() {
        let tag = match change.tag() {
            ChangeTag::Equal => " ",
            ChangeTag::Insert => {
                added += 1;
                "+"
            }
            ChangeTag::Delete => {
                removed += 1;
                "-"
            }
        };
        lines.push(DiffLine {
            tag: tag.into(),
            old: change.old_index().map(|i| i + 1),
            new: change.new_index().map(|i| i + 1),
            text: change.value().trim_end_matches(['\n', '\r']).to_string(),
        });
    }
    let unified = d.unified_diff().context_radius(3).header("before", "after").to_string();
    DiffResult { added, removed, lines, unified }
}

/* ---------------------------------------------------------------- search */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub path: String,
    pub site: String,
    pub device: String,
    pub timestamp: i64,
    pub line_no: usize,
    pub line: String,
}

/// Search the newest backup of each device (or all backups when
/// `all_versions`). Plain text is matched case-insensitively.
pub fn search(root: &Path, query: &str, use_regex: bool, all_versions: bool) -> Result<Vec<SearchHit>> {
    let re = if use_regex { Regex::new(&format!("(?i){query}")).context("bad regular expression")? } else { Regex::new(&format!("(?i){}", regex::escape(query)))? };
    let mut backups: Vec<Backup> = list(root, None).into_iter().filter(|b| b.kind != "checks").collect();
    if !all_versions {
        let mut seen = std::collections::HashSet::new();
        backups.retain(|b| seen.insert((b.site.clone(), b.device.clone(), b.kind.clone())));
    }
    let mut hits = Vec::new();
    for b in backups {
        let Ok(text) = fs::read_to_string(&b.path) else { continue };
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                hits.push(SearchHit { path: b.path.clone(), site: b.site.clone(), device: b.device.clone(), timestamp: b.timestamp, line_no: i + 1, line: line.trim_end().to_string() });
                if hits.len() >= 2000 {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

/* ---------------------------------------------------------------- bundle */

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleFile {
    pub name: String,
    pub contents: String,
}

/// Zip the given files (deflate). Names are flattened to safe file names.
pub fn write_bundle(path: &Path, files: &[BundleFile]) -> Result<()> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let file = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut used = std::collections::HashSet::new();
    for f in files {
        let mut name = f.name.replace(['/', '\\'], "_");
        if name.trim().is_empty() {
            name = "file.txt".into();
        }
        let mut n = 1;
        let base = name.clone();
        while !used.insert(name.clone()) {
            name = format!("{n}_{base}");
            n += 1;
        }
        zip.start_file(name, opts)?;
        zip.write_all(f.contents.as_bytes())?;
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_read_and_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let b = save(tmp.path(), "HQ", "core-sw1", "running", "hostname core-sw1\n").unwrap();
        assert_eq!(b.kind, "running");
        assert!(b.path.ends_with("_running.cfg"));
        let b2 = save(tmp.path(), "HQ", "core-sw1", "running", "hostname core-sw1\nntp server 1.1.1.1\n").unwrap();
        let all = list(tmp.path(), None);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].device, "core-sw1");
        assert!(list(tmp.path(), Some("other")).is_empty());
        assert_eq!(read(tmp.path(), &b.path).unwrap(), "hostname core-sw1\n");
        assert!(read(tmp.path(), "/etc/hostname").is_err());
        assert!(read(tmp.path(), &format!("{}/../../etc/passwd", b2.path)).is_err());
        delete(tmp.path(), &b.path).unwrap();
        assert_eq!(list(tmp.path(), None).len(), 1);
    }

    #[test]
    fn diff_ignores_volatile_lines() {
        let old = "! Last configuration change at 10:00:00 UTC Mon Oct 13 2026\nhostname sw1\ninterface Gi1/0/1\n description a\n";
        let new = "! Last configuration change at 11:00:00 UTC Tue Oct 14 2026\nhostname sw1\ninterface Gi1/0/1\n description b\n";
        let d = diff(old, new, true);
        assert_eq!((d.added, d.removed), (1, 1));
        assert!(d.unified.contains("- description a"));
        let d = diff(old, new, false);
        assert_eq!((d.added, d.removed), (2, 2));
    }

    #[test]
    fn search_newest_only() {
        let tmp = tempfile::tempdir().unwrap();
        save(tmp.path(), "HQ", "sw1", "running", "snmp-server community OLD RO\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        save(tmp.path(), "HQ", "sw1", "running", "snmp-server community NEW RO\n").unwrap();
        save(tmp.path(), "Lab", "sw2", "running", "no snmp\n").unwrap();
        let hits = search(tmp.path(), "snmp-server", false, false).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].line.contains("NEW"));
        assert_eq!(search(tmp.path(), "snmp-server", false, true).unwrap().len(), 2);
        assert_eq!(search(tmp.path(), r"community \w+ RO", true, false).unwrap().len(), 1);
    }

    #[test]
    fn bundle_zips() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("b.zip");
        write_bundle(&p, &[BundleFile { name: "a/show version.txt".into(), contents: "x".into() }, BundleFile { name: "a_show version.txt".into(), contents: "y".into() }]).unwrap();
        let mut z = zip::ZipArchive::new(fs::File::open(&p).unwrap()).unwrap();
        assert_eq!(z.len(), 2);
        assert!(z.by_name("a_show version.txt").is_ok());
        assert!(z.by_name("1_a_show version.txt").is_ok());
    }
}
