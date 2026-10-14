import { useEffect, useState } from "react";
import { listSerialPorts, passwordExists } from "../lib/backend";
import { DEFAULT_PORT, type Device, type Protocol, type SerialPortInfo } from "../lib/types";

export type PasswordChange = { kind: "keep" } | { kind: "set"; value: string } | { kind: "delete" };

interface Props {
  device: Device;
  isNew: boolean;
  sites: string[];
  onSave: (device: Device, password: PasswordChange) => void;
  onCancel: () => void;
}

const BAUDS = [9600, 19200, 38400, 57600, 115200];

export default function DeviceDialog({ device, isNew, sites, onSave, onCancel }: Props) {
  const [d, setD] = useState<Device>(device);
  const [password, setPassword] = useState("");
  const [savePassword, setSavePassword] = useState(true);
  const [hadPassword, setHadPassword] = useState(false);
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!isNew) {
      void passwordExists(device.id).then((has) => {
        setHadPassword(has);
        setSavePassword(has);
      });
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

  function submit(e: React.FormEvent) {
    e.preventDefault();
    const out: Device = { ...d, name: d.name.trim(), host: d.host.trim(), site: d.site.trim(), username: d.username.trim() };
    if (out.protocol === "serial") {
      if (!out.serialPort) return setError("Pick the serial port the console cable is on.");
    } else if (!out.host) {
      return setError("Enter the device's host name or IP address.");
    }
    if (!out.name) out.name = out.protocol === "serial" ? out.serialPort : out.host;
    let change: PasswordChange = { kind: "keep" };
    if (out.protocol !== "ssh" || !savePassword) change = hadPassword ? { kind: "delete" } : { kind: "keep" };
    else if (password) change = { kind: "set", value: password };
    onSave(out, change);
  }

  const serial = d.protocol === "serial";
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <form className="dialog" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && onCancel()}>
        <h3>{isNew ? "Add device" : "Edit device"}</h3>
        <div className="form">
          <label>Connection</label>
          <div className="segmented">
            {(["ssh", "telnet", "serial"] as Protocol[]).map((p) => (
              <button type="button" key={p} className={d.protocol === p ? "on" : ""} onClick={() => setProtocol(p)}>
                {p === "ssh" ? "SSH" : p === "telnet" ? "Telnet" : "Serial"}
              </button>
            ))}
          </div>

          <label>Name</label>
          <input autoFocus value={d.name} onChange={(e) => set("name", e.target.value)} placeholder="core-sw1" spellCheck={false} />

          <label>Site</label>
          <input list="sites" value={d.site} onChange={(e) => set("site", e.target.value)} placeholder="Optional, groups the sidebar" />
          <datalist id="sites">{sites.map((s) => <option key={s} value={s} />)}</datalist>

          {!serial && (
            <>
              <label>Host</label>
              <div className="row">
                <input value={d.host} onChange={(e) => set("host", e.target.value)} placeholder="10.0.0.1 or switch.example.net" spellCheck={false} />
                <input
                  style={{ width: 74 }}
                  type="number"
                  min={1}
                  max={65535}
                  value={d.port || ""}
                  onChange={(e) => set("port", parseInt(e.target.value, 10) || 0)}
                  title="Port"
                />
              </div>
            </>
          )}

          {serial && (
            <>
              <label>Serial port</label>
              <div className="row">
                <input
                  list="serial-ports"
                  value={d.serialPort}
                  onChange={(e) => set("serialPort", e.target.value)}
                  placeholder={ports[0]?.name ?? "COM3 or /dev/ttyUSB0"}
                  spellCheck={false}
                />
                <button type="button" className="btn" onClick={() => void listSerialPorts().then(setPorts)}>Refresh</button>
              </div>
              <datalist id="serial-ports">
                {ports.map((p) => <option key={p.name} value={p.name}>{p.description}</option>)}
              </datalist>
              {ports.length === 0 && <div className="hint">No serial ports found. Plug in the console cable and press Refresh.</div>}
              <label>Speed</label>
              <select value={d.baud} onChange={(e) => set("baud", parseInt(e.target.value, 10))}>
                {[...new Set([...BAUDS, d.baud])].sort((a, b) => a - b).map((b) => (
                  <option key={b} value={b}>{b} baud</option>
                ))}
              </select>
            </>
          )}

          {d.protocol === "ssh" && (
            <>
              <label>Username</label>
              <input value={d.username} onChange={(e) => set("username", e.target.value)} placeholder="Asked when connecting if empty" spellCheck={false} />
              <label>Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={hadPassword ? "Saved, leave empty to keep it" : "Asked when connecting if empty"}
              />
              <label className="check">
                <input type="checkbox" checked={savePassword} onChange={(e) => setSavePassword(e.target.checked)} />
                Save password in the system keychain
              </label>
            </>
          )}
          {d.protocol === "telnet" && <div className="hint">Telnet logs in at the device's own prompts.</div>}

          <label>Notes</label>
          <textarea value={d.notes} onChange={(e) => set("notes", e.target.value)} placeholder="Rack, model, anything useful" />
        </div>
        {error && <p style={{ color: "var(--bad)", marginTop: 12 }}>{error}</p>}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>Cancel</button>
          <button type="submit" className="btn primary">{isNew ? "Add device" : "Save"}</button>
        </div>
      </form>
    </div>
  );
}
