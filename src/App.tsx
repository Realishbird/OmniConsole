import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Sidebar from "./components/Sidebar";
import DeviceDialog, { type SecretChange, type SecretChanges } from "./components/DeviceDialog";
import PromptDialog from "./components/PromptDialog";
import ConfirmDialog from "./components/ConfirmDialog";
import SettingsDialog from "./components/SettingsDialog";
import SnippetsDialog from "./components/SnippetsDialog";
import ToolsPanel, { type ToolsSection } from "./components/ToolsPanel";
import CommandPalette, { type PaletteItem } from "./components/CommandPalette";
import {
  BookmarkIcon, ChecksIcon, CloseIcon, CommandIcon, DownIcon, Logo, PlusIcon, RecordIcon, SaveIcon, SearchIcon, SettingsIcon,
  SidebarIcon, SplitDownIcon, SplitRightIcon, UpIcon,
} from "./components/Icons";
import {
  deleteSecret, exportDevices, importDevices, loadData, loadDevices, openLogFolder, revealLog, saveData, saveDevices,
  saveText, setSecret, type SecretKind,
} from "./lib/backend";
import { PRIVILEGE_LABEL, type PromptInfo } from "./lib/prompt";
import { parseQuick } from "./lib/quick";
import { DEFAULT_SETTINGS, type Settings } from "./lib/settings";
import { TerminalSession, type PendingPrompt, type SessionSpec, type SessionState } from "./lib/terminal";
import { applyTheme, THEMES, themeById } from "./lib/themes";
import { envInfo, newDevice, serialLabel, type Device, type HistoryEntry, type Snippet } from "./lib/types";

interface Tab {
  key: string;
  title: string;
  session: TerminalSession;
  deviceId?: string;
  state: SessionState;
  message: string;
  prompt: PendingPrompt | null;
  promptInfo: PromptInfo | null;
  logPath: string | null;
  cols: number;
  rows: number;
  everConnected: boolean;
  /** automatic reconnect scheduled: attempt number and when */
  retry: { attempt: number; at: number } | null;
}

