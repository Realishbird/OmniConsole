//! The credential locker: named login profiles that devices reference, with
//! defaults per customer or site, a vault lock, encrypted export and a
//! connection test. Secrets live only in the OS credential store (Credential
//! Manager, Keychain, Secret Service) under `cred/<id>`; this file keeps the
//! names, usernames and bookkeeping in credentials.json. Nothing here logs,
//! returns in an error, or exports a secret in clear text.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::store;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/* ------------------------------------------------------------- model */

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub username: String,
    /// "password" | "key" | "agent"
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub key_path: String,
    /// Windows / TACACS domain, prepended as DOMAIN\user when set
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub has_password: bool,
    #[serde(default)]
    pub has_enable: bool,
    #[serde(default)]
    pub has_passphrase: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    /// unix ms of the last change to the profile or its secrets
    #[serde(default)]
    pub updated: i64,
    /// temporary credentials: unix ms after which the secrets are deleted
    #[serde(default)]
    pub expires_at: Option<i64>,
}

fn default_method() -> String {
    "password".into()
}

/// Which profile a customer or site uses when the device names none.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDefault {
    /// "customer" | "site"
    pub scope: String,
    pub value: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Locker {
    #[serde(default)]
    pub credentials: Vec<Credential>,
    #[serde(default)]
    pub defaults: Vec<CredentialDefault>,
    /// minutes of inactivity before the vault locks (0 = never)
    #[serde(default)]
    pub auto_lock_minutes: u32,
    /// clipboard is cleared this many seconds after a copy (0 = never)
    #[serde(default = "default_clip")]
    pub clipboard_clear_seconds: u32,
    /// password prompts per connection before giving up
    #[serde(default = "default_retries")]
    pub retry_limit: u32,
}

fn default_clip() -> u32 {
    20
}
fn default_retries() -> u32 {
    3
}

pub fn load(dir: &Path) -> Result<Locker> {
    let path = dir.join("credentials.json");
    if !path.exists() {
        return Ok(Locker { clipboard_clear_seconds: 20, retry_limit: 3, ..Default::default() });
    }
    serde_json::from_str(&std::fs::read_to_string(&path)?).with_context(|| format!("reading {}", path.display()))
}

pub fn save(dir: &Path, locker: &Locker) -> Result<()> {
    for c in &locker.credentials {
        if uuid::Uuid::parse_str(&c.id).is_err() {
            bail!("bad credential id");
        }
    }
    store::write_atomic(&dir.join("credentials.json"), &serde_json::to_string_pretty(locker)?)
}

pub const SECRET_KINDS: &[(&str, &str)] = &[("cred-password", "cred"), ("cred-enable", "cred-enable"), ("cred-passphrase", "cred-passphrase")];

fn secret(kind: &str, id: &str) -> Option<String> {
    if vault().locked() {
        return None;
    }
    let out = store::secret_get(&format!("{kind}/{id}"));
    if out.is_some() {
        vault().touch();
    }
    out
}

/* ---------------------------------------------------------- resolving */

/// The profile a device uses: its own reference, else the site default,
/// else the customer default. Expired temporary profiles do not count.
pub fn credential_for(locker: &Locker, device: &store::Device) -> Option<Credential> {
    let now = chrono::Utc::now().timestamp_millis();
    let alive = |c: &Credential| c.expires_at.map(|t| t > now).unwrap_or(true);
    let find = |id: &str| locker.credentials.iter().find(|c| c.id == id && alive(c)).cloned();
    if !device.credential_id.is_empty() {
        return find(&device.credential_id);
    }
    let by = |scope: &str, value: &str| {
        if value.is_empty() {
            return None;
        }
        locker.defaults.iter().find(|d| d.scope == scope && d.value.eq_ignore_ascii_case(value)).and_then(|d| find(&d.credential_id))
    };
    by("site", &device.site).or_else(|| by("customer", &device.customer))
}

/// Username, password, key path and passphrase a session should use for a saved device.
#[derive(Debug, Default, Clone)]
pub struct Resolved {
    pub username: Option<String>,
    pub password: Option<String>,
    pub enable: Option<String>,
    pub key_path: Option<String>,
    pub passphrase: Option<String>,
    pub method: Option<String>,
    /// profile name, for notices (never the secret)
    pub source: String,
}

