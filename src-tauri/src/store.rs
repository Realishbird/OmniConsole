//! Saved devices (devices.json), known SSH host keys (known_hosts.json),
//! small app data files (snippets, command history) and secrets in the OS
//! credential store (Windows Credential Manager, macOS Keychain, Secret
//! Service on Linux). Secrets never touch the JSON files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const KEYRING_SERVICE: &str = "OmniConsole";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub protocol: String, // "ssh" | "telnet" | "serial"
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,

    // where it lives
    #[serde(default)]
    pub customer: String,
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub building: String,
    #[serde(default)]
    pub rack: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// "production" | "staging" | "lab" | "customer" | ""
    #[serde(default)]
    pub environment: String,
    /// tab color override, "#rrggbb" or ""
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub favorite: bool,
    /// unix ms of the last connect
    #[serde(default)]
    pub last_connected: Option<i64>,
    #[serde(default)]
    pub notes: String,

    // SSH
    /// "password" | "key" | "agent"
    #[serde(default = "default_auth")]
    pub auth_method: String,
    #[serde(default)]
    pub key_path: String,
    /// another saved SSH device used as a jump host
    #[serde(default)]
    pub jump_device_id: String,
    #[serde(default = "default_keepalive")]
    pub keepalive: u32,

    // serial
    #[serde(default)]
    pub serial_port: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// "none" | "odd" | "even"
    #[serde(default = "default_none")]
    pub parity: String,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// "none" | "software" | "hardware"
    #[serde(default = "default_none")]
    pub flow_control: String,

    // session behaviour
    #[serde(default)]
    pub auto_reconnect: bool,
    #[serde(default)]
    pub log_session: bool,
    #[serde(default)]
    pub auto_login: bool,
}

fn default_auth() -> String {
    "password".into()
}
fn default_keepalive() -> u32 {
    30
}
fn default_baud() -> u32 {
    9600
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_none() -> String {
    "none".into()
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir().context("no config folder")?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Default folder for session logs: Documents/OmniConsole/Logs.
pub fn default_log_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("OmniConsole")
        .join("Logs")
}

/// Write via a temporary file so a crash never leaves half a file.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_devices(dir: &Path) -> Result<Vec<Device>> {
    let path = dir.join("devices.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
}

pub fn save_devices(dir: &Path, devices: &[Device]) -> Result<()> {
    write_atomic(&dir.join("devices.json"), &serde_json::to_string_pretty(devices)?)
}

/*
 *  small app data files the UI owns (snippets, history, settings)
 */
const DATA_KINDS: &[&str] = &["snippets", "history", "settings", "workspace", "audit", "checklists"];

fn data_path(dir: &Path, kind: &str) -> Result<PathBuf> {
    if !DATA_KINDS.contains(&kind) {
        bail!("unknown data file {kind}");
    }
    Ok(dir.join(format!("{kind}.json")))
}

pub fn data_load(dir: &Path, kind: &str) -> Result<serde_json::Value> {
    let path = data_path(dir, kind)?;
    if !path.exists() {
        return Ok(serde_json::Value::Null);
    }
    Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
}

pub fn data_save(dir: &Path, kind: &str, value: &serde_json::Value) -> Result<()> {
    write_atomic(&data_path(dir, kind)?, &serde_json::to_string_pretty(value)?)
}

/*
 *  import / export of connection profiles (never includes secrets)
 */
#[derive(Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    devices: Vec<Device>,
}

pub fn export_devices(devices: &[Device]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&ExportFile {
        format: "omniconsole-devices".into(),
        version: 1,
        devices: devices.to_vec(),
    })?)
}

/// OmniConsole JSON (file or bare array) or CSV with a header row.
pub fn parse_import(text: &str) -> Result<Vec<Device>> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('{') {
        let file: ExportFile = serde_json::from_str(trimmed).context("not an OmniConsole export")?;
        return Ok(file.devices);
    }
    if trimmed.starts_with('[') {
        return Ok(serde_json::from_str(trimmed)?);
    }
    parse_csv(trimmed)
}

fn csv_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