interface Confirm {
  title: string;
  message: string;
  confirm: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

type SplitDir = "row" | "column";

const isMac = navigator.platform.toLowerCase().includes("mac");
const MAX_HISTORY = 2000;
const RETRY_DELAYS = [3, 5, 10, 20, 30]; // seconds; the last one repeats
const MAX_RETRIES = 20;

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

const STATE_LABEL: Record<SessionState, string> = {
  connecting: "Connecting",
  connected: "Connected",
  closed: "Disconnected",
  error: "Failed",
};

export default function App() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsLoaded, setSettingsLoaded] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(() => stored("sidebarOpen", true));
  const [sidebarWidth, setSidebarWidth] = useState<number>(() => stored("sidebarWidth", 280));
  const [toolsOpen, setToolsOpen] = useState<boolean>(() => stored("toolsOpen", false));
  const [toolsSection, setToolsSection] = useState<ToolsSection>(() => stored("toolsSection", "checks"));
  const [devices, setDevices] = useState<Device[]>([]);
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [slots, setSlots] = useState<string[]>([]);
  const [focusSlot, setFocusSlot] = useState(0);
  const [splitDir, setSplitDir] = useState<SplitDir>("row");
  const [editing, setEditing] = useState<{ device: Device; isNew: boolean } | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [snippetsOpen, setSnippetsOpen] = useState(false);
  const [palette, setPalette] = useState<{ initial: string; placeholder?: string; only?: string } | null>(null);
  const [quick, setQuick] = useState("");
  const [quickInvalid, setQuickInvalid] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findText, setFindText] = useState("");
  const [toast, setToast] = useState<string | null>(null);
  const [, setTick] = useState(0);
  const quickRef = useRef<HTMLInputElement>(null);
  const findRef = useRef<HTMLInputElement>(null);
  const panes = useRef<Record<string, HTMLDivElement | null>>({});
  const panesBox = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<Tab[]>([]);
  tabsRef.current = tabs;
  const devicesRef = useRef<Device[]>([]);
  devicesRef.current = devices;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const focusSlotRef = useRef(focusSlot);
  focusSlotRef.current = focusSlot;
  const retryTimers = useRef<Map<string, number>>(new Map());

  const activeKey = slots[focusSlot] ?? null;
  const active = tabs.find((t) => t.key === activeKey) ?? null;
  const activeDevice = active?.deviceId ? devices.find((d) => d.id === active.deviceId) : undefined;
  const split = slots.length === 2;

  /* ---------- persistence ---------- */

  useEffect(() => {
    loadDevices().then(setDevices).catch((e) => console.error("loading devices", e));
    loadData<Partial<Settings>>("settings")
      .then((s) => setSettings({ ...DEFAULT_SETTINGS, ...(s ?? {}) }))
      .catch(() => {})
      .finally(() => setSettingsLoaded(true));
    loadData<Snippet[]>("snippets").then((s) => setSnippets(Array.isArray(s) ? s : [])).catch(() => {});
    loadData<HistoryEntry[]>("history").then((h) => setHistory(Array.isArray(h) ? h : [])).catch(() => {});
  }, []);

  useEffect(() => {
    applyTheme(themeById(settings.theme));
    document.documentElement.dataset.density = settings.density;
    for (const t of tabsRef.current) t.session.applySettings(settings);
    if (!settingsLoaded) return;
    const timer = window.setTimeout(() => void saveData("settings", settings).catch(() => {}), 300);
    return () => window.clearTimeout(timer);
  }, [settings, settingsLoaded]);

  useEffect(() => store("sidebarOpen", sidebarOpen), [sidebarOpen]);
  useEffect(() => store("toolsOpen", toolsOpen), [toolsOpen]);
  useEffect(() => store("toolsSection", toolsSection), [toolsSection]);
  useEffect(() => store("sidebarWidth", sidebarWidth), [sidebarWidth]);

  const historyDirty = useRef(false);
  useEffect(() => {
    if (!historyDirty.current) return;
    const timer = window.setTimeout(() => void saveData("history", history).catch(() => {}), 1000);
    return () => window.clearTimeout(timer);
  }, [history]);

  const persist = useCallback(async (next: Device[]) => {
    setDevices(next);
    try {
      await saveDevices(next);
    } catch (e) {
      console.error("saving devices", e);
    }
  }, []);

  function updateSnippets(next: Snippet[]) {
    setSnippets(next);
    void saveData("snippets", next).catch(() => {});
  }

  function showToast(text: string) {
    setToast(text);
    window.setTimeout(() => setToast((t) => (t === text ? null : t)), 3500);
  }

  // countdown for automatic reconnects
  useEffect(() => {
    if (!tabs.some((t) => t.retry)) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, [tabs]);

  /* ---------- dialogs ---------- */

  const askConfirm = useCallback(
    (c: Omit<Confirm, "resolve">) => new Promise<boolean>((resolve) => setConfirm({ ...c, resolve })),
    [],
  );

  const patchTab = useCallback((key: string, patch: Partial<Tab>) => {
    setTabs((prev) => prev.map((t) => (t.key === key ? { ...t, ...patch } : t)));
  }, []);

  /* ---------- tabs and panes ---------- */

  /** Show a tab: focus its pane if visible, otherwise put it in the focused pane. */
  const activate = useCallback((key: string) => {
    setSlots((prev) => {
      const i = prev.indexOf(key);
      if (i >= 0) {
        setFocusSlot(i);
        return prev;
      }
      if (prev.length === 0) {
        setFocusSlot(0);
        return [key];
      }
      const next = [...prev];
      next[Math.min(focusSlotRef.current, next.length - 1)] = key;
      return next;
    });
  }, []);

  const cancelRetry = useCallback((key: string) => {
    const timer = retryTimers.current.get(key);
    if (timer) window.clearTimeout(timer);
    retryTimers.current.delete(key);
  }, []);

  const scheduleRetry = useCallback(
    (key: string) => {
      const tab = tabsRef.current.find((t) => t.key === key);
      if (!tab) return;
      const attempt = (tab.retry?.attempt ?? 0) + 1;
      if (attempt > MAX_RETRIES) {
        patchTab(key, { retry: null });
        return;
      }
      const delay = RETRY_DELAYS[Math.min(attempt - 1, RETRY_DELAYS.length - 1)];
      patchTab(key, { retry: { attempt, at: Date.now() + delay * 1000 } });
      cancelRetry(key);
      retryTimers.current.set(
        key,
        window.setTimeout(() => {
          retryTimers.current.delete(key);
          const t = tabsRef.current.find((x) => x.key === key);
          if (t && (t.state === "closed" || t.state === "error")) void t.session.reconnect();
        }, delay * 1000),
      );
    },
    [cancelRetry, patchTab],
  );

  const onSessionState = useCallback(
    (key: string, state: SessionState, message: string) => {
      const tab = tabsRef.current.find((t) => t.key === key);
      const patch: Partial<Tab> = { state, message };
      if (state === "connected") {
        patch.everConnected = true;
        patch.retry = null;
        cancelRetry(key);
        if (tab?.deviceId) {
          const now = Date.now();
          void persist(devicesRef.current.map((d) => (d.id === tab.deviceId ? { ...d, lastConnected: now } : d)));
        }
      }
      patchTab(key, patch);
      // only sessions that were up retry; a failed login does not loop
      if ((state === "closed" || state === "error") && tab?.everConnected) {
        const device = tab.deviceId ? devicesRef.current.find((d) => d.id === tab.deviceId) : undefined;
        if (device?.autoReconnect) scheduleRetry(key);
      }
    },
    [cancelRetry, patchTab, persist, scheduleRetry],
  );

  const openTab = useCallback(
    (title: string, spec: SessionSpec, deviceId?: string) => {
      const key = crypto.randomUUID();
      const session = new TerminalSession(
        spec,
        {
          onState: (state, message) => onSessionState(key, state, message),
          onPrompt: (prompt) => patchTab(key, { prompt }),
          onSize: (cols, rows) => patchTab(key, { cols, rows }),
          onLog: (logPath) => patchTab(key, { logPath }),
          onPromptInfo: (promptInfo) => patchTab(key, { promptInfo }),
          onCommand: (command) => {
            historyDirty.current = true;
            setHistory((prev) => {
              if (prev[prev.length - 1]?.command === command) return prev;
              return [...prev, { command, device: title, at: Date.now() }].slice(-MAX_HISTORY);
            });
          },
          onFocus: () => activate(key),
          confirmPaste: (lines) =>
            askConfirm({
              title: "Paste multiple lines?",
              message: `You are about to paste ${lines} lines into ${title}. Each line is sent as a command.`,
              confirm: `Paste ${lines} lines`,
            }),
        },
        settingsRef.current,
      );
      setTabs((prev) => [
        ...prev,
        {
          key, title, session, deviceId, state: "connecting", message: "Connecting", prompt: null, promptInfo: null,
          logPath: null, cols: 0, rows: 0, everConnected: false, retry: null,
        },
      ]);
      activate(key);
      return key;
    },
    [activate, askConfirm, onSessionState, patchTab],
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
      cancelRetry(key);
      tab.session.dispose();
      const before = tabsRef.current;
      const rest = before.filter((t) => t.key !== key);
      setTabs(rest);
      setSlots((prev) => {
        const i = prev.indexOf(key);
        if (i < 0) return prev;
        const hidden = rest.filter((t) => !prev.includes(t.key));
        const next = [...prev];
        if (hidden.length) {
          // the nearest hidden tab takes its place
          const at = before.findIndex((t) => t.key === key);
          const dist = (t: Tab) => Math.abs(before.indexOf(t) - at);
          next[i] = hidden.reduce((best, t) => (dist(t) < dist(best) ? t : best)).key;
        } else {
          next.splice(i, 1);
        }
        setFocusSlot((f) => Math.min(f, Math.max(0, next.length - 1)));
        return next;
      });
    },
    [askConfirm, cancelRetry],
  );

  /** Split the window; the other pane shows another tab, or a copy of this session. */
  const splitPane = useCallback(
    (dir: SplitDir) => {
      if (!active) return;
      if (split) {
        if (splitDir === dir) {
          setSlots([active.key]);
          setFocusSlot(0);
        } else setSplitDir(dir);
        return;
      }
      setSplitDir(dir);
      const other = tabsRef.current.find((t) => t.key !== active.key);
      if (other) {
        setSlots([active.key, other.key]);
        setFocusSlot(1);
      } else {
        const key = openTab(active.title, active.session.spec, active.deviceId);
        setSlots([active.key, key]);
        setFocusSlot(1);
      }
    },
    [active, openTab, split, splitDir],
  );

  // attach and fit the visible terminals
  useEffect(() => {
    for (const key of slots) {
      const tab = tabsRef.current.find((t) => t.key === key);
      const el = panes.current[key];
      if (tab && el) void tab.session.attach(el);
    }
    const t = tabsRef.current.find((x) => x.key === activeKey);
    if (t && !t.prompt) setTimeout(() => t.session.focus(), 0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [slots.join("|"), activeKey, splitDir]);

  useEffect(() => {
    const box = panesBox.current;
    if (!box) return;
    const ro = new ResizeObserver(() => {
      for (const key of slots) tabsRef.current.find((x) => x.key === key)?.session.fitNow();
    });
    ro.observe(box);
    return () => ro.disconnect();
  }, [slots]);

  /* ---------- connecting ---------- */

  const specFor = useCallback((d: Device): SessionSpec => {
    const jumpDevice = d.jumpDeviceId ? devicesRef.current.find((x) => x.id === d.jumpDeviceId) : undefined;
    const s = settingsRef.current;
    return {
      protocol: d.protocol,
      label: d.name,
      host: d.host,
      port: d.port,
      username: d.username,
      deviceId: d.id,
      authMethod: d.authMethod,
      keyPath: d.keyPath,
      jump:
        d.protocol === "ssh" && jumpDevice
          ? { host: jumpDevice.host, port: jumpDevice.port, username: jumpDevice.username, deviceId: jumpDevice.id, authMethod: jumpDevice.authMethod, keyPath: jumpDevice.keyPath }
          : null,
      keepalive: d.keepalive,
      serialPort: d.serialPort,
      baud: d.baud,
      dataBits: d.dataBits,
      parity: d.parity,
      stopBits: d.stopBits,
      flowControl: d.flowControl,
      log: s.logAll || d.logSession,
      logTimestamps: s.logTimestamps,
    };
  }, []);

  const connectDevice = useCallback(
    async (d: Device) => {
      if (d.protocol === "telnet" && d.environment === "production" && settingsRef.current.warnInsecure) {
        const ok = await askConfirm({
          title: `Telnet to production device ${d.name}?`,
          message: "Telnet is unencrypted. Passwords and configuration cross the network in clear text. Use SSH if the device supports it.",
          confirm: "Connect with Telnet",
          danger: true,
        });
        if (!ok) return;
      }
      openTab(d.name, specFor(d), d.id);
    },
    [askConfirm, openTab, specFor],
  );

  function quickConnect(e: React.FormEvent) {
    e.preventDefault();
    const t = parseQuick(quick);
    if (!t) {
      setQuickInvalid(true);
      return;
    }
    const title = t.protocol === "serial" ? t.serialPort : t.username ? `${t.username}@${t.host}` : t.host;
    const s = settingsRef.current;
    const { protocol, host, port, username, serialPort, baud, authMethod, keyPath, keepalive, dataBits, parity, stopBits, flowControl } = {
      ...newDevice(t.protocol),
      ...t,
    };
    openTab(title, {
      protocol, host, port, username, serialPort, baud, authMethod, keyPath, keepalive, dataBits, parity, stopBits, flowControl,
      label: title,
      jump: null,
      log: s.logAll,
      logTimestamps: s.logTimestamps,
    });
    setQuick("");
  }

  /* ---------- devices ---------- */

  async function applySecret(kind: SecretKind, id: string, change: SecretChange) {
    if (change.kind === "set") await setSecret(kind, id, change.value);
    if (change.kind === "delete") await deleteSecret(kind, id);
  }

  async function onSaveDevice(device: Device, secrets: SecretChanges) {
    const exists = devices.some((d) => d.id === device.id);
    await persist(exists ? devices.map((d) => (d.id === device.id ? device : d)) : [...devices, device]);
    try {
      await applySecret("password", device.id, secrets.password);
      await applySecret("passphrase", device.id, secrets.passphrase);
    } catch (e) {
      await askConfirm({ title: "Secret not saved", message: `The system keychain refused it: ${e}`, confirm: "OK" });
    }
    setEditing(null);
  }

  async function onDeleteDevice(device: Device) {
    const users = devices.filter((d) => d.jumpDeviceId === device.id);
    const ok = await askConfirm({
      title: `Delete ${device.name}?`,
      message:
        "It is removed together with its saved password." +
        (users.length ? ` ${users.length} device${users.length > 1 ? "s use" : " uses"} it as a jump host and will connect directly.` : ""),
      confirm: "Delete",
      danger: true,
    });
    if (!ok) return;
    await persist(devices.filter((d) => d.id !== device.id).map((d) => (d.jumpDeviceId === device.id ? { ...d, jumpDeviceId: "" } : d)));
    void deleteSecret("password", device.id).catch(() => {});
    void deleteSecret("passphrase", device.id).catch(() => {});
  }

  async function onImport() {
    try {
      const incoming = await importDevices();
      if (!incoming) return;
      const key = (d: Device) => `${d.protocol}|${d.host.toLowerCase()}|${d.port}|${d.serialPort}|${d.name.toLowerCase()}`;
      const have = new Set(devices.map(key));
      const ids = new Set(devices.map((d) => d.id));
      const fresh = incoming
        .filter((d) => !have.has(key(d)))
        .map((d) => ({ ...newDevice(d.protocol), ...d, id: ids.has(d.id) ? crypto.randomUUID() : d.id, lastConnected: null }));
      await persist([...devices, ...fresh]);
      const skipped = incoming.length - fresh.length;
      showToast(`Imported ${fresh.length} device${fresh.length === 1 ? "" : "s"}${skipped ? `, skipped ${skipped} already saved` : ""}.`);
    } catch (e) {
      await askConfirm({ title: "Import failed", message: String(e), confirm: "OK" });
    }
  }

  async function onExport() {
    const path = await exportDevices(devices).catch((e) => {
      void askConfirm({ title: "Export failed", message: String(e), confirm: "OK" });
      return null;
    });
    if (path) showToast(`Exported ${devices.length} devices. Passwords are never exported.`);
  }

  /* ---------- output, search, snippets ---------- */

  function fileName(title: string) {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[T:]/g, "-");
    return `${title.replace(/[\\/:*?"<>|@ ]+/g, "_")}_${stamp}.txt`;
  }

  async function saveOutput(selectionOnly = false) {
    if (!active) return;
    const text = selectionOnly ? active.session.selection() : active.session.text();
    if (!text.trim()) return showToast(selectionOnly ? "Select some output first." : "Nothing to save yet.");
    const path = await saveText(fileName(active.title), text, settings.maskExports).catch((e) => {
      showToast(`Not saved: ${e}`);
      return null;
    });
    if (path) showToast(`Saved ${selectionOnly ? "selection" : "output"} to ${path}`);
  }

  function find(next: boolean) {
    if (!active || !findText) return;
    const opts = { caseSensitive: false, decorations: { matchOverviewRuler: "#e0af68", activeMatchColorOverviewRuler: "#7aa2f7" } };
    if (next) active.session.search.findNext(findText, opts);
    else active.session.search.findPrevious(findText, opts);
  }

  function openFind() {
    setFindOpen(true);
    setTimeout(() => findRef.current?.select(), 0);
  }

  async function sendSnippet(s: Snippet) {
    if (!active) return;
    const lines = s.body.replace(/\r\n/g, "\n").replace(/\n+$/, "").split("\n");
    const prod = activeDevice?.environment === "production";
    if (lines.length > 1 || prod) {
      const ok = await askConfirm({
        title: prod ? `Send to production device ${active.title}?` : `Send ${lines.length} lines?`,
        message: `${lines.length === 1 ? "This command" : `These ${lines.length} commands`} will be typed into ${active.title}:\n\n${lines.slice(0, 12).join("\n")}${lines.length > 12 ? "\n…" : ""}`,
        confirm: "Send",
        danger: prod,
      });
      if (!ok) return;
    }
    setSnippetsOpen(false);
    active.session.send(lines.join("\r") + "\r");
  }

  function addBookmark() {
    const b = active?.session.addBookmark();
    if (b) showToast(`Bookmark ${b.id} added. Find it in the command palette.`);
  }

  /* ---------- command palette ---------- */

  const mod = isMac ? "⌘" : "Ctrl";
  const paletteItems = useMemo((): PaletteItem[] => {
    if (!palette) return [];
    const items: PaletteItem[] = [];
    const action = (id: string, title: string, run: () => void, keys?: string, enabled = true) => {
      if (enabled) items.push({ id: `a:${id}`, group: "Action", title, run, keys });
    };
    action("quick", "Quick connect", () => quickRef.current?.focus(), `${mod}+Shift+T`);
    action("add", "Add device", () => setEditing({ device: newDevice(), isNew: true }));
    action("close", "Close tab", () => active && void closeTab(active.key), `${mod}+Shift+W`, !!active);
    action("dup", "Duplicate session in a new tab", () => active && openTab(active.title, active.session.spec, active.deviceId), undefined, !!active);
    action("splitr", split && splitDir === "row" ? "Unsplit" : "Split right", () => splitPane("row"), `${mod}+Shift+D`, !!active);
    action("splitd", split && splitDir === "column" ? "Unsplit" : "Split down", () => splitPane("column"), `${mod}+Shift+E`, !!active);
    action("reconnect", "Reconnect", () => active && void active.session.reconnect(), `${mod}+Shift+R`, !!active && active.state !== "connected");
    action("break", "Send break (serial and Telnet)", () => active?.session.sendBreak(), undefined, !!active && active.session.spec.protocol !== "ssh");
    action("find", "Find in output", openFind, `${mod}+Shift+F`, !!active);
    action("save", "Save output to a file", () => void saveOutput(), `${mod}+Shift+S`, !!active);
    action("savesel", "Save selection to a file", () => void saveOutput(true), undefined, !!active);
    action("bookmark", "Add bookmark at the cursor", addBookmark, `${mod}+Shift+M`, !!active);
    action("history", "Search command history", () => setPalette({ initial: "", only: "History", placeholder: "Search command history" }), `${mod}+Shift+H`, !!active);
    action("clear", "Clear scrollback", () => active?.session.term.clear(), undefined, !!active);
    action("sidebar", sidebarOpen ? "Hide devices" : "Show devices", () => setSidebarOpen((v) => !v), `${mod}+Shift+B`);
    action("tools", toolsOpen ? "Hide tools" : "Show tools (health checks, configs)", () => setToolsOpen((v) => !v), `${mod}+Shift+I`);
    action("checks", "Run health checks", () => { setToolsSection("checks"); setToolsOpen(true); });
    action("backup", "Back up running config", () => { setToolsSection("configs"); setToolsOpen(true); });
    action("snippets", "Manage snippets", () => setSnippetsOpen(true), `${mod}+Shift+O`);
    action("settings", "Settings", () => setSettingsOpen(true), `${mod}+,`);
    action("import", "Import devices", () => void onImport());
    action("export", "Export devices", () => void onExport());
    action("logs", "Open session log folder", () => void openLogFolder());
    for (const t of THEMES) action(`theme-${t.id}`, `Theme: ${t.name}`, () => setSettings((s) => ({ ...s, theme: t.id })));
    for (const t of tabs) items.push({ id: `t:${t.key}`, group: "Tab", title: t.title, subtitle: STATE_LABEL[t.state], run: () => activate(t.key) });
    for (const b of active?.session.bookmarks ?? []) {
      items.push({ id: `b:${b.id}`, group: "Bookmark", title: b.label, subtitle: new Date(b.at).toLocaleTimeString(), run: () => active?.session.goToBookmark(b.id) });
    }
    for (const d of devices) {
      items.push({
        id: `d:${d.id}`, group: "Connect", title: d.name,
        subtitle: [d.host || d.serialPort, d.customer, d.site, d.role, ...d.tags].filter(Boolean).join(" · "),
        run: () => void connectDevice(d),
      });
    }
    for (const s of snippets) items.push({ id: `s:${s.id}`, group: "Snippet", title: s.name, subtitle: s.body.split("\n")[0], run: () => void sendSnippet(s) });
    if (active) {
      // past commands are typed without Enter, so they can be checked first
      const seen = new Set<string>();
      for (let i = history.length - 1; i >= 0 && seen.size < 300; i--) {
        const h = history[i];
        if (seen.has(h.command)) continue;
        seen.add(h.command);
        items.push({ id: `h:${i}`, group: "History", title: h.command, subtitle: h.device, run: () => active.session.send(h.command) });
      }
    }
    return palette.only ? items.filter((i) => i.group === palette.only) : items;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [palette, tabs, devices, snippets, history, active, split, splitDir, sidebarOpen]);

  /* ---------- keyboard ---------- */

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const modKey = isMac ? e.metaKey : e.ctrlKey;
      const key = e.key.toLowerCase();
      let handled = true;
      if (e.ctrlKey && key === "tab") {
        const list = tabsRef.current;
        if (list.length > 1) {
          const i = list.findIndex((t) => t.key === activeKey);
          activate(list[(i + (e.shiftKey ? list.length - 1 : 1)) % list.length].key);
        }
      } else if (e.altKey && !e.ctrlKey && !e.metaKey && /^Digit[1-9]$/.test(e.code)) {
        const t = tabsRef.current[parseInt(e.code.slice(5), 10) - 1];
        if (t) activate(t.key);
      } else if (key === "f6" && split) setFocusSlot((f) => 1 - f);
      else if (modKey && e.shiftKey && key === "p") setPalette({ initial: "" });
      else if (modKey && !e.shiftKey && key === "k") setPalette({ initial: "" });
      else if (modKey && e.shiftKey && key === "h") active && setPalette({ initial: "", only: "History", placeholder: "Search command history" });
      else if (modKey && e.shiftKey && key === "t") quickRef.current?.focus();
      else if (modKey && e.shiftKey && key === "w") activeKey && void closeTab(activeKey);
      else if (modKey && e.shiftKey && key === "b") setSidebarOpen((v) => !v);
      else if (modKey && e.shiftKey && key === "i") setToolsOpen((v) => !v);
      else if (modKey && e.shiftKey && key === "f") openFind();
      else if (modKey && e.shiftKey && key === "c") active?.session.copySelection();
      else if (modKey && e.shiftKey && key === "v") void active?.session.pasteClipboard();
      else if (modKey && e.shiftKey && key === "s") void saveOutput();
      else if (modKey && e.shiftKey && key === "d") splitPane("row");
      else if (modKey && e.shiftKey && key === "e") splitPane("column");
      else if (modKey && e.shiftKey && key === "m") addBookmark();
      else if (modKey && e.shiftKey && key === "o") setSnippetsOpen(true);
      else if (modKey && e.shiftKey && key === "r") active && active.state !== "connected" && void active.session.reconnect();
      else if (modKey && key === ",") setSettingsOpen(true);
      else if (modKey && (key === "=" || key === "+")) setSettings((s) => ({ ...s, fontSize: Math.min(28, s.fontSize + 1) }));
      else if (modKey && key === "-") setSettings((s) => ({ ...s, fontSize: Math.max(9, s.fontSize - 1) }));
      else if (modKey && key === "0") setSettings((s) => ({ ...s, fontSize: DEFAULT_SETTINGS.fontSize }));
      else handled = false;
      if (handled) {
        e.preventDefault();
        e.stopPropagation();
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  /* ---------- rendering ---------- */

  function paneStyle(key: string): React.CSSProperties {
    const i = slots.indexOf(key);
    if (i < 0) return { display: "none" };
    if (!split) return { display: "block", inset: 0 };
    return splitDir === "row"
      ? { display: "block", top: 0, bottom: 0, left: i === 0 ? 0 : "50%", width: "50%" }
      : { display: "block", left: 0, right: 0, top: i === 0 ? 0 : "50%", height: "50%" };
  }

  function tabDevice(t: Tab): Device | undefined {
    return t.deviceId ? devices.find((x) => x.id === t.deviceId) : undefined;
  }

  const promptGlyph = (p: PromptInfo) =>
    p.privilege === "config" ? `(${p.mode})#` : p.privilege === "privileged" ? "#" : p.privilege === "user" ? ">" : "$";

  function protocolLabel(t: Tab): string {
    const s = t.session.spec;
    if (s.protocol === "serial") return `Serial ${s.serialPort} ${serialLabel(s)}`;
    if (s.protocol === "telnet") return "Telnet (unencrypted)";
    return s.jump ? `SSH via ${s.jump.host}` : "SSH";
  }

  const activeEnv = envInfo(activeDevice?.environment ?? "");

  return (
    <div className="app">
      {sidebarOpen ? (
        <Sidebar
          devices={devices}
          width={sidebarWidth}
          groupBy={settings.groupBy}
          onGroupBy={(groupBy) => setSettings((s) => ({ ...s, groupBy }))}
          onResize={setSidebarWidth}
          onConnect={(d) => void connectDevice(d)}
          onAdd={(preset) => setEditing({ device: { ...newDevice("ssh"), ...preset }, isNew: true })}
          onEdit={(device) => setEditing({ device, isNew: false })}
          onDuplicate={(d) => setEditing({ device: { ...d, id: crypto.randomUUID(), name: `${d.name} copy`, favorite: false, lastConnected: null }, isNew: true })}
          onDelete={(d) => void onDeleteDevice(d)}
          onToggleFavorite={(d) => void persist(devices.map((x) => (x.id === d.id ? { ...x, favorite: !x.favorite } : x)))}
          onImport={() => void onImport()}
          onExport={() => void onExport()}
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
          <button className="icon-btn" title={`Command palette (${mod}+Shift+P)`} onClick={() => setPalette({ initial: "" })}>
            <CommandIcon />
          </button>
          <button className={`icon-btn${split && splitDir === "row" ? " on" : ""}`} title={`Split right (${mod}+Shift+D)`} disabled={!active} onClick={() => splitPane("row")}>
            <SplitRightIcon />
          </button>
          <button className={`icon-btn${split && splitDir === "column" ? " on" : ""}`} title={`Split down (${mod}+Shift+E)`} disabled={!active} onClick={() => splitPane("column")}>
            <SplitDownIcon />
          </button>
          <button className="icon-btn" title={`Find (${mod}+Shift+F)`} disabled={!active} onClick={openFind}>
            <SearchIcon />
          </button>
          <button className="icon-btn" title={`Bookmark this line (${mod}+Shift+M)`} disabled={!active} onClick={addBookmark}>
            <BookmarkIcon />
          </button>
          <button className="icon-btn" title={`Save output to a file (${mod}+Shift+S)`} disabled={!active} onClick={() => void saveOutput()}>
            <SaveIcon />
          </button>
          <button className={`icon-btn${toolsOpen ? " on" : ""}`} title={`Tools: health checks and configs (${mod}+Shift+I)`} onClick={() => setToolsOpen((v) => !v)}>
            <ChecksIcon />
          </button>
          <button className="icon-btn" title={`Settings (${mod}+,)`} onClick={() => setSettingsOpen(true)}>
            <SettingsIcon />
          </button>
        </div>

        {tabs.length > 0 && (
          <div className="tabs">
            {tabs.map((t, i) => {
              const d = tabDevice(t);
              const env = envInfo(d?.environment ?? "");
              const color = d?.color || env.color;
              return (
                <div
                  key={t.key}
                  className={`tab${t.key === activeKey ? " active" : ""}${slots.includes(t.key) && t.key !== activeKey ? " visible" : ""}${color ? " colored" : ""}`}
                  style={color ? { ["--tab-color" as string]: color } : undefined}
                  onMouseDown={(e) => {
                    if (e.button === 1) {
                      e.preventDefault();
                      void closeTab(t.key);
                    } else activate(t.key);
                  }}
                  title={`${t.title}\n${STATE_LABEL[t.state]}: ${t.message}${i < 9 ? `\nAlt+${i + 1}` : ""}`}
                >
                  <span className={`dot ${t.state}`} role="img" aria-label={STATE_LABEL[t.state]} />
                  {env.short && <span className="env-badge" style={{ ["--env" as string]: env.color }}>{env.short}</span>}
                  <span className="title">{t.title}</span>
                  {t.promptInfo && <span className={`priv ${t.promptInfo.privilege}`} title={PRIVILEGE_LABEL[t.promptInfo.privilege]}>{promptGlyph(t.promptInfo)}</span>}
                  {t.logPath && <span className="rec" title="Logging to a file"><RecordIcon /></span>}
                  <button className="icon-btn" title={`Close (${mod}+Shift+W)`} onMouseDown={(e) => e.stopPropagation()} onClick={() => void closeTab(t.key)}>
                    <CloseIcon />
                  </button>
                </div>
              );
            })}
            <button className="icon-btn new-tab" title={`Quick connect (${mod}+Shift+T)`} onClick={() => quickRef.current?.focus()}>
              <PlusIcon />
            </button>
          </div>
        )}

        <div className="workspace">
        <div className={`panes${split ? ` split ${splitDir}` : ""}`} ref={panesBox}>
          {tabs.length === 0 && (
            <div className="welcome">
              <div className="welcome-card">
                <Logo />
                <h1>OmniConsole</h1>
                <div>Double-click a device in the sidebar, or type an address in quick connect.</div>
                <ul>
                  <li><span>Command palette</span><kbd>{mod}+Shift+P</kbd></li>
                  <li><span>Quick connect</span><kbd>{mod}+Shift+T</kbd></li>
                  <li><span>Split right / down</span><kbd>{mod}+Shift+D / E</kbd></li>
                  <li><span>Command history</span><kbd>{mod}+Shift+H</kbd></li>
                  <li><span>Snippets</span><kbd>{mod}+Shift+O</kbd></li>
                  <li><span>Health checks and configs</span><kbd>{mod}+Shift+I</kbd></li>
                  <li><span>Find in output</span><kbd>{mod}+Shift+F</kbd></li>
                  <li><span>Bookmark a line</span><kbd>{mod}+Shift+M</kbd></li>
                  <li><span>Switch tab</span><kbd>Ctrl+Tab · Alt+1…9</kbd></li>
                  <li><span>Copy / paste</span><span>select to copy, right-click to paste</span></li>
                </ul>
              </div>
            </div>
          )}
          {tabs.map((t) => (
            <div
              key={t.key}
              className={`pane${t.key === activeKey ? " active" : ""}`}
              style={paneStyle(t.key)}
              ref={(el) => {
                panes.current[t.key] = el;
              }}
            >
              {(t.state === "closed" || t.state === "error") && slots.includes(t.key) && (
                <div className="pane-banner">
                  <span>
                    {t.state === "error" ? "Connection failed" : "Disconnected"}
                    {t.retry && `. Reconnecting in ${Math.max(0, Math.ceil((t.retry.at - Date.now()) / 1000))} s (attempt ${t.retry.attempt})`}
                  </span>
                  {t.retry && (
                    <button className="btn" onClick={() => { cancelRetry(t.key); patchTab(t.key, { retry: null }); }}>Stop</button>
                  )}
                  <button className="btn primary" onClick={() => { cancelRetry(t.key); patchTab(t.key, { retry: null }); void t.session.reconnect(); }}>Reconnect now</button>
                  <button className="btn" onClick={() => void closeTab(t.key)}>Close tab</button>
                </div>
              )}
            </div>
          ))}
          {split && <div className={`split-divider ${splitDir}`} />}
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
          {toast && <div className="toast" role="status">{toast}</div>}
        </div>
        {toolsOpen && (
          <ToolsPanel
            sessionId={active?.state === "connected" ? active.session.backendId : null}
            tabKey={active?.key ?? "none"}
            deviceName={activeDevice?.name || active?.promptInfo?.hostname || active?.title || ""}
            site={activeDevice?.site ?? ""}
            hostname={active?.promptInfo?.hostname ?? ""}
            maskSecrets={settings.maskExports}
            section={toolsSection}
            onSection={setToolsSection}
            onClose={() => setToolsOpen(false)}
            onSend={(c) => active?.session.send(c + "\r")}
            onToast={showToast}
          />
        )}
        </div>

        <div className="statusbar">
          {active ? (
            <>
              <span className="item"><span className={`dot ${active.state}`} />{STATE_LABEL[active.state]}</span>
              <span className="item strong">{active.promptInfo?.hostname || active.title}</span>
              <span className={`item${active.session.spec.protocol === "telnet" ? " warn-text" : ""}`}>{protocolLabel(active)}</span>
              {activeEnv.short && (
                <span className="env-badge" style={{ ["--env" as string]: activeEnv.color }}>{activeEnv.label}</span>
              )}
              {active.promptInfo && PRIVILEGE_LABEL[active.promptInfo.privilege] && (
                <span className={`priv-label ${active.promptInfo.privilege}`}>
                  {PRIVILEGE_LABEL[active.promptInfo.privilege]}
                  {active.promptInfo.privilege === "config" ? ` (${active.promptInfo.mode})` : ""}
                </span>
              )}
              <span className="grow" title={active.message}>{active.state === "connected" ? "" : active.message}</span>
              {active.logPath && (
                <button className="item link rec-label" title={active.logPath} onClick={() => void revealLog(active.logPath!)}>
                  <RecordIcon /> Logging
                </button>
              )}
              {active.cols > 0 && <span className="item">{active.cols} × {active.rows}</span>}
            </>
          ) : (
            <span className="grow">{devices.length} saved device{devices.length === 1 ? "" : "s"}</span>
          )}
          <span className="item">{settings.fontSize}px</span>
        </div>
      </div>

      {active?.prompt && <PromptDialog key={active.key + active.prompt.kind + active.prompt.message} prompt={active.prompt} onAnswer={(v) => active.session.answer(v)} />}
      {editing && (
        <DeviceDialog device={editing.device} isNew={editing.isNew} devices={devices} onSave={(d, s) => void onSaveDevice(d, s)} onCancel={() => setEditing(null)} />
      )}
      {settingsOpen && <SettingsDialog settings={settings} onChange={setSettings} onClose={() => { setSettingsOpen(false); active?.session.focus(); }} />}
      {snippetsOpen && (
        <SnippetsDialog
          snippets={snippets}
          canSend={!!active && active.state === "connected"}
          onChange={updateSnippets}
          onSend={(s) => void sendSnippet(s)}
          onClose={() => { setSnippetsOpen(false); active?.session.focus(); }}
        />
      )}
      {palette && (
        <CommandPalette
          items={paletteItems}
          initial={palette.initial}
          placeholder={palette.placeholder}
          onClose={() => { setPalette(null); active?.session.focus(); }}
        />
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
