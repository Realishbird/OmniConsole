//! Saved devices (devices.json), known SSH host keys (known_hosts.json) and
//! passwords in the OS credential store (Windows Credential Manager, macOS
//! Keychain, Secret Service on Linux). Passwords never touch the JSON files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const KEYRING_SERVICE: &str = "OmniConsole";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub site: String,
    pub protocol: String, // "ssh" | "telnet" | "serial"
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub serial_port: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default)]
    pub notes: String,
}

fn default_baud() -> u32 {
    9600
}

pub fn config_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir().context("no config folder")?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Write via a temporary file so a crash never leaves half a file.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_devices(app: &AppHandle) -> Result<Vec<Device>> {
    let path = config_dir(app)?.join("devices.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?)
}

pub fn save_devices(app: &AppHandle, devices: &[Device]) -> Result<()> {
    let path = config_dir(app)?.join("devices.json");
    write_atomic(&path, &serde_json::to_string_pretty(devices)?)
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
 *  passwords
 */
fn entry(device_id: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, &format!("device/{device_id}"))?)
}

pub fn password_get(device_id: &str) -> Option<String> {
    entry(device_id).ok()?.get_password().ok()
}

pub fn password_set(device_id: &str, password: &str) -> Result<()> {
    entry(device_id)?.set_password(password)?;
    Ok(())
}

pub fn password_delete(device_id: &str) -> Result<()> {
    match entry(device_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
