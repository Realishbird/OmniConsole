import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import Sidebar from "./components/Sidebar";
import DeviceDialog, { type PasswordChange } from "./components/DeviceDialog";
import PromptDialog from "./components/PromptDialog";
import ConfirmDialog from "./components/ConfirmDialog";
import {
  CloseIcon, DownIcon, Logo, MoonIcon, PlusIcon, SaveIcon, SearchIcon, SidebarIcon, SunIcon, UpIcon,
} from "./components/Icons";
import { deletePassword, loadDevices, saveDevices, saveTextFile, setPassword } from "./lib/backend";
import { parseQuick } from "./lib/quick";
import { TerminalSession, type PendingPrompt, type SessionState } from "./lib/terminal";
import { newDevice, type ConnectSpec, type Device } from "./lib/types";

interface Tab {
  key: string;
  title: string;
  session: TerminalSession;
  state: SessionState;
  message: string;
  prompt: PendingPrompt | null;
  cols: number;
  rows: number;
}

interface Confirm {
  title: string;
  message: string;
  confirm: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

const isMac = navigator.platform.toLowerCase().includes("mac");

function stored<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // private mode
  }
}

export default function App() {
  const [dark, setDark] = useState<boolean>(() => stored("dark", true));
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(() => stored("sidebarOpen", true));
  const [sidebarWidth, setSidebarWidth] = useState<number>(() => stored("sidebarWidth", 260));
  const [fontSize, setFontSize] = useState<number>(() => stored("fontSize", 14));
  const [devices, setDevices] = useState<Device[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ device: Device; isNew: boolean } | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [quick, setQuick] = useState("");
  const [quickInvalid, setQuickInvalid] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findText, setFindText] = useState("");
  const quickRef = useRef<HTMLInputElement>(null);
  const findRef = useRef<HTMLInputElement>(null);
  const panes = useRef<Record<string, HTMLDivElement | null>>({});
  const panesBox = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<Tab[]>([]);
  tabsRef.current = tabs;

  const active = tabs.find((t) => t.key === activeKey) ?? null;

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    store("dark", dark);
    for (const t of tabsRef.current) t.session.setDark(dark);
  }, [dark]);
  useEffect(() => store("sidebarOpen", sidebarOpen), [sidebarOpen]);
  useEffect(() => store("sidebarWidth", sidebarWidth), [sidebarWidth]);
  useEffect(() => {
    store("fontSize", fontSize);
    for (const t of tabsRef.current) t.session.setFontSize(fontSize);
  }, [fontSize]);

  useEffect(() => {
    loadDevices().then(setDevices).catch((e) => console.error("loading devices", e));
  }, []);

  const askConfirm = useCallback(
    (c: Omit<Confirm, "resolve">) => new Promise<boolean>((resolve) => setConfirm({ ...c, resolve })),
    [],
  );

  const patchTab = useCallback((key: string, patch: Partial<Tab>) => {
    setTabs((prev) => prev.map((t) => (t.key === key ? { ...t, ...patch } : t)));
  }, []);

  /* ---------- tabs ---------- */

  const openTab = useCallback(
    (title: string, spec: Omit<ConnectSpec, "cols" | "rows">) => {
      const key = crypto.randomUUID();
      const session = new TerminalSession(
        spec,
        {
          onState: (state, message) => patchTab(key, { state, message }),
          onPrompt: (prompt) => patchTab(key, { prompt }),
          onSize: (cols, rows) => patchTab(key, { cols, rows }),
          confirmPaste: (lines) =>
            askConfirm({
              title: "Paste multiple lines?",
              message: `You are about to paste ${lines} lines into ${title}. Each line is sent as a command.`,
              confirm: `Paste ${lines} lines`,
            }),
        },
        dark,
      );
      session.setFontSize(fontSize);
      setTabs((prev) => [...prev, { key, title, session, state: "connecting", message: "Connecting", prompt: null, cols: 0, rows: 0 }]);
      setActiveKey(key);
    },
    [askConfirm, dark, fontSize, patchTab],
  );

  const closeTab = useCallback(
    async (key: string) => {
      const tab = tabsRef.current.find((t) => t.key === key);
      if (!tab) return;
      if (tab.state === "connected") {
        const ok = await askConfirm({
          title: `Disconnect ${tab.title}?`,
          message: "The session is still connected.",
          confirm: "Disconnect",
          danger: true,
        });
        if (!ok) return;
      }
      tab.session.dispose();
      const rest = tabsRef.current.filter((t) => t.key !== key);
      setTabs(rest);
      setActiveKey((cur) => {
        if (cur !== key) return cur;
        const i = tabsRef.current.findIndex((t) => t.key === key);
        return rest[Math.min(i, rest.length - 1)]?.key ?? null;
      });
    },
    [askConfirm],
  );

  // show / connect the active tab
  useEffect(() => {
    if (!active) return;
    const el = panes.current[active.key];
    if (el) void active.session.attach(el).then(() => active.session.focus());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeKey]);

  // fit the terminal when the window or sidebar changes size
  useEffect(() => {
    const box = panesBox.current;
    if (!box) return;
    const ro = new ResizeObserver(() => {
      const t = tabsRef.current.find((x) => x.key === activeKey);
      t?.session.fitNow();
    });
    ro.observe(box);
    return () => ro.disconnect();
  }, [activeKey]);

  /* ---------- connecting ---------- */

  const connectDevice = useCallback(
    (d: Device) => {
      openTab(d.name, {
        protocol: d.protocol,
        host: d.host,
        port: d.port,
        username: d.username,
        deviceId: d.id,
        serialPort: d.serialPort,
        baud: d.baud,
      });
    },
    [openTab],
  );

  function quickConnect(e: React.FormEvent) {
    e.preventDefault();
    const t = parseQuick(quick);
    if (!t) {
      setQuickInvalid(true);
      return;
    }
    const title = t.protocol === "serial" ? t.serialPort : t.username ? `${t.username}@${t.host}` : t.host;
    openTab(title, { ...t });
    setQuick("");
  }

  /* ---------- devices ---------- */

  const sites = useMemo(() => [...new Set(devices.map((d) => d.site).filter(Boolean))].sort(), [devices]);

  async function persist(next: Device[]) {
    setDevices(next);
    try {
      await saveDevices(next);
    } catch (e) {
      console.error("saving devices", e);
    }
  }

  async function onSaveDevice(device: Device, password: PasswordChange) {
    const exists = devices.some((d) => d.id === device.id);
    await persist(exists ? devices.map((d) => (d.id === device.id ? device : d)) : [...devices, device]);
    try {
      if (password.kind === "set") await setPassword(device.id, password.value);
      if (password.kind === "delete") await deletePassword(device.id);
    } catch (e) {
      await askConfirm({ title: "Password not saved", message: `The system keychain refused it: ${e}`, confirm: "OK" });
    }
    setEditing(null);
  }

  async function onDeleteDevice(device: Device) {
    const ok = await askConfirm({
      title: `Delete ${device.name}?`,
      message: "It is removed from the sidebar together with its saved password.",
      confirm: "Delete",
      danger: true,
    });
    if (!ok) return;
    await persist(devices.filter((d) => d.id !== device.id));
    void deletePassword(device.id).catch(() => {});
  }

  /* ---------- output and search ---------- */

  async function saveOutput() {
    if (!active) return;
    const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
    const path = await save({
      defaultPath: `${active.title.replace(/[\\/:*?"<>|@ ]+/g, "_")}_${stamp}.txt`,
      filters: [{ name: "Text", extensions: ["txt", "log"] }],
    });
    if (path) await saveTextFile(path, active.session.text());
  }

  function find(next: boolean) {
    if (!active || !findText) return;
    const opts = { caseSensitive: false, decorations: { matchOverviewRuler: "#e0af68", activeMatchColorOverviewRuler: "#7aa2f7" } };
    if (next) active.session.search.findNext(findText, opts);
    else active.session.search.findPrevious(findText, opts);
  }

  /* ---------- keyboard ---------- */

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = isMac ? e.metaKey : e.ctrlKey;
      const key = e.key.toLowerCase();
      let handled = true;
      if (e.ctrlKey && key === "tab") {
        const list = tabsRef.current;
        if (list.length > 1) {
          const i = list.findIndex((t) => t.key === activeKey);
          setActiveKey(list[(i + (e.shiftKey ? list.length - 1 : 1)) % list.length].key);
        }
      } else if (mod && e.shiftKey && key === "t") quickRef.current?.focus();
      else if (mod && e.shiftKey && key === "w") activeKey && void closeTab(activeKey);
      else if (mod && e.shiftKey && key === "b") setSidebarOpen((v) => !v);
      else if (mod && e.shiftKey && key === "f") {
        setFindOpen(true);
        setTimeout(() => findRef.current?.select(), 0);
      } else if (mod && e.shiftKey && key === "c") active?.session.copySelection();
      else if (mod && e.shiftKey && key === "v") void active?.session.pasteClipboard();
      else if (mod && e.shiftKey && key === "s") void saveOutput();
      else if (mod && (key === "=" || key === "+")) setFontSize((s) => Math.min(28, s + 1));
      else if (mod && key === "-") setFontSize((s) => Math.max(9, s - 1));
      else if (mod && key === "0") setFontSize(14);
      else handled = false;
      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const mod = isMac ? "⌘" : "Ctrl";

  return (
    <div className="app">
      {sidebarOpen ? (
        <Sidebar
          devices={devices}
          width={sidebarWidth}
          onResize={setSidebarWidth}
          onConnect={connectDevice}
          onAdd={(site) => setEditing({ device: newDevice("ssh", site ?? ""), isNew: true })}
          onEdit={(device) => setEditing({ device, isNew: false })}
          onDuplicate={(d) => setEditing({ device: { ...d, id: crypto.randomUUID(), name: `${d.name} copy` }, isNew: true })}
          onDelete={(d) => void onDeleteDevice(d)}
        />
      ) : (
        <div />
      )}

      <div className="main">
        <div className="topbar">
          <button className={`icon-btn${sidebarOpen ? " on" : ""}`} title={`Devices (${mod}+Shift+B)`} onClick={() => setSidebarOpen((v) => !v)}>
            <SidebarIcon />
          </button>
          <form style={{ flex: 1, display: "flex" }} onSubmit={quickConnect}>
            <input
              ref={quickRef}
              className={`quick${quickInvalid ? " invalid" : ""}`}
              placeholder="Quick connect:  ssh admin@10.0.0.1   ·   telnet 10.0.0.5 2001   ·   COM3 9600"
              value={quick}
              onChange={(e) => {
                setQuick(e.target.value);
                setQuickInvalid(false);
              }}
              spellCheck={false}
            />
          </form>
          <button className="icon-btn" title={`Find (${mod}+Shift+F)`} disabled={!active} onClick={() => { setFindOpen(true); setTimeout(() => findRef.current?.select(), 0); }}>
            <SearchIcon />
          </button>
          <button className="icon-btn" title={`Save output to a file (${mod}+Shift+S)`} disabled={!active} onClick={() => void saveOutput()}>
            <SaveIcon />
          </button>
          <button className="icon-btn" title={dark ? "Light theme" : "Dark theme"} onClick={() => setDark((v) => !v)}>
            {dark ? <SunIcon /> : <MoonIcon />}
          </button>
        </div>

        {tabs.length > 0 && (
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.key}
                className={`tab${t.key === activeKey ? " active" : ""}`}
                onMouseDown={(e) => {
                  if (e.button === 1) {
                    e.preventDefault();
                    void closeTab(t.key);
                  } else setActiveKey(t.key);
                }}
                title={t.message}
              >
                <span className={`dot ${t.state}`} />
                <span className="title">{t.title}</span>
                <button className="icon-btn" title={`Close (${mod}+Shift+W)`} onMouseDown={(e) => e.stopPropagation()} onClick={() => void closeTab(t.key)}>
                  <CloseIcon />
                </button>
              </div>
            ))}
            <button className="icon-btn new-tab" title={`Quick connect (${mod}+Shift+T)`} onClick={() => quickRef.current?.focus()}>
              <PlusIcon />
            </button>
          </div>
        )}

        <div className="panes" ref={panesBox}>
          {tabs.length === 0 && (
            <div className="welcome">
              <div className="welcome-card">
                <Logo />
                <h1>OmniConsole</h1>
                <div>Double-click a device in the sidebar, or type an address in quick connect.</div>
                <ul>
                  <li><span>Quick connect</span><kbd>{mod}+Shift+T</kbd></li>
                  <li><span>Show or hide devices</span><kbd>{mod}+Shift+B</kbd></li>
                  <li><span>Find in output</span><kbd>{mod}+Shift+F</kbd></li>
                  <li><span>Save output to a file</span><kbd>{mod}+Shift+S</kbd></li>
                  <li><span>Next tab</span><kbd>Ctrl+Tab</kbd></li>
                  <li><span>Copy / paste</span><span>select to copy, right-click to paste</span></li>
                </ul>
              </div>
            </div>
          )}
          {tabs.map((t) => (
            <div key={t.key} className={`pane${t.key === activeKey ? " active" : ""}`} ref={(el) => { panes.current[t.key] = el; }}>
              {(t.state === "closed" || t.state === "error") && t.key === activeKey && (
                <div className="pane-banner">
                  <span>{t.state === "error" ? "Connection failed" : "Disconnected"}</span>
                  <button className="btn primary" onClick={() => void t.session.reconnect()}>Reconnect</button>
                  <button className="btn" onClick={() => void closeTab(t.key)}>Close tab</button>
                </div>
              )}
            </div>
          ))}
          {findOpen && active && (
            <form className="findbar" onSubmit={(e) => { e.preventDefault(); find(true); }}>
              <input
                ref={findRef}
                autoFocus
                placeholder="Find in output"
                value={findText}
                onChange={(e) => setFindText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    setFindOpen(false);
                    active.session.search.clearDecorations();
                    active.session.focus();
                  } else if (e.key === "Enter" && e.shiftKey) {
                    e.preventDefault();
                    find(false);
                  }
                }}
                spellCheck={false}
              />
              <button type="button" className="icon-btn" title="Previous (Shift+Enter)" onClick={() => find(false)}><UpIcon /></button>
              <button type="submit" className="icon-btn" title="Next (Enter)"><DownIcon /></button>
              <button type="button" className="icon-btn" title="Close (Esc)" onClick={() => { setFindOpen(false); active.session.search.clearDecorations(); active.session.focus(); }}>
                <CloseIcon />
              </button>
            </form>
          )}
        </div>

        <div className="statusbar">
          {active ? (
            <>
              <span className="item"><span className={`dot ${active.state}`} /></span>
              <span className="grow">{active.message}</span>
              {active.cols > 0 && <span className="item">{active.cols} × {active.rows}</span>}
              <span className="item">UTF-8</span>
            </>
          ) : (
            <span className="grow">{devices.length} saved device{devices.length === 1 ? "" : "s"}</span>
          )}
          <span className="item">{fontSize}px</span>
        </div>
      </div>

      {active?.prompt && <PromptDialog key={active.key + active.prompt.kind + active.prompt.message} prompt={active.prompt} onAnswer={(v) => active.session.answer(v)} />}
      {editing && (
        <DeviceDialog device={editing.device} isNew={editing.isNew} sites={sites} onSave={(d, p) => void onSaveDevice(d, p)} onCancel={() => setEditing(null)} />
      )}
      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          message={confirm.message}
          confirm={confirm.confirm}
          danger={confirm.danger}
          onResult={(ok) => {
            confirm.resolve(ok);
            setConfirm(null);
          }}
        />
      )}
    </div>
  );
}