pub fn resolve(dir: &Path, device_id: &str) -> Resolved {
    let mut r = Resolved::default();
    if uuid::Uuid::parse_str(device_id).is_err() {
        return r;
    }
    let devices = store::load_devices(dir).unwrap_or_default();
    let locker = load(dir).unwrap_or_default();
    let device = devices.iter().find(|d| d.id == device_id);
    if let Some(c) = device.and_then(|d| credential_for(&locker, d)) {
        let user = if c.domain.is_empty() || c.username.is_empty() { c.username.clone() } else { format!("{}\\{}", c.domain, c.username) };
        r.username = (!user.is_empty()).then_some(user);
        r.method = Some(c.method.clone());
        r.key_path = (!c.key_path.is_empty()).then(|| c.key_path.clone());
        r.password = c.has_password.then(|| secret("cred", &c.id)).flatten();
        r.enable = c.has_enable.then(|| secret("cred-enable", &c.id)).flatten();
        r.passphrase = c.has_passphrase.then(|| secret("cred-passphrase", &c.id)).flatten();
        r.source = c.name.clone();
        return r;
    }
    // the device's own saved secrets
    if !vault().locked() {
        r.password = store::secret_get(&format!("device/{device_id}"));
        r.passphrase = store::secret_get(&format!("passphrase/{device_id}"));
        if r.password.is_some() || r.passphrase.is_some() {
            vault().touch();
        }
    }
    r
}

/// Password for a saved device: profile first, then its own saved password. None when the vault is locked.
pub fn password_for(dir: &Path, device_id: &str) -> Option<String> {
    resolve(dir, device_id).password
}

pub fn passphrase_for(dir: &Path, device_id: &str) -> Option<String> {
    resolve(dir, device_id).passphrase
}

/* --------------------------------------------------------------- vault */

/// App-level lock over every stored secret. The OS store keeps the secrets;
/// while the vault is locked the app does not read them, so connects ask for
/// the password and reveal/copy are refused.
pub struct Vault {
    locked: Mutex<bool>,
    last: Mutex<Instant>,
    auto_lock: Mutex<Duration>,
    failures: Mutex<(u32, Instant)>,
}

static VAULT: OnceLock<Vault> = OnceLock::new();

pub fn vault() -> &'static Vault {
    VAULT.get_or_init(|| Vault { locked: Mutex::new(false), last: Mutex::new(Instant::now()), auto_lock: Mutex::new(Duration::ZERO), failures: Mutex::new((0, Instant::now())) })
}

impl Vault {
    /// A vault with no passphrase set never locks.
    pub fn configured(&self) -> bool {
        store::secret_get("vault/verifier").is_some()
    }

    pub fn locked(&self) -> bool {
        if !self.configured() {
            return false;
        }
        let auto = *self.auto_lock.lock().unwrap();
        if !auto.is_zero() && self.last.lock().unwrap().elapsed() > auto {
            *self.locked.lock().unwrap() = true;
        }
        *self.locked.lock().unwrap()
    }

    pub fn touch(&self) {
        *self.last.lock().unwrap() = Instant::now();
    }

    pub fn lock(&self) {
        *self.locked.lock().unwrap() = true;
    }

    fn set_auto_lock(&self, minutes: u32) {
        *self.auto_lock.lock().unwrap() = Duration::from_secs(minutes as u64 * 60);
    }

    /// Unlock with the passphrase. Five wrong tries in a row add a growing delay.
    pub fn unlock(&self, passphrase: &str) -> Result<bool> {
        {
            let (n, since) = *self.failures.lock().unwrap();
            if n >= 5 {
                let wait = Duration::from_secs(30 * (n as u64 - 4).min(20));
                if since.elapsed() < wait {
                    bail!("too many wrong passphrases; try again in {} s", (wait - since.elapsed()).as_secs() + 1);
                }
            }
        }
        let ok = verify(passphrase)?;
        let mut f = self.failures.lock().unwrap();
        if ok {
            *f = (0, Instant::now());
            *self.locked.lock().unwrap() = false;
            self.touch();
        } else {
            *f = (f.0 + 1, Instant::now());
        }
        Ok(ok)
    }
}

/// argon2id over a random salt; "salt:hash" base64 in the OS store.
fn verifier(passphrase: &str) -> Result<String> {
    let salt: [u8; 16] = rand::random();
    let mut out = [0u8; 32];
    argon2::Argon2::default().hash_password_into(passphrase.as_bytes(), &salt, &mut out).map_err(|e| anyhow!("{e}"))?;
    let b = base64::engine::general_purpose::STANDARD;
    Ok(format!("{}:{}", b.encode(salt), b.encode(out)))
}

