import { useEffect, useState } from "react";
import { listSerialPorts, secretExists } from "../lib/backend";
import {
  DEFAULT_PORT, ENVIRONMENTS, type AuthMethod, type Device, type Environment, type FlowControl, type Parity,
  type Protocol, type SerialPortInfo,
} from "../lib/types";

export type SecretChange = { kind: "keep" } | { kind: "set"; value: string } | { kind: "delete" };

export interface SecretChanges {
  password: SecretChange;
  passphrase: SecretChange;
}

interface Props {
  device: Device;
  isNew: boolean;
  devices: Device[];
  onSave: (device: Device, secrets: SecretChanges) => void;
  onCancel: () => void;
}

const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];
type Section = "general" | "location" | "login" | "serial" | "session";

function uniq(devices: Device[], key: "customer" | "site" | "building" | "role"): string[] {
  return [...new Set(devices.map((d) => d[key]).filter(Boolean))].sort();
}

export default function DeviceDialog({ device, isNew, devices, onSave, onCancel }: Props) {
  const [d, setD] = useState<Device>(device);
  const [section, setSection] = useState<Section>("general");
  const [tagText, setTagText] = useState(device.tags.join(", "));
  const [password, setPassword] = useState("");
  const [savePassword, setSavePassword] = useState(true);
  const [hadPassword, setHadPassword] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [hadPassphrase, setHadPassphrase] = useState(false);
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!isNew) {
      void secretExists("password", device.id).then((has) => {
        setHadPassword(has);
        setSavePassword(has);
      });
      void secretExists("passphrase", device.id).then(setHadPassphrase);
    }
    void listSerialPorts().then(setPorts).catch(() => setPorts([]));
  }, [device.id, isNew]);

  const set = <K extends keyof Device>(key: K, value: Device[K]) => setD((prev) => ({ ...prev, [key]: value }));

  function setProtocol(p: Protocol) {
    setD((prev) => ({
      ...prev,
      protocol: p,
      // keep a custom port, swap the default one
      port: prev.port === DEFAULT_PORT[prev.protocol] || !prev.port ? DEFAULT_PORT[p] : prev.port,
    }));
  }

  const jumpCandidates = devices.filter((x) => x.protocol === "ssh" && x.id !== d.id && x.jumpDeviceId !== d.id);

  function submit(e: React.FormEvent) {
    e.preventDefault();
    const out: Device = {
      ...d,
      name: d.name.trim(),
      host: d.host.trim(),
      customer: d.customer.trim(),
      site: d.site.trim(),
      building: d.building.trim(),
      rack: d.rack.trim(),
      role: d.role.trim(),
      username: d.username.trim(),
      keyPath: d.keyPath.trim(),
      tags: [...new Set(tagText.split(",").map((t) => t.trim()).filter(Boolean))],
    };
    if (out.protocol === "serial") {
      if (!out.serialPort) {
        setSection("serial");
        return setError("Pick the serial port the console cable is on.");
      }
    } else if (!out.host) {
      setSection("general");
      return setError("Enter the device's host name or IP address.");
    }
    if (out.protocol === "ssh" && out.authMethod === "key" && !out.keyPath) {
      setSection("login");
      return setError("Enter the path of the private key file, or pick another login method.");
    }
    if (!out.name) out.name = out.protocol === "serial" ? out.serialPort : out.host;

    const keepSecrets = out.protocol === "ssh" || out.autoLogin;
    let pw: SecretChange = { kind: "keep" };
    if (!keepSecrets || !savePassword) pw = hadPassword ? { kind: "delete" } : { kind: "keep" };
    else if (password) pw = { kind: "set", value: password };
    let pp: SecretChange = { kind: "keep" };
    if (out.protocol !== "ssh" || out.authMethod !== "key") pp = hadPassphrase ? { kind: "delete" } : { kind: "keep" };
    else if (passphrase) pp = { kind: "set", value: passphrase };
    onSave(out, { password: pw, passphrase: pp });
  }

  const serial = d.protocol === "serial";
  const sections: { id: Section; label: string; show: boolean }[] = [
    { id: "general", label: "General", show: true },
    { id: "location", label: "Location", show: true },
    { id: "login", label: "Login", show: true },
    { id: "serial", label: "Serial", show: serial },
    { id: "session", label: "Session", show: true },
  ];

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <form className="dialog wide" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onCancel()}>
        <h3>{isNew ? "Add device" : `Edit ${device.name}`}</h3>
        <div className="dialog-tabs">
          {sections.filter((s) => s.show).map((s) => (
            <button type="button" key={s.id} className={section === s.id ? "on" : ""} onClick={() => setSection(s.id)}>{s.label}</button>
          ))}
        </div>

        {section === "general" && (
          <div className="form">
            <label>Connection</label>
            <div className="segmented">
              {(["ssh", "telnet", "serial"] as Protocol[]).map((p) => (
                <button type="button" key={p} className={d.protocol === p ? "on" : ""} onClick={() => setProtocol(p)}>
                  {p === "ssh" ? "SSH" : p === "telnet" ? "Telnet" : "Serial"}
                </button>
              ))}
            </div>
            {d.protocol === "telnet" && <div className="hint warn-text">Telnet is unencrypted: passwords and configs cross the network in clear text.</div>}

            <label>Name</label>
            <input autoFocus value={d.name} onChange={(e) => set("name", e.target.value)} placeholder="core-sw1" spellCheck={false} />

            {!serial && (
              <>
                <label>Host</label>
                <div className="row">
                  <input value={d.host} onChange={(e) => set("host", e.target.value)} placeholder="10.0.0.1 or switch.example.net" spellCheck={false} />
                  <input style={{ width: 80 }} type="number" min={1} max={65535} value={d.port || ""} onChange={(e) => set("port", parseInt(e.target.value, 10) || 0)} title="Port" />
                </div>
              </>
            )}

            <label>Environment</label>
            <div className="row">
              <select value={d.environment} onChange={(e) => set("environment", e.target.value as Environment)}>
                {ENVIRONMENTS.map((env) => <option key={env.value} value={env.value}>{env.label}</option>)}
              </select>
              <input type="color" className="color" value={d.color || ENVIRONMENTS.find((x) => x.value === d.environment)?.color || "#7aa2f7"} onChange={(e) => set("color", e.target.value)} title="Tab color" />
              {d.color && <button type="button" className="btn" onClick={() => set("color", "")}>Default</button>}
            </div>
            <div className="hint">Production sessions get a red tab and a warning before risky actions.</div>

            <label>Tags</label>
            <input value={tagText} onChange={(e) => setTagText(e.target.value)} placeholder="core, stack, poe (comma separated)" spellCheck={false} />

            <label>Notes</label>
            <textarea value={d.notes} onChange={(e) => set("notes", e.target.value)} placeholder="Model, serial number, anything useful" />
          </div>
        )}

        {section === "location" && (
          <div className="form">
            <label>Customer</label>
            <input list="dl-customer" value={d.customer} onChange={(e) => set("customer", e.target.value)} placeholder="Optional" />
            <label>Site</label>
            <input list="dl-site" value={d.site} onChange={(e) => set("site", e.target.value)} placeholder="Optional" />
            <label>Building</label>
            <input list="dl-building" value={d.building} onChange={(e) => set("building", e.target.value)} placeholder="Optional" />
            <label>Rack</label>
            <input value={d.rack} onChange={(e) => set("rack", e.target.value)} placeholder="e.g. MDF R2 U14" />
            <label>Device role</label>
            <input list="dl-role" value={d.role} onChange={(e) => set("role", e.target.value)} placeholder="core, distribution, access, firewall…" />
            <datalist id="dl-customer">{uniq(devices, "customer").map((s) => <option key={s} value={s} />)}</datalist>
            <datalist id="dl-site">{uniq(devices, "site").map((s) => <option key={s} value={s} />)}</datalist>
            <datalist id="dl-building">{uniq(devices, "building").map((s) => <option key={s} value={s} />)}</datalist>
            <datalist id="dl-role">{[...new Set([...uniq(devices, "role"), "core", "distribution", "access", "router", "firewall", "wireless controller"])].map((s) => <option key={s} value={s} />)}</datalist>
          </div>
        )}

        {section === "login" && (
          <div className="form">
            <label>Username</label>
            <input value={d.username} onChange={(e) => set("username", e.target.value)} placeholder="Asked when connecting if empty" spellCheck={false} />

            {d.protocol === "ssh" && (
              <>
                <label>Log in with</label>
                <div className="segmented">
                  {(["password", "key", "agent"] as AuthMethod[]).map((m) => (
                    <button type="button" key={m} className={d.authMethod === m ? "on" : ""} onClick={() => set("authMethod", m)}>
                      {m === "password" ? "Password" : m === "key" ? "Key file" : "SSH agent"}
                    </button>
                  ))}
                </div>
                {d.authMethod === "agent" && <div className="hint">Uses ssh-agent (macOS, Linux) or the OpenSSH agent / Pageant (Windows). Falls back to the password.</div>}
                {d.authMethod === "key" && (
                  <>
                    <label>Private key</label>
                    <input value={d.keyPath} onChange={(e) => set("keyPath", e.target.value)} placeholder="~/.ssh/id_ed25519" spellCheck={false} />
                    <label>Passphrase</label>
                    <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} placeholder={hadPassphrase ? "Saved in the keychain, leave empty to keep" : "Asked when connecting if the key has one"} />
                  </>
                )}
              </>
            )}

            {d.protocol !== "ssh" && (
              <label className="check">
                <input type="checkbox" checked={d.autoLogin} onChange={(e) => set("autoLogin", e.target.checked)} />
                Log in automatically at the Username and Password prompts
              </label>
            )}
            {(d.protocol === "ssh" || d.autoLogin) && (
              <>
                <label>Password</label>
                <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={hadPassword ? "Saved, leave empty to keep it" : "Asked when connecting if empty"} disabled={!savePassword} />
                <label className="check">
                  <input type="checkbox" checked={savePassword} onChange={(e) => setSavePassword(e.target.checked)} />
                  Save password in the system keychain
                </label>
              </>
            )}
            {d.protocol === "telnet" && !d.autoLogin && <div className="hint">Telnet logs in at the device's own prompts; turn on automatic login to answer them with the saved username and password.</div>}

            {d.protocol === "ssh" && (
              <>
                <label>Jump host</label>
                <select value={d.jumpDeviceId} onChange={(e) => set("jumpDeviceId", e.target.value)}>
                  <option value="">None, connect directly</option>
                  {jumpCandidates.map((j) => <option key={j.id} value={j.id}>{j.name} ({j.host})</option>)}
                </select>
                <div className="hint">Connect through another saved SSH device, such as a bastion host.</div>
                <label>Keepalive</label>
                <select value={d.keepalive} onChange={(e) => set("keepalive", parseInt(e.target.value, 10))}>
                  {[0, 10, 15, 30, 60, 120].map((s) => <option key={s} value={s}>{s === 0 ? "Off" : `Every ${s} seconds`}</option>)}
                </select>
              </>
            )}
          </div>
        )}

        {section === "serial" && serial && (
          <div className="form">
            <label>Serial port</label>
            <div className="row">
              <input list="serial-ports" value={d.serialPort} onChange={(e) => set("serialPort", e.target.value)} placeholder={ports[0]?.name ?? "COM3 or /dev/ttyUSB0"} spellCheck={false} />
              <button type="button" className="btn" onClick={() => void listSerialPorts().then(setPorts)}>Refresh</button>
            </div>
            <datalist id="serial-ports">{ports.map((p) => <option key={p.name} value={p.name}>{p.description}</option>)}</datalist>
            {ports.length === 0 && <div className="hint">No serial ports found. Plug in the console cable and press Refresh.</div>}
            {ports.length > 0 && <div className="hint">{ports.map((p) => `${p.name}${p.description ? ` (${p.description})` : ""}`).join(", ")}</div>}
            <label>Speed</label>
            <select value={d.baud} onChange={(e) => set("baud", parseInt(e.target.value, 10))}>
              {[...new Set([...BAUDS, d.baud])].sort((a, b) => a - b).map((b) => <option key={b} value={b}>{b} baud</option>)}
            </select>
            <label>Data bits</label>
            <select value={d.dataBits} onChange={(e) => set("dataBits", parseInt(e.target.value, 10))}>
              {[8, 7, 6, 5].map((b) => <option key={b} value={b}>{b}</option>)}
            </select>
            <label>Parity</label>
            <select value={d.parity} onChange={(e) => set("parity", e.target.value as Parity)}>
              <option value="none">None</option>
              <option value="even">Even</option>
              <option value="odd">Odd</option>
            </select>
            <label>Stop bits</label>
            <select value={d.stopBits} onChange={(e) => set("stopBits", parseInt(e.target.value, 10))}>
              <option value={1}>1</option>
              <option value={2}>2</option>
            </select>
            <label>Flow control</label>
            <select value={d.flowControl} onChange={(e) => set("flowControl", e.target.value as FlowControl)}>
              <option value="none">None</option>
              <option value="software">XON/XOFF (software)</option>
              <option value="hardware">RTS/CTS (hardware)</option>
            </select>
            <div className="hint">Cisco consoles use 9600 8N1 with no flow control unless someone changed it.</div>
          </div>
        )}

        {section === "session" && (
          <div className="form">
            <label className="check">
              <input type="checkbox" checked={d.autoReconnect} onChange={(e) => set("autoReconnect", e.target.checked)} />
              Reconnect automatically when the connection drops
            </label>
            <label className="check">
              <input type="checkbox" checked={d.logSession} onChange={(e) => set("logSession", e.target.checked)} />
              Always log this device's sessions to a file
            </label>
            <label className="check">
              <input type="checkbox" checked={d.favorite} onChange={(e) => set("favorite", e.target.checked)} />
              Show in Favorites
            </label>
          </div>
        )}

        {error && <p style={{ color: "var(--bad)", marginTop: 12 }}>{error}</p>}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>Cancel</button>
          <button type="submit" className="btn primary">{isNew ? "Add device" : "Save"}</button>
        </div>
      </form>
    </div>
  );
}
