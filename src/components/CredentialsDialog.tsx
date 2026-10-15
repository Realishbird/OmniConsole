import { useEffect, useMemo, useState } from "react";
import {
  copySecret, deleteCredential, exportCredentials, expiryLabel, getCredentialSecret, importCredentials, newCredential,
  setCredentialSecret, testCredential, usedBy, vaultLock, vaultSetup, type Credential, type CredentialDefault, type CredMethod,
  type CredSecretKind, type Locker, type TestResult, type VaultStatus,
} from "../lib/credentials";
import type { Device } from "../lib/types";
import { CopyIcon, EyeIcon } from "./Icons";

interface Props {
  locker: Locker;
  vault: VaultStatus;
  devices: Device[];
  onChange: (locker: Locker) => void;
  onVault: (status: VaultStatus) => void;
  onToast: (text: string) => void;
  /** the vault is locked: ask for the passphrase, resolve true when unlocked */
  onUnlock: () => Promise<boolean>;
  onClose: () => void;
}

type Tab = "profiles" | "defaults" | "security";
const METHOD_LABEL: Record<CredMethod, string> = { password: "Password", key: "Key file", agent: "SSH agent" };
const EXPIRY_CHOICES: { label: string; hours: number }[] = [{ label: "Permanent", hours: 0 }, { label: "4 hours", hours: 4 }, { label: "1 day", hours: 24 }, { label: "1 week", hours: 168 }, { label: "30 days", hours: 720 }];