fn verify(passphrase: &str) -> Result<bool> {
    let Some(stored) = store::secret_get("vault/verifier") else { return Ok(true) };
    let b = base64::engine::general_purpose::STANDARD;
    let (salt, hash) = stored.split_once(':').ok_or_else(|| anyhow!("bad vault verifier"))?;
    let salt = b.decode(salt)?;
    let hash = b.decode(hash)?;
    let mut out = vec![0u8; hash.len()];
    argon2::Argon2::default().hash_password_into(passphrase.as_bytes(), &salt, &mut out).map_err(|e| anyhow!("{e}"))?;
    Ok(ct_eq(&out, &hash))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub configured: bool,
    pub locked: bool,
    pub auto_lock_minutes: u32,
}

fn dir(app: &AppHandle) -> CmdResult<std::path::PathBuf> {
    store::config_dir(app).map_err(err)
}

#[tauri::command]
pub fn vault_status(app: AppHandle) -> CmdResult<VaultStatus> {
    let locker = load(&dir(&app)?).map_err(err)?;
    vault().set_auto_lock(locker.auto_lock_minutes);
    Ok(VaultStatus { configured: vault().configured(), locked: vault().locked(), auto_lock_minutes: locker.auto_lock_minutes })
}

/// Set or change the vault passphrase (the current one is needed to change
/// it); an empty new passphrase removes the lock.
#[tauri::command]
pub fn vault_setup(app: AppHandle, current: Option<String>, passphrase: String) -> CmdResult<VaultStatus> {
    if vault().configured() {
        if vault().locked() {
            return Err("unlock the vault first".into());
        }
        if !verify(current.as_deref().unwrap_or("")).map_err(err)? {
            return Err("the current passphrase is wrong".into());
        }
    }
    if passphrase.is_empty() {
        store::secret_delete("vault/verifier").map_err(err)?;
    } else {
        if passphrase.chars().count() < 8 {
            return Err("use at least 8 characters".into());
        }
        store::secret_set("vault/verifier", &verifier(&passphrase).map_err(err)?).map_err(err)?;
    }
    *vault().locked.lock().unwrap() = false;
    vault().touch();
    vault_status(app)
}

#[tauri::command]
pub fn vault_lock(app: AppHandle) -> CmdResult<VaultStatus> {
    vault().lock();
    vault_status(app)
}

#[tauri::command]
pub fn vault_unlock(app: AppHandle, passphrase: String) -> CmdResult<VaultStatus> {
    if !vault().unlock(&passphrase).map_err(err)? {
        return Err("wrong passphrase".into());
    }
    vault_status(app)
}

/// The UI reports activity so the auto-lock timer restarts.
#[tauri::command]
pub fn vault_touch() {
    vault().touch();
}

/* ------------------------------------------------------------ commands */

#[tauri::command]
pub fn credentials_load(app: AppHandle) -> CmdResult<Locker> {
    let d = dir(&app)?;
    let mut locker = load(&d).map_err(err)?;
    vault().set_auto_lock(locker.auto_lock_minutes);
    // temporary credentials past their time lose their secrets
    let now = chrono::Utc::now().timestamp_millis();
    let mut changed = false;
    for c in locker.credentials.iter_mut() {
        if c.expires_at.map(|t| t <= now).unwrap_or(false) && (c.has_password || c.has_enable || c.has_passphrase) {
            for (_, prefix) in SECRET_KINDS {
                let _ = store::secret_delete(&format!("{prefix}/{}", c.id));
            }
            c.has_password = false;
            c.has_enable = false;
            c.has_passphrase = false;
            changed = true;
        }
    }
    if changed {
        save(&d, &locker).map_err(err)?;
    }
    Ok(locker)
}

#[tauri::command]
pub fn credentials_save(app: AppHandle, locker: Locker) -> CmdResult<()> {
    vault().set_auto_lock(locker.auto_lock_minutes);
    save(&dir(&app)?, &locker).map_err(err)
}

/// Store a profile secret. `kind` is cred-password, cred-enable or cred-passphrase; an empty value deletes it.
#[tauri::command]
pub fn credential_secret_set(kind: String, id: String, value: String) -> CmdResult<()> {
    if vault().locked() {
        return Err("the vault is locked".into());
    }
    let name = store::secret_name(&kind, &id).map_err(err)?;
    if value.is_empty() { store::secret_delete(&name).map_err(err) } else { store::secret_set(&name, &value).map_err(err) }
}

