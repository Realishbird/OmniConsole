import { useEffect, useState } from "react";
import { logFolder, openLogFolder } from "../lib/backend";
import { configFolder, openConfigFolder } from "../lib/tools";
import { DEFAULT_SETTINGS, FONT_CHOICES, type AnsiKey, type CursorStyle, type Density, type Settings } from "../lib/settings";
import { THEMES, themeById } from "../lib/themes";

interface Props {
  settings: Settings;
  onChange: (s: Settings) => void;
  onClose: () => void;
}

const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;

type Section = "appearance" | "terminal" | "colors" | "logging";

/** Changes apply as you make them, so you can watch the terminal change. */
export default function SettingsDialog({ settings, onChange, onClose }: Props) {
  const [section, setSection] = useState<Section>("appearance");
  const [folder, setFolder] = useState("");
  const [configs, setConfigs] = useState("");
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => onChange({ ...settings, [key]: value });
  const theme = themeById(settings.theme);

  useEffect(() => {
    void logFolder().then(setFolder);
  }, [settings.logDir]);
  useEffect(() => {
    void configFolder().then(setConfigs);
  }, [settings.configDir]);

  function ansi(name: string, bright: boolean) {
    const key = (bright ? `bright${name[0].toUpperCase()}${name.slice(1)}` : name) as AnsiKey;
    const value = (settings.ansiOverrides[key] ?? theme.term[key]) as string;
    return (
      <input
        key={key}
        type="color"
        className="color"
        title={`${bright ? "bright " : ""}${name}`}
        value={value}
        onChange={(e) => set("ansiOverrides", { ...settings.ansiOverrides, [key]: e.target.value })}
      />
    );
  }

  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog wide" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h3>Settings</h3>
        <div className="dialog-tabs">
          {(["appearance", "terminal", "colors", "logging"] as Section[]).map((s) => (
            <button type="button" key={s} className={section === s ? "on" : ""} onClick={() => setSection(s)}>
              {s[0].toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>

        {section === "appearance" && (
          <div className="form">
            <label>Theme</label>
            <div className="theme-grid">
              {THEMES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={`theme-swatch${settings.theme === t.id ? " on" : ""}`}
                  onClick={() => set("theme", t.id)}
                  style={{ background: t.term.background, color: t.term.foreground }}
                >
                  <span className="swatch-dots">
                    {[t.term.red, t.term.green, t.term.yellow, t.term.blue, t.term.magenta].map((c, i) => <i key={i} style={{ background: c }} />)}
                  </span>
                  {t.name}
                </button>
              ))}
            </div>
            <label>Layout</label>
            <div className="segmented">
              {(["compact", "comfortable"] as Density[]).map((d) => (
                <button type="button" key={d} className={settings.density === d ? "on" : ""} onClick={() => set("density", d)}>
                  {d === "compact" ? "Compact" : "Comfortable"}
                </button>
              ))}
            </div>
            <label className="check">
              <input type="checkbox" checked={settings.highlight} onChange={(e) => set("highlight", e.target.checked)} />
              Highlight IPs, interfaces, errors and state changes in output
            </label>
            <label className="check">
              <input type="checkbox" checked={settings.warnInsecure} onChange={(e) => set("warnInsecure", e.target.checked)} />
              Warn before Telnet to production devices
            </label>
          </div>
        )}

        {section === "terminal" && (
          <div className="form">
            <label>Font</label>
            <input list="fonts" value={settings.fontFamily} onChange={(e) => set("fontFamily", e.target.value)} spellCheck={false} />
            <datalist id="fonts">{FONT_CHOICES.map((f) => <option key={f} value={f} />)}</datalist>
            <label>Font size</label>
            <div className="row">
              <input type="range" min={9} max={28} value={settings.fontSize} onChange={(e) => set("fontSize", parseInt(e.target.value, 10))} />
              <span className="value">{settings.fontSize}px</span>
            </div>
            <label>Line spacing</label>
            <div className="row">
              <input type="range" min={1} max={1.6} step={0.05} value={settings.lineHeight} onChange={(e) => set("lineHeight", parseFloat(e.target.value))} />
              <span className="value">{settings.lineHeight.toFixed(2)}</span>
            </div>
            <label>Letter spacing</label>
            <div className="row">
              <input type="range" min={0} max={3} step={0.5} value={settings.letterSpacing} onChange={(e) => set("letterSpacing", parseFloat(e.target.value))} />
              <span className="value">{settings.letterSpacing}px</span>
            </div>
            <label>Cursor</label>
            <div className="segmented">
              {(["block", "bar", "underline"] as CursorStyle[]).map((c) => (
                <button type="button" key={c} className={settings.cursorStyle === c ? "on" : ""} onClick={() => set("cursorStyle", c)}>
                  {c[0].toUpperCase() + c.slice(1)}
                </button>
              ))}
            </div>
            <label className="check">
              <input type="checkbox" checked={settings.cursorBlink} onChange={(e) => set("cursorBlink", e.target.checked)} />
              Blinking cursor
            </label>
            <label>Scrollback</label>
            <select value={settings.scrollback} onChange={(e) => set("scrollback", parseInt(e.target.value, 10))}>
              {[10000, 50000, 100000, 250000].map((n) => <option key={n} value={n}>{n.toLocaleString()} lines</option>)}
            </select>
          </div>
        )}

        {section === "colors" && (
          <div className="form">
            <label>Contrast</label>
            <div className="row">
              <input type="range" min={1} max={7} step={0.5} value={settings.minimumContrast} onChange={(e) => set("minimumContrast", parseFloat(e.target.value))} />
              <span className="value">{settings.minimumContrast === 1 ? "Off" : `${settings.minimumContrast}:1`}</span>
            </div>
            <div className="hint">Raises dim text to at least this contrast against the background.</div>
            <label>ANSI colors</label>
            <div className="ansi-grid">{ANSI.map((n) => ansi(n, false))}</div>
            <label>Bright</label>
            <div className="ansi-grid">{ANSI.map((n) => ansi(n, true))}</div>
            <div className="hint">
              Changes are kept on top of every theme.{" "}
              <button type="button" className="link" onClick={() => set("ansiOverrides", {})}>Reset to the theme's colors</button>
            </div>
          </div>
        )}

        {section === "logging" && (
          <div className="form">
            <label className="check">
              <input type="checkbox" checked={settings.logAll} onChange={(e) => set("logAll", e.target.checked)} />
              Log every session to a file
            </label>
            <label className="check">
              <input type="checkbox" checked={settings.logTimestamps} onChange={(e) => set("logTimestamps", e.target.checked)} />
              Timestamp each line in logs
            </label>
            <label className="check">
              <input type="checkbox" checked={settings.maskExports} onChange={(e) => set("maskExports", e.target.checked)} />
              Mask passwords and keys when saving output
            </label>
            <label>Log folder</label>
            <div className="row">
              <input value={settings.logDir} onChange={(e) => set("logDir", e.target.value)} placeholder={folder} spellCheck={false} />
              <button type="button" className="btn" onClick={() => void openLogFolder()}>Open</button>
            </div>
            <div className="hint">Logs always mask passwords, secrets, SNMP communities and keys. Device settings can turn logging on for single devices.</div>
            <label>Config backups</label>
            <div className="row">
              <input value={settings.configDir} onChange={(e) => set("configDir", e.target.value)} placeholder={configs} spellCheck={false} />
              <button type="button" className="btn" onClick={() => void openConfigFolder()}>Open</button>
            </div>
            <div className="hint">Backups are stored as taken, one folder per site and device, so they can be restored. Bundles and exports follow the masking setting above.</div>
          </div>
        )}

        <div className="dialog-actions">
          <button type="button" className="btn left" onClick={() => onChange({ ...DEFAULT_SETTINGS, groupBy: settings.groupBy, logDir: settings.logDir, configDir: settings.configDir })}>
            Restore defaults
          </button>
          <button type="button" className="btn primary" onClick={onClose} autoFocus>Done</button>
        </div>
      </div>
    </div>
  );
}