/** Named login profiles, their defaults per customer and site, and the vault that guards them. */
export default function CredentialsDialog({ locker, vault, devices, onChange, onVault, onToast, onUnlock, onClose }: Props) {
  const [tab, setTab] = useState<Tab>("profiles");
  const [selected, setSelected] = useState<string | null>(locker.credentials[0]?.id ?? null);
  const [filter, setFilter] = useState("");
  const [tagText, setTagText] = useState("");
  const current = locker.credentials.find((c) => c.id === selected) ?? null;

  useEffect(() => setTagText(current?.tags.join(", ") ?? ""), [current?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const shown = useMemo(() => {
    const f = filter.toLowerCase();
    return locker.credentials
      .filter((c) => !f || `${c.name} ${c.username} ${c.domain} ${c.tags.join(" ")} ${c.notes}`.toLowerCase().includes(f))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [locker.credentials, filter]);

  async function ensureUnlocked(): Promise<boolean> {
    if (!vault.locked) return true;
    return onUnlock();
  }

  function patch(id: string, p: Partial<Credential>) {
    onChange({ ...locker, credentials: locker.credentials.map((c) => (c.id === id ? { ...c, ...p, updated: Date.now() } : c)) });
  }

  function add() {
    const c = newCredential();
    onChange({ ...locker, credentials: [...locker.credentials, c] });
    setSelected(c.id);
    setTab("profiles");
  }

  function duplicate() {
    if (!current) return;
    const c: Credential = { ...current, id: crypto.randomUUID(), name: `${current.name} copy`, hasPassword: false, hasEnable: false, hasPassphrase: false, updated: Date.now() };
    onChange({ ...locker, credentials: [...locker.credentials, c] });
    setSelected(c.id);
    onToast("Duplicated without its secrets; enter them for the copy");
  }

  async function remove() {
    if (!current) return;
    const users = usedBy(locker, current, devices);
    if (!window.confirm(`Delete the profile "${current.name}" and its stored secrets?${users.length ? ` ${users.length} device${users.length === 1 ? "" : "s"} will fall back to defaults or ask for a password.` : ""}`)) return;
    try {
      const next = await deleteCredential(current.id);
      onChange(next);
      setSelected(next.credentials[0]?.id ?? null);
    } catch (e) {
      onToast(String(e));
    }
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog xwide creds" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h3>Credentials {vault.configured && <span className={`vault-pill${vault.locked ? " locked" : ""}`}>{vault.locked ? "Vault locked" : "Vault unlocked"}</span>}</h3>
        <div className="dialog-tabs">
          {(["profiles", "defaults", "security"] as Tab[]).map((t) => (
            <button type="button" key={t} className={tab === t ? "on" : ""} onClick={() => setTab(t)}>{t === "profiles" ? `Profiles (${locker.credentials.length})` : t === "defaults" ? "Defaults" : "Security"}</button>
          ))}
        </div>

        {tab === "profiles" && (
          <div className="split-editor">
            <div className="split-list">
              <input className="search" placeholder="Search profiles" value={filter} onChange={(e) => setFilter(e.target.value)} spellCheck={false} />
              <div className="list">
                {shown.map((c) => (
                  <div key={c.id} className={`list-row${c.id === selected ? " selected" : ""}`} onClick={() => setSelected(c.id)}>
                    <span className="name">{c.name}{c.expiresAt ? <span className="cred-badge">{expiryLabel(c)}</span> : null}</span>
                    <span className="target">{c.domain ? `${c.domain}\\` : ""}{c.username || "(no username)"} · {METHOD_LABEL[c.method]}{c.tags.length ? ` · ${c.tags.join(", ")}` : ""}</span>
                  </div>
                ))}
                {locker.credentials.length === 0 && <div className="sidebar-empty">No profiles yet. A profile is a username and password (or key) that many devices share; devices point at it, so a password change is one edit.</div>}
              </div>
              <button className="btn" onClick={add}>New profile</button>
            </div>
            <div className="split-body">
              {current ? (
                <ProfileForm
                  key={current.id}
                  c={current}
                  locker={locker}
                  devices={devices}
                  tagText={tagText}
                  onTagText={setTagText}
                  vaultLocked={vault.locked}
                  ensureUnlocked={ensureUnlocked}
                  onPatch={(p) => patch(current.id, p)}
                  onToast={onToast}
                />
              ) : (
                <div className="sidebar-empty">Pick or create a profile.</div>
              )}
            </div>
          </div>
        )}

        {tab === "defaults" && <DefaultsTab locker={locker} devices={devices} onChange={onChange} />}

        {tab === "security" && <SecurityTab locker={locker} vault={vault} onChange={onChange} onVault={onVault} onToast={onToast} ensureUnlocked={ensureUnlocked} />}

        <div className="dialog-actions">
          {tab === "profiles" && current && (
            <>
              <button className="btn danger left" onClick={() => void remove()}>Delete</button>
              <button className="btn" onClick={duplicate}>Duplicate</button>
            </>
          )}
          <button className="btn primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ profile */

function ProfileForm({ c, locker, devices, tagText, onTagText, vaultLocked, ensureUnlocked, onPatch, onToast }: {
  c: Credential;
  locker: Locker;
  devices: Device[];
  tagText: string;
  onTagText: (t: string) => void;
  vaultLocked: boolean;
  ensureUnlocked: () => Promise<boolean>;
  onPatch: (p: Partial<Credential>) => void;
  onToast: (t: string) => void;
}) {
  const users = usedBy(locker, c, devices);
  const sshDevices = devices.filter((d) => d.protocol === "ssh");
  const [testDevice, setTestDevice] = useState(users.find((u) => u.device.protocol === "ssh")?.device.id ?? sshDevices[0]?.id ?? "");
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<TestResult | null>(null);

  async function runTest() {
    if (!testDevice || !(await ensureUnlocked())) return;
    setTesting(true);
    setTest(null);
    try {
      setTest(await testCredential(c.id, testDevice));
    } catch (e) {
      setTest({ ok: false, message: String(e), millis: 0 });
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className="form cred-form">
      <label>Name</label>
      <input value={c.name} onChange={(e) => onPatch({ name: e.target.value })} placeholder="Acme network admin" spellCheck={false} />
      <label>Username</label>
      <div className="row">
        <input value={c.username} onChange={(e) => onPatch({ username: e.target.value })} placeholder="admin" spellCheck={false} autoComplete="off" />
        <input style={{ width: 130 }} value={c.domain} onChange={(e) => onPatch({ domain: e.target.value })} placeholder="Domain (optional)" spellCheck={false} title="Sent as DOMAIN\\user" />
      </div>
      <label>Log in with</label>
      <div className="segmented">
        {(["password", "key", "agent"] as CredMethod[]).map((m) => (
          <button type="button" key={m} className={c.method === m ? "on" : ""} onClick={() => onPatch({ method: m })}>{METHOD_LABEL[m]}</button>
        ))}
      </div>
      {c.method === "key" && (
        <>
          <label>Private key</label>
          <input value={c.keyPath} onChange={(e) => onPatch({ keyPath: e.target.value })} placeholder="~/.ssh/id_ed25519" spellCheck={false} />
          <label>Passphrase</label>
          <SecretField kind="cred-passphrase" c={c} has={c.hasPassphrase} locker={locker} vaultLocked={vaultLocked} ensureUnlocked={ensureUnlocked} onPatch={onPatch} onToast={onToast} />
        </>
      )}
      <label>Password</label>
      <SecretField kind="cred-password" c={c} has={c.hasPassword} locker={locker} vaultLocked={vaultLocked} ensureUnlocked={ensureUnlocked} onPatch={onPatch} onToast={onToast} />
      {c.method !== "password" && <div className="hint">Used when the key or agent is refused and the device falls back to a password prompt.</div>}
      <label>Enable secret</label>
      <SecretField kind="cred-enable" c={c} has={c.hasEnable} locker={locker} vaultLocked={vaultLocked} ensureUnlocked={ensureUnlocked} onPatch={onPatch} onToast={onToast} />
      <div className="hint">Kept for copy and reveal; the app never types it on its own.</div>
      <label>Tags</label>
      <input value={tagText} onChange={(e) => onTagText(e.target.value)} onBlur={() => onPatch({ tags: [...new Set(tagText.split(",").map((t) => t.trim()).filter(Boolean))] })} placeholder="customer, lab, read-only (comma separated)" spellCheck={false} />
      <label>Valid for</label>
      <div className="row">
        <select value={c.expiresAt ? "custom" : "0"} onChange={(e) => { const h = Number(e.target.value); onPatch({ expiresAt: h > 0 ? Date.now() + h * 3_600_000 : null }); }}>
          {EXPIRY_CHOICES.map((x) => <option key={x.hours} value={x.hours}>{x.label}</option>)}
          {c.expiresAt && <option value="custom">{expiryLabel(c)}</option>}
        </select>
        <span className="quiet small">{c.expiresAt ? "Temporary: the secrets are deleted when it expires" : "Secrets stay until you delete them"}</span>
      </div>
      <label>Notes</label>
      <textarea value={c.notes} onChange={(e) => onPatch({ notes: e.target.value })} placeholder="Who owns this account, rotation schedule, ticket" />
      <label>Used by</label>
      <div className="cred-users">
        {users.length === 0 && <span className="quiet">No device yet. Pick it in a device's Login tab, or set it as a default.</span>}
        {users.slice(0, 12).map((u) => <span key={u.device.id} className="cred-user" title={u.via === "device" ? "set on the device" : `${u.via} default`}>{u.device.name}<small>{u.via === "device" ? "" : ` (${u.via})`}</small></span>)}
        {users.length > 12 && <span className="quiet">and {users.length - 12} more</span>}
      </div>
      <label>Test</label>
      <div className="row">
        <select value={testDevice} onChange={(e) => setTestDevice(e.target.value)}>
          <option value="">Pick an SSH device</option>
          {sshDevices.map((d) => <option key={d.id} value={d.id}>{d.name} ({d.host})</option>)}
        </select>
        <button type="button" className="btn" disabled={!testDevice || testing || (c.method !== "agent" && !c.hasPassword && !c.hasPassphrase && c.method === "password")} onClick={() => void runTest()}>{testing ? "Testing…" : "Test login"}</button>
      </div>
      {test && <div className={`hint ${test.ok ? "ok-text" : "warn-text"}`}>{test.message}{test.millis ? ` (${test.millis} ms)` : ""}</div>}
      <div className="hint">Logs in once with this profile only and disconnects. The device's host key must already be trusted.</div>
      <label>Updated</label>
      <span className="quiet">{new Date(c.updated).toLocaleString()}</span>
    </div>
  );
}

/** Masked secret entry with save, reveal and copy. The value is only in this field while typing. */
function SecretField({ kind, c, has, locker, vaultLocked, ensureUnlocked, onPatch, onToast }: {
  kind: CredSecretKind;
  c: Credential;
  has: boolean;
  locker: Locker;
  vaultLocked: boolean;
  ensureUnlocked: () => Promise<boolean>;
  onPatch: (p: Partial<Credential>) => void;
  onToast: (t: string) => void;
}) {
  const [value, setValue] = useState("");
  const [shown, setShown] = useState<string | null>(null);
  const flag: keyof Credential = kind === "cred-password" ? "hasPassword" : kind === "cred-enable" ? "hasEnable" : "hasPassphrase";

  useEffect(() => {
    if (!shown) return;
    const t = window.setTimeout(() => setShown(null), 15_000);
    return () => window.clearTimeout(t);
  }, [shown]);

  async function save() {
    if (!(await ensureUnlocked())) return;
    try {
      await setCredentialSecret(kind, c.id, value);
      onPatch({ [flag]: value.length > 0 } as Partial<Credential>);
      setValue("");
      setShown(null);
      onToast(value ? "Saved to the OS credential store" : "Removed from the OS credential store");
    } catch (e) {
      onToast(String(e));
    }
  }

  async function reveal() {
    if (shown !== null) return setShown(null);
    if (!(await ensureUnlocked())) return;
    try {
      setShown((await getCredentialSecret(kind, c.id)) ?? "");
    } catch (e) {
      onToast(String(e));
    }
  }

  async function copy() {
    if (!(await ensureUnlocked())) return;
    try {
      const v = await getCredentialSecret(kind, c.id);
      if (v === null) return onToast("Nothing stored");
      await copySecret(v, locker.clipboardClearSeconds);
      onToast(locker.clipboardClearSeconds ? `Copied; the clipboard clears in ${locker.clipboardClearSeconds} s` : "Copied");
    } catch (e) {
      onToast(String(e));
    }
  }

  return (
    <div className="secret-field">
      {shown !== null ? (
        <code className="secret-shown" onClick={() => setShown(null)} title="Click to hide">{shown || "(empty)"}</code>
      ) : (
        <input type="password" value={value} onChange={(e) => setValue(e.target.value)} placeholder={has ? (vaultLocked ? "Stored (vault locked)" : "Stored; type to replace") : "Not stored"} autoComplete="new-password" onKeyDown={(e) => e.key === "Enter" && value && void save()} />
      )}
      {value && <button type="button" className="btn small primary" onClick={() => void save()}>Save</button>}
      {has && !value && (
        <>
          <button type="button" className="icon-btn" title={shown !== null ? "Hide" : "Reveal for 15 seconds"} onClick={() => void reveal()}><EyeIcon /></button>
          <button type="button" className="icon-btn" title="Copy (the clipboard is cleared afterwards)" onClick={() => void copy()}><CopyIcon /></button>
          <button type="button" className="btn small" title="Remove the stored value" onClick={() => { setValue(""); void (async () => { if (await ensureUnlocked()) { await setCredentialSecret(kind, c.id, "").catch((e) => onToast(String(e))); onPatch({ [flag]: false } as Partial<Credential>); } })(); }}>Clear</button>
        </>
      )}
    </div>
  );
}

/* ----------------------------------------------------------- defaults */

function DefaultsTab({ locker, devices, onChange }: { locker: Locker; devices: Device[]; onChange: (l: Locker) => void }) {
  const customers = [...new Set(devices.map((d) => d.customer).filter(Boolean))].sort();
  const sites = [...new Set(devices.map((d) => d.site).filter(Boolean))].sort();
  const get = (scope: CredentialDefault["scope"], value: string) => locker.defaults.find((d) => d.scope === scope && d.value.toLowerCase() === value.toLowerCase())?.credentialId ?? "";
  function set(scope: CredentialDefault["scope"], value: string, credentialId: string) {
    const rest = locker.defaults.filter((d) => !(d.scope === scope && d.value.toLowerCase() === value.toLowerCase()));
    onChange({ ...locker, defaults: credentialId ? [...rest, { scope, value, credentialId }] : rest });
  }
  const options = locker.credentials.slice().sort((a, b) => a.name.localeCompare(b.name));
  const count = (scope: CredentialDefault["scope"], value: string) => devices.filter((d) => !d.credentialId && (scope === "site" ? d.site : d.customer).toLowerCase() === value.toLowerCase()).length;
  return (
    <div className="cred-defaults">
      <p>A device without a profile of its own uses its site's default, then its customer's. Devices with their own saved password keep it when no default applies.</p>
      {sites.length > 0 && (
        <>
          <div className="menu-label">Sites</div>
          {sites.map((s) => (
            <div key={s} className="cred-default-row">
              <span>{s}</span>
              <small className="quiet">{count("site", s)} device{count("site", s) === 1 ? "" : "s"} without a profile</small>
              <select value={get("site", s)} onChange={(e) => set("site", s, e.target.value)}>
                <option value="">No default</option>
                {options.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </div>
          ))}
        </>
      )}
      {customers.length > 0 && (
        <>
          <div className="menu-label">Customers</div>
          {customers.map((s) => (
            <div key={s} className="cred-default-row">
              <span>{s}</span>
              <small className="quiet">{count("customer", s)} device{count("customer", s) === 1 ? "" : "s"} without a profile</small>
              <select value={get("customer", s)} onChange={(e) => set("customer", s, e.target.value)}>
                <option value="">No default</option>
                {options.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </div>
          ))}
        </>
      )}
      {sites.length === 0 && customers.length === 0 && <div className="sidebar-empty">Give devices a site or customer (Location tab) to set defaults for them.</div>}
    </div>
  );
}

/* ----------------------------------------------------------- security */

function SecurityTab({ locker, vault, onChange, onVault, onToast, ensureUnlocked }: {
  locker: Locker;
  vault: VaultStatus;
  onChange: (l: Locker) => void;
  onVault: (v: VaultStatus) => void;
  onToast: (t: string) => void;
  ensureUnlocked: () => Promise<boolean>;
}) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [exportPass, setExportPass] = useState("");
  const [error, setError] = useState("");

  async function setPassphrase(remove = false) {
    setError("");
    if (!remove && next !== again) return setError("The two passphrases differ.");
    if (!remove && next.length < 8) return setError("Use at least 8 characters.");
    try {
      onVault(await vaultSetup(vault.configured ? current : null, remove ? "" : next));
      setCurrent("");
      setNext("");
      setAgain("");
      onToast(remove ? "Vault lock removed" : "Vault passphrase set");
    } catch (e) {
      setError(String(e));
    }
  }

  async function doExport() {
    if (!(await ensureUnlocked())) return;
    try {
      const p = await exportCredentials(exportPass);
      if (p) onToast(`Encrypted export saved: ${p}`);
      setExportPass("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function doImport() {
    if (!(await ensureUnlocked())) return;
    try {
      const r = await importCredentials(exportPass);
      if (r) onToast(`Imported ${r.added} new, ${r.updated} updated profile(s), ${r.defaults} default(s)`);
      setExportPass("");
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="form cred-form" style={{ gridTemplateColumns: "150px 1fr" }}>
      <div className="hint" style={{ gridColumn: "1 / -1", marginTop: 0 }}>Secrets live in the operating system's credential store (Credential Manager, Keychain, Secret Service), never in OmniConsole's files, logs, exports, reports or AI requests. The vault lock stops the app from reading them until you unlock it.</div>

      <label>Vault passphrase</label>
      <div className="cred-stack">
        {vault.configured && <input type="password" value={current} onChange={(e) => setCurrent(e.target.value)} placeholder="Current passphrase" autoComplete="current-password" />}
        <input type="password" value={next} onChange={(e) => setNext(e.target.value)} placeholder={vault.configured ? "New passphrase" : "Passphrase (8+ characters)"} autoComplete="new-password" />
        <input type="password" value={again} onChange={(e) => setAgain(e.target.value)} placeholder="Repeat it" autoComplete="new-password" />
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn primary" disabled={!next} onClick={() => void setPassphrase()}>{vault.configured ? "Change" : "Set passphrase"}</button>
          {vault.configured && <button type="button" className="btn" disabled={!current} onClick={() => void setPassphrase(true)}>Remove lock</button>}
          {vault.configured && !vault.locked && <button type="button" className="btn" onClick={() => void vaultLock().then(onVault)}>Lock now</button>}
        </div>
      </div>
      <div className="hint">{vault.configured ? "While locked, connections ask for the password and reveal, copy, export and test are refused." : "No passphrase set: secrets are readable whenever the OS session is unlocked."}</div>

      <label>Auto-lock</label>
      <select value={locker.autoLockMinutes} onChange={(e) => onChange({ ...locker, autoLockMinutes: Number(e.target.value) })} disabled={!vault.configured}>
        {[0, 5, 15, 30, 60, 240].map((m) => <option key={m} value={m}>{m === 0 ? "Never" : `After ${m} minutes idle`}</option>)}
      </select>

      <label>Clipboard</label>
      <select value={locker.clipboardClearSeconds} onChange={(e) => onChange({ ...locker, clipboardClearSeconds: Number(e.target.value) })}>
        {[0, 10, 20, 30, 60].map((s) => <option key={s} value={s}>{s === 0 ? "Keep copied secrets" : `Clear copied secrets after ${s} s`}</option>)}
      </select>

      <label>Login attempts</label>
      <select value={locker.retryLimit} onChange={(e) => onChange({ ...locker, retryLimit: Number(e.target.value) })}>
        {[1, 2, 3, 5].map((n) => <option key={n} value={n}>{n} password prompt{n === 1 ? "" : "s"} per connection</option>)}
      </select>
      <div className="hint">A rejected stored password is never retried with another profile; the connection asks you instead.</div>

      <label>Encrypted export</label>
      <div className="cred-stack">
        <input type="password" value={exportPass} onChange={(e) => setExportPass(e.target.value)} placeholder="Export passphrase (not the vault passphrase)" autoComplete="off" />
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn" disabled={exportPass.length < 8} onClick={() => void doExport()}>Export profiles…</button>
          <button type="button" className="btn" disabled={!exportPass} onClick={() => void doImport()}>Import…</button>
        </div>
      </div>
      <div className="hint">AES-256-GCM with an argon2id key from the passphrase. The device export never contains secrets; this one does, encrypted.</div>
      {error && <div className="hint warn-text">{error}</div>}
    </div>
  );
}