/// Reveal a profile secret for the masked view or the clipboard; refused while locked.
#[tauri::command]
pub fn credential_secret_get(kind: String, id: String) -> CmdResult<Option<String>> {
    if vault().locked() {
        return Err("the vault is locked".into());
    }
    let name = store::secret_name(&kind, &id).map_err(err)?;
    vault().touch();
    Ok(store::secret_get(&name))
}

/// Remove a profile and its secrets; devices that referenced it fall back to defaults.
#[tauri::command]
pub fn credential_delete(app: AppHandle, id: String) -> CmdResult<Locker> {
    let d = dir(&app)?;
    let mut locker = load(&d).map_err(err)?;
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err("bad credential id".into());
    }
    locker.credentials.retain(|c| c.id != id);
    locker.defaults.retain(|x| x.credential_id != id);
    save(&d, &locker).map_err(err)?;
    for (_, prefix) in SECRET_KINDS {
        let _ = store::secret_delete(&format!("{prefix}/{id}"));
    }
    let mut devices = store::load_devices(&d).map_err(err)?;
    let mut changed = false;
    for dev in devices.iter_mut() {
        if dev.credential_id == id {
            dev.credential_id.clear();
            changed = true;
        }
    }
    if changed {
        store::save_devices(&d, &devices).map_err(err)?;
    }
    Ok(locker)
}

/// What a device will log in with: the profile name and where it came from, never the secret.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effective {
    pub credential_id: String,
    pub name: String,
    /// "device" | "site" | "customer" | "own" | "none"
    pub via: String,
    pub username: String,
    pub has_password: bool,
}

#[tauri::command]
pub fn credential_effective(app: AppHandle, device_id: String) -> CmdResult<Effective> {
    let d = dir(&app)?;
    let devices = store::load_devices(&d).map_err(err)?;
    let locker = load(&d).map_err(err)?;
    let Some(dev) = devices.iter().find(|x| x.id == device_id) else { return Err("unknown device".into()) };
    if let Some(c) = credential_for(&locker, dev) {
        let via = if !dev.credential_id.is_empty() { "device" } else if locker.defaults.iter().any(|x| x.scope == "site" && x.credential_id == c.id && x.value.eq_ignore_ascii_case(&dev.site)) { "site" } else { "customer" };
        return Ok(Effective { credential_id: c.id.clone(), name: c.name.clone(), via: via.into(), username: c.username.clone(), has_password: c.has_password });
    }
    let own = store::secret_name("password", &device_id).ok().and_then(|n| store::secret_get(&n)).is_some();
    Ok(Effective { credential_id: String::new(), name: String::new(), via: if own { "own" } else { "none" }.into(), username: dev.username.clone(), has_password: own })
}

/* ------------------------------------------------- encrypted export */

#[derive(Serialize, Deserialize)]
struct ExportSecrets {
    password: Option<String>,
    enable: Option<String>,
    passphrase: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ExportBody {
    credentials: Vec<Credential>,
    defaults: Vec<CredentialDefault>,
    secrets: std::collections::BTreeMap<String, ExportSecrets>,
}

#[derive(Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    kdf: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn derive(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    argon2::Argon2::default().hash_password_into(passphrase.as_bytes(), salt, &mut key).map_err(|e| anyhow!("{e}"))?;
    Ok(key)
}

fn encrypt_export(locker: &Locker, secrets: std::collections::BTreeMap<String, ExportSecrets>, passphrase: &str) -> Result<String> {
    if passphrase.chars().count() < 8 {
        bail!("use an export passphrase of at least 8 characters");
    }
    let body = serde_json::to_vec(&ExportBody { credentials: locker.credentials.clone(), defaults: locker.defaults.clone(), secrets })?;
    let salt: [u8; 16] = rand::random();
    let nonce: [u8; 12] = rand::random();
    let key = derive(passphrase, &salt)?;
    let cipher = Aes256Gcm::new((&key).into());
    let ct = cipher.encrypt(&Nonce::try_from(&nonce[..]).map_err(|_| anyhow!("bad nonce"))?, body.as_slice()).map_err(|_| anyhow!("encryption failed"))?;
    let b = base64::engine::general_purpose::STANDARD;
    Ok(serde_json::to_string_pretty(&ExportFile { format: "omniconsole-credentials".into(), version: 1, kdf: "argon2id".into(), salt: b.encode(salt), nonce: b.encode(nonce), ciphertext: b.encode(ct) })?)
}