fn parse_csv(text: &str) -> Result<Vec<Device>> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<String> = csv_fields(lines.next().context("empty file")?)
        .into_iter()
        .map(|h| h.to_lowercase().replace([' ', '_'], ""))
        .collect();
    let col = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
    let c_name = col(&["name", "hostname", "label", "device"]);
    let c_host = col(&["host", "ip", "address", "ipaddress", "mgmtip"]);
    if c_name.is_none() && c_host.is_none() {
        bail!("the CSV needs a name or host column");
    }
    let c_port = col(&["port"]);
    let c_proto = col(&["protocol", "proto", "type"]);
    let c_user = col(&["username", "user", "login"]);
    let c_customer = col(&["customer", "client", "tenant"]);
    let c_site = col(&["site", "location"]);
    let c_building = col(&["building"]);
    let c_rack = col(&["rack"]);
    let c_role = col(&["role", "devicerole"]);
    let c_tags = col(&["tags", "tag"]);
    let c_env = col(&["environment", "env"]);
    let c_serial = col(&["serialport", "comport", "com"]);
    let c_baud = col(&["baud", "baudrate", "speed"]);
    let c_notes = col(&["notes", "description", "comment"]);

    let mut out = Vec::new();
    for line in lines {
        let f = csv_fields(line);
        let get = |c: Option<usize>| c.and_then(|i| f.get(i)).cloned().unwrap_or_default();
        let proto = match get(c_proto).to_lowercase().as_str() {
            "telnet" => "telnet",
            "serial" | "console" => "serial",
            _ => "ssh",
        }
        .to_string();
        let host = get(c_host);
        let name = Some(get(c_name)).filter(|n| !n.is_empty()).unwrap_or_else(|| host.clone());
        let port = get(c_port).parse().unwrap_or(match proto.as_str() {
            "telnet" => 23,
            "serial" => 0,
            _ => 22,
        });
        let mut d: Device = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": name,
            "protocol": proto,
        }))?;
        d.host = host;
        d.port = port;
        d.username = get(c_user);
        d.customer = get(c_customer);
        d.site = get(c_site);
        d.building = get(c_building);
        d.rack = get(c_rack);
        d.role = get(c_role);
        d.tags = get(c_tags)
            .split([';', '|'])
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        d.environment = get(c_env).to_lowercase();
        d.serial_port = get(c_serial);
        if let Ok(b) = get(c_baud).parse() {
            d.baud = b;
        }
        d.notes = get(c_notes);
        out.push(d);
    }
    Ok(out)
}

/*
 *  known hosts: "host:port" -> "<algorithm> <base64 key>" (OpenSSH format)
 */
pub enum HostKeyCheck {
    Known,
    New,
    Changed,
}

fn known_hosts_path(dir: &Path) -> PathBuf {
    dir.join("known_hosts.json")
}

fn load_known_hosts(dir: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(known_hosts_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn check_host_key(dir: &Path, host_port: &str, key: &str) -> HostKeyCheck {
    match load_known_hosts(dir).get(host_port) {
        None => HostKeyCheck::New,
        Some(k) if k == key => HostKeyCheck::Known,
        Some(_) => HostKeyCheck::Changed,
    }
}

pub fn remember_host_key(dir: &Path, host_port: &str, key: &str) -> Result<()> {
    let mut hosts = load_known_hosts(dir);
    hosts.insert(host_port.to_string(), key.to_string());
    write_atomic(&known_hosts_path(dir), &serde_json::to_string_pretty(&hosts)?)
}

/*
 *  secrets: "device/<id>" = password, "passphrase/<id>" = SSH key passphrase
 */
fn entry(name: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, name)?)
}

pub fn secret_get(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok()
}

pub fn secret_set(name: &str, value: &str) -> Result<()> {
    entry(name)?.set_password(value)?;
    Ok(())
}

pub fn secret_delete(name: &str) -> Result<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Only these secret names may be used, so the UI cannot read other
/// applications' keychain entries.
pub fn secret_name(kind: &str, device_id: &str) -> Result<String> {
    if uuid::Uuid::parse_str(device_id).is_err() {
        bail!("bad device id");
    }
    match kind {
        "password" => Ok(format!("device/{device_id}")),
        "passphrase" => Ok(format!("passphrase/{device_id}")),
        // AI provider API keys, keyed by the provider id
        "apikey" => Ok(format!("ai/{device_id}")),
        _ => bail!("unknown secret kind {kind}"),
    }
}

pub fn password_get(device_id: &str) -> Option<String> {
    secret_get(&format!("device/{device_id}"))
}

pub fn passphrase_get(device_id: &str) -> Option<String> {
    secret_get(&format!("passphrase/{device_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_import() {
        let csv = "Name,IP,Protocol,Customer,Site,Tags,Env\n\
                   core-sw1,10.0.0.1,ssh,Acme,HQ,core;stack,Production\n\
                   \"lab, rack 2\",10.9.9.9,telnet,,Lab,,lab\n";
        let d = parse_import(csv).unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "core-sw1");
        assert_eq!(d[0].port, 22);
        assert_eq!(d[0].tags, vec!["core", "stack"]);
        assert_eq!(d[0].environment, "production");
        assert_eq!(d[1].name, "lab, rack 2");
        assert_eq!(d[1].protocol, "telnet");
        assert_eq!(d[1].port, 23);
    }

    #[test]
    fn json_roundtrip_and_old_files() {
        // a devices.json written by 0.1 still loads
        let old = r#"[{"id":"a","name":"sw","site":"HQ","protocol":"ssh","host":"h","port":22,"username":"","serialPort":"","baud":9600,"notes":""}]"#;
        let d: Vec<Device> = serde_json::from_str(old).unwrap();
        assert_eq!(d[0].auth_method, "password");
        assert_eq!(d[0].data_bits, 8);
        let back = parse_import(&export_devices(&d).unwrap()).unwrap();
        assert_eq!(back[0].site, "HQ");
    }

    #[test]
    fn secret_names_are_restricted() {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(secret_name("password", &id).is_ok());
        assert!(secret_name("password", "../other").is_err());
        assert!(secret_name("token", &id).is_err());
    }
}