fn decrypt_export(text: &str, passphrase: &str) -> Result<ExportBody> {
    let file: ExportFile = serde_json::from_str(text).context("not an OmniConsole credential export")?;
    if file.format != "omniconsole-credentials" {
        bail!("not an OmniConsole credential export");
    }
    let b = base64::engine::general_purpose::STANDARD;
    let key = derive(passphrase, &b.decode(&file.salt)?)?;
    let nonce = b.decode(&file.nonce)?;
    let cipher = Aes256Gcm::new((&key).into());
    let plain = cipher.decrypt(&Nonce::try_from(nonce.as_slice()).map_err(|_| anyhow!("bad nonce"))?, b.decode(&file.ciphertext)?.as_slice()).map_err(|_| anyhow!("wrong passphrase or damaged file"))?;
    Ok(serde_json::from_slice(&plain)?)
}

/// Export profiles with their secrets, encrypted with a passphrase that is
/// not the vault passphrase. The file is useless without it.
#[tauri::command]
pub async fn credentials_export(app: AppHandle, passphrase: String) -> CmdResult<Option<String>> {
    if vault().locked() {
        return Err("the vault is locked".into());
    }
    let d = dir(&app)?;
    let locker = load(&d).map_err(err)?;
    let mut secrets = std::collections::BTreeMap::new();
    for c in &locker.credentials {
        secrets.insert(c.id.clone(), ExportSecrets { password: secret("cred", &c.id), enable: secret("cred-enable", &c.id), passphrase: secret("cred-passphrase", &c.id) });
    }
    let text = encrypt_export(&locker, secrets, &passphrase).map_err(err)?;
    let Some(path) = crate::commands::pick_save(&app, "omniconsole-credentials.enc.json", ("Encrypted credentials", &["json"])).await else {
        return Ok(None);
    };
    std::fs::write(&path, text).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub added: usize,
    pub updated: usize,
    pub defaults: usize,
}

/// Import an encrypted export: profiles are matched by id, secrets go straight into the OS store.
#[tauri::command]
pub async fn credentials_import(app: AppHandle, passphrase: String) -> CmdResult<Option<ImportSummary>> {
    if vault().locked() {
        return Err("the vault is locked".into());
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    use tauri_plugin_dialog::DialogExt;
    app.dialog().file().add_filter("Encrypted credentials", &["json"]).pick_file(move |p| {
        let _ = tx.send(p);
    });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let body = decrypt_export(&text, &passphrase).map_err(err)?;
    let d = dir(&app)?;
    let mut locker = load(&d).map_err(err)?;
    let mut added = 0;
    let mut updated = 0;
    for mut c in body.credentials {
        if uuid::Uuid::parse_str(&c.id).is_err() {
            continue;
        }
        if let Some(s) = body.secrets.get(&c.id) {
            for (value, prefix, flag) in [(&s.password, "cred", &mut c.has_password), (&s.enable, "cred-enable", &mut c.has_enable), (&s.passphrase, "cred-passphrase", &mut c.has_passphrase)] {
                if let Some(v) = value {
                    store::secret_set(&format!("{prefix}/{}", c.id), v).map_err(err)?;
                    *flag = true;
                }
            }
        }
        if let Some(existing) = locker.credentials.iter_mut().find(|x| x.id == c.id) {
            *existing = c;
            updated += 1;
        } else {
            locker.credentials.push(c);
            added += 1;
        }
    }
    let mut defaults = 0;
    for def in body.defaults {
        if !locker.defaults.iter().any(|x| x.scope == def.scope && x.value == def.value) {
            locker.defaults.push(def);
            defaults += 1;
        }
    }
    save(&d, &locker).map_err(err)?;
    Ok(Some(ImportSummary { added, updated, defaults }))
}

/* ------------------------------------------------------------- testing */

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub message: String,
    pub millis: u128,
}

/// Try the profile against a saved SSH device: connect, authenticate, disconnect.
/// Only this profile is tried, never another one.
#[tauri::command]
pub async fn credential_test(app: AppHandle, credential_id: String, device_id: String) -> CmdResult<TestResult> {
    if vault().locked() {
        return Err("the vault is locked".into());
    }
    let d = dir(&app)?;
    let devices = store::load_devices(&d).map_err(err)?;
    let locker = load(&d).map_err(err)?;
    let dev = devices.iter().find(|x| x.id == device_id).ok_or("unknown device")?;
    let c = locker.credentials.iter().find(|x| x.id == credential_id).ok_or("unknown credential")?;
    if dev.protocol != "ssh" {
        return Err("the test needs an SSH device".into());
    }
    let user = if c.domain.is_empty() { c.username.clone() } else { format!("{}\\{}", c.domain, c.username) };
    if user.is_empty() {
        return Err("the profile has no username".into());
    }
    let password = c.has_password.then(|| secret("cred", &c.id)).flatten();
    let passphrase = c.has_passphrase.then(|| secret("cred-passphrase", &c.id)).flatten();
    let started = Instant::now();
    let r = crate::session::ssh::test_login(&d, &dev.host, dev.port, &user, &c.method, &c.key_path, password.as_deref(), passphrase.as_deref()).await;
    Ok(match r {
        Ok(()) => TestResult { ok: true, message: format!("{} accepted {} on {}", dev.name, c.name, dev.host), millis: started.elapsed().as_millis() },
        // the error text comes from the SSH layer, which never includes the password
        Err(e) => TestResult { ok: false, message: format!("{e:#}"), millis: started.elapsed().as_millis() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(site: &str, customer: &str, cred: &str) -> store::Device {
        let mut d: store::Device = serde_json::from_str(r#"{"id":"11111111-1111-4111-8111-111111111111","name":"sw1","protocol":"ssh"}"#).unwrap();
        d.site = site.into();
        d.customer = customer.into();
        d.credential_id = cred.into();
        d
    }

    #[test]
    fn device_then_site_then_customer() {
        let a = Credential { id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(), name: "A".into(), ..Default::default() };
        let b = Credential { id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(), name: "B".into(), ..Default::default() };
        let c = Credential { id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), name: "C".into(), expires_at: Some(1), ..Default::default() };
        let locker = Locker {
            credentials: vec![a.clone(), b.clone(), c.clone()],
            defaults: vec![
                CredentialDefault { scope: "site".into(), value: "HQ".into(), credential_id: b.id.clone() },
                CredentialDefault { scope: "customer".into(), value: "Acme".into(), credential_id: a.id.clone() },
            ],
            ..Default::default()
        };
        assert_eq!(credential_for(&locker, &device("HQ", "Acme", &a.id)).unwrap().name, "A");
        assert_eq!(credential_for(&locker, &device("hq", "Acme", "")).unwrap().name, "B");
        assert_eq!(credential_for(&locker, &device("Branch", "Acme", "")).unwrap().name, "A");
        assert!(credential_for(&locker, &device("Branch", "Other", "")).is_none());
        // an expired temporary profile is ignored
        assert!(credential_for(&locker, &device("Branch", "Other", &c.id)).is_none());
    }

    #[test]
    fn export_roundtrip_needs_the_passphrase() {
        let a = Credential { id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(), name: "A".into(), username: "admin".into(), has_password: true, ..Default::default() };
        let locker = Locker { credentials: vec![a], ..Default::default() };
        let mut secrets = std::collections::BTreeMap::new();
        secrets.insert("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_string(), ExportSecrets { password: Some("hunter22".into()), enable: None, passphrase: None });
        let text = encrypt_export(&locker, secrets, "correct horse battery").unwrap();
        assert!(!text.contains("hunter22"));
        assert!(!text.contains("admin"));
        let body = decrypt_export(&text, "correct horse battery").unwrap();
        assert_eq!(body.secrets["aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"].password.as_deref(), Some("hunter22"));
        assert!(decrypt_export(&text, "wrong").is_err());
    }

    #[test]
    fn verifier_matches_only_its_passphrase() {
        let v = verifier("open sesame 123").unwrap();
        let b = base64::engine::general_purpose::STANDARD;
        let (salt, hash) = v.split_once(':').unwrap();
        let salt = b.decode(salt).unwrap();
        let hash = b.decode(hash).unwrap();
        let mut out = vec![0u8; 32];
        argon2::Argon2::default().hash_password_into(b"open sesame 123", &salt, &mut out).unwrap();
        assert!(ct_eq(&out, &hash));
        argon2::Argon2::default().hash_password_into(b"open sesame 124", &salt, &mut out).unwrap();
        assert!(!ct_eq(&out, &hash));
    }
}
