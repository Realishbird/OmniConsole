import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChecksIcon, CloseIcon, DiffIcon, FolderIcon, PlayIcon, RefreshIcon, SaveIcon, StopIcon, TrashIcon } from "./Icons";
import Assistant from "./Assistant";
import Workstation from "./Workstation";
import Workflows from "./Workflows";
import type { AiSettings } from "../lib/ai";
import {
  backupConfig, bundleFiles, cancelChecks, checksCatalog, compareReports, deleteConfig, detectDevice, diffConfigs, fileStamp,
  listConfigs, OS_LABEL, openConfigFolder, outputText, readConfig, revealConfig, runChecks, runCommand, saveBundle, saveReport,
  searchConfigs, stamp, STATUS_LABEL, type Backup, type Change, type CheckInfo, type CheckProgress, type CheckReport,
  type CheckResult, type DiffResult, type Facts, type SearchHit, type Status, type Table,
} from "../lib/tools";

export type ToolsSection = "checks" | "configs" | "assistant" | "workstation" | "workflows";


interface Props {
  /** the Rust session id of the active tab, when connected */
  sessionId: string | null;
  tabKey: string;
  /** device name and site used for backups and bundles */
  deviceName: string;
  site: string;
  hostname: string;
  maskSecrets: boolean;
  section: ToolsSection;
  onSection: (s: ToolsSection) => void;
  onClose: () => void;
  /** send a follow-up command to the terminal */
  onSend: (command: string) => void;
  onToast: (text: string) => void;
  environment: string;
  /** the device host, for the workstation tools' default target */
  host: string;
  ai: AiSettings;
  getSelection: () => string;
  getRecent: () => string;
  onOpenSettings: () => void;
}

/** Per-tab state survives closing the panel and switching tabs. */
interface TabTools {
  facts: Facts | null;
  report: CheckReport | null;
  baseline: CheckReport | null;
  baselineLabel: string;
  running: boolean;
  progress: { command: string; index: number; total: number } | null;
  live: CheckResult[];
  selected: string[];
  error: string | null;
  every: number;
}

const cache = new Map<string, TabTools>();

function fresh(): TabTools {
  return { facts: null, report: null, baseline: null, baselineLabel: "", running: false, progress: null, live: [], selected: [], error: null, every: 0 };
}

const STATUS_ORDER: Record<Status, number> = { fail: 0, warn: 1, unknown: 2, pass: 3, skipped: 4 };

export default function ToolsPanel(props: Props) {
  const { sessionId, tabKey, section, onSection, onClose } = props;
  const [pendingAsk, setPendingAsk] = useState<{ question: string; output: string } | null>(null);
  const [, setTick] = useState(0);
  const stateRef = useRef<TabTools>(cache.get(tabKey) ?? fresh());
  // switching tabs swaps the state object
  if (!cache.has(tabKey)) cache.set(tabKey, stateRef.current);
  if (cache.get(tabKey) !== stateRef.current) stateRef.current = cache.get(tabKey)!;
  const st = stateRef.current;
  const update = useCallback((patch: Partial<TabTools>) => {
    Object.assign(cache.get(tabKey) ?? st, patch);
    setTick((t) => t + 1);
  }, [tabKey, st]);

  return (
    <aside className="tools">
      <div className="tools-head">
        <div className="dialog-tabs">
          <button className={section === "assistant" ? "on" : ""} onClick={() => onSection("assistant")}>Assistant</button>
          <button className={section === "checks" ? "on" : ""} onClick={() => onSection("checks")}>Checks</button>
          <button className={section === "workflows" ? "on" : ""} onClick={() => onSection("workflows")}>Workflows</button>
          <button className={section === "configs" ? "on" : ""} onClick={() => onSection("configs")}>Configs</button>
          <button className={section === "workstation" ? "on" : ""} onClick={() => onSection("workstation")}>Workstation</button>
        </div>
        <button className="icon-btn" title="Close tools" onClick={onClose}><CloseIcon /></button>
      </div>
      {section === "checks" && <Checks {...props} st={st} update={update} />}
      {section === "configs" && <Configs {...props} st={st} update={update} />}
      {section === "assistant" && (
        <Assistant
          sessionId={sessionId}
          tabKey={tabKey}
          facts={st.facts}
          report={st.report}
          hostname={props.hostname}
          deviceName={props.deviceName}
          site={props.site}
          environment={props.environment}
          ai={props.ai}
          getSelection={props.getSelection}
          getRecent={props.getRecent}
          onToast={props.onToast}
          onOpenSettings={props.onOpenSettings}
          pendingAsk={pendingAsk}
          onAskConsumed={() => setPendingAsk(null)}
        />
      )}
      {section === "workflows" && (
        <Workflows sessionId={sessionId} facts={st.facts} onToast={props.onToast} onAsk={(question, output) => { setPendingAsk({ question, output }); onSection("assistant"); }} />
      )}
      {section === "workstation" && <Workstation key={tabKey} defaultTarget={props.host} onToast={props.onToast} />}
      {!sessionId && section !== "workstation" && <div className="tools-note">Connect a device to run checks, workflows, backups or proposed commands. Saved configs can still be viewed, searched and compared.</div>}
    </aside>
  );
}

/* ------------------------------------------------------------- checks */

function Checks({ sessionId, tabKey, deviceName, site, maskSecrets, onSend, onToast, st, update }: Props & { st: TabTools; update: (p: Partial<TabTools>) => void }) {
  const [catalog, setCatalog] = useState<CheckInfo[]>([]);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [pickOpen, setPickOpen] = useState(false);
  const [baselines, setBaselines] = useState<Backup[]>([]);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    void checksCatalog(st.facts?.os ?? null).then(setCatalog);
  }, [st.facts?.os]);

  useEffect(() => {
    void listConfigs(deviceName).then((b) => setBaselines(b.filter((x) => x.kind === "checks")));
  }, [deviceName, st.report]);

  const run = useCallback(async () => {
    if (!sessionId || st.running) return;
    update({ running: true, error: null, live: [], progress: null });
    try {
      let facts = st.facts;
      if (!facts) {
        facts = await detectDevice(sessionId);
        update({ facts });
      }
      const report = await runChecks(sessionId, facts, st.selected, (p: CheckProgress) => {
        const cur = cache.get(tabKey)!;
        if (p.type === "command") update({ progress: { command: p.command, index: p.index, total: p.total } });
        else if (p.type === "result") update({ live: [...cur.live, p.result] });
      });
      update({ report, facts: report.facts, running: false, progress: null });
      const fails = report.results.filter((r) => r.status === "fail").length;
      const warns = report.results.filter((r) => r.status === "warn").length;
      onToast(fails ? `${fails} failed, ${warns} warning${warns === 1 ? "" : "s"}` : warns ? `${warns} warning${warns === 1 ? "" : "s"}, nothing failed` : "All checks passed");
    } catch (e) {
      update({ running: false, progress: null, error: String(e) });
    }
  }, [sessionId, st, tabKey, update, onToast]);

  // repeat on a timer while the tab is open
  useEffect(() => {
    if (timer.current) window.clearInterval(timer.current);
    timer.current = null;
    if (st.every > 0 && sessionId) {
      timer.current = window.setInterval(() => void run(), st.every * 60_000);
    }
    return () => {
      if (timer.current) window.clearInterval(timer.current);
    };
  }, [st.every, sessionId, run]);

  const results = st.running ? st.live : st.report?.results ?? [];
  const sorted = useMemo(() => [...results].sort((a, b) => STATUS_ORDER[a.status] - STATUS_ORDER[b.status]), [results]);
  const counts = useMemo(() => {
    const c: Record<Status, number> = { pass: 0, warn: 0, fail: 0, unknown: 0, skipped: 0 };
    for (const r of results) c[r.status]++;
    return c;
  }, [results]);
  const changes: Change[] = useMemo(() => (st.baseline && st.report ? compareReports(st.baseline, st.report) : []), [st.baseline, st.report]);
  const categories = useMemo(() => Array.from(new Set(catalog.map((c) => c.category))), [catalog]);

  async function saveBaseline() {
    if (!st.report) return;
    try {
      const b = await saveReport(site, deviceName, st.report);
      update({ baseline: st.report, baselineLabel: stamp(b.timestamp) });
      onToast("Baseline saved");
    } catch (e) {
      onToast(String(e));
    }
  }

  async function loadBaseline(path: string) {
    try {
      const b = baselines.find((x) => x.path === path);
      const report = JSON.parse(await readConfig(path)) as CheckReport;
      update({ baseline: report, baselineLabel: b ? stamp(b.timestamp) : path });
    } catch (e) {
      onToast(String(e));
    }
  }

  async function bundle() {
    if (!st.report) return;
    const extras = [];
    if (sessionId) {
      try {
        extras.push({ name: "running-config.txt", contents: await runCommand(sessionId, "show running-config", 300) });
      } catch {
        // the config is optional in a bundle
      }
    }
    const name = `${(st.facts?.hostname || deviceName || "device").replace(/[^\w.-]+/g, "_")}_${fileStamp(st.report.finished)}_bundle.zip`;
    const path = await saveBundle(name, bundleFiles(st.report, extras), maskSecrets).catch((e) => {
      onToast(String(e));
      return null;
    });
    if (path) onToast(`Bundle saved: ${path}`);
  }

  const f = st.facts;
  return (
    <div className="tools-body">
      <div className="tools-actions">
        {st.running ? (
          <button className="btn" onClick={() => sessionId && void cancelChecks(sessionId)}><StopIcon /> Stop</button>
        ) : (
          <button className="btn primary" disabled={!sessionId} onClick={() => void run()}><PlayIcon /> Run checks</button>
        )}
        <button className={`btn${pickOpen ? " on" : ""}`} onClick={() => setPickOpen((v) => !v)} title="Choose which checks run">
          {st.selected.length ? `${st.selected.length} of ${catalog.length}` : "All checks"}
        </button>
        <select className="tools-every" value={st.every} onChange={(e) => update({ every: Number(e.target.value) })} title="Repeat the checks while this tab stays open">
          <option value={0}>Run once</option>
          <option value={5}>Every 5 min</option>
          <option value={15}>Every 15 min</option>
          <option value={60}>Every hour</option>
        </select>
        <span className="grow" />
        <button className="icon-btn" title="Save as baseline" disabled={!st.report} onClick={() => void saveBaseline()}><SaveIcon /></button>
        <button className="icon-btn" title="Save a diagnostic bundle (zip)" disabled={!st.report} onClick={() => void bundle()}><FolderIcon /></button>
      </div>

      {pickOpen && (
        <div className="tools-pick">
          {categories.map((cat) => (
            <div key={cat} className="tools-pick-group">
              <div className="tools-pick-title">{cat}</div>
              {catalog.filter((c) => c.category === cat).map((c) => (
                <label key={c.id} className="check" title={c.commands.join("\n") || "not supported on this platform"}>
                  <input
                    type="checkbox"
                    checked={st.selected.length === 0 || st.selected.includes(c.id)}
                    onChange={(e) => {
                      const all = catalog.map((x) => x.id);
                      let next = st.selected.length === 0 ? all : [...st.selected];
                      next = e.target.checked ? [...next, c.id] : next.filter((x) => x !== c.id);
                      update({ selected: next.length === all.length ? [] : next });
                    }}
                  />
                  {c.title}
                </label>
              ))}
            </div>
          ))}
          <div className="tools-pick-actions">
            <button className="btn" onClick={() => update({ selected: [] })}>All</button>
            <button className="btn" onClick={() => update({ selected: ["__none__"] })}>None</button>
            <button className="btn primary" onClick={() => setPickOpen(false)}>Done</button>
          </div>
        </div>
      )}

      {f && (
        <div className="tools-facts">
          <span className="strong">{f.hostname || deviceName}</span>
          <span>{f.os ? OS_LABEL[f.os] : "Unknown OS"} {f.version}</span>
          {f.model && <span>{f.model}</span>}
          {f.serial && <span>SN {f.serial}</span>}
          {f.uptime && <span>up {f.uptime}</span>}
        </div>
      )}

      {st.progress && (
        <div className="tools-progress">
          <div className="bar"><div style={{ width: `${((st.progress.index + 1) / Math.max(1, st.progress.total)) * 100}%` }} /></div>
          <span>{st.progress.index + 1}/{st.progress.total} · {st.progress.command}</span>
        </div>
      )}
      {st.error && <div className="tools-error">{st.error}</div>}

      {results.length > 0 && (
        <div className="tools-summary">
          {(["fail", "warn", "pass", "unknown", "skipped"] as Status[]).map((s) => counts[s] > 0 && <span key={s} className={`status-chip ${s}`}>{counts[s]} {STATUS_LABEL[s].toLowerCase()}</span>)}
          {st.report && <span className="quiet">{stamp(st.report.finished)}{st.report.cancelled ? " (stopped early)" : ""}</span>}
        </div>
      )}

      {(baselines.length > 0 || st.baseline) && (
        <div className="tools-baseline">
          <span>Compare with</span>
          <select value={st.baselineLabel ? baselines.find((b) => stamp(b.timestamp) === st.baselineLabel)?.path ?? "" : ""} onChange={(e) => e.target.value ? void loadBaseline(e.target.value) : update({ baseline: null, baselineLabel: "" })}>
            <option value="">no baseline</option>
            {baselines.map((b) => <option key={b.path} value={b.path}>{stamp(b.timestamp)}</option>)}
          </select>
          {st.baseline && st.report && (
            <span className={changes.length ? "warn-text" : "quiet"}>{changes.length ? `${changes.length} change${changes.length === 1 ? "" : "s"}` : "no status changes"}</span>
          )}
        </div>
      )}
      {changes.length > 0 && (
        <ul className="tools-changes">
          {changes.map((c) => (
            <li key={c.id}><span className={`status-chip ${c.from}`}>{STATUS_LABEL[c.from]}</span> → <span className={`status-chip ${c.to}`}>{STATUS_LABEL[c.to]}</span> {c.title}: {c.summary}</li>
          ))}
        </ul>
      )}

      <div className="tools-results">
        {sorted.map((r) => (
          <div key={r.id} className={`check-row ${r.status}${open[r.id] ? " open" : ""}`}>
            <button className="check-head" onClick={() => setOpen((o) => ({ ...o, [r.id]: !o[r.id] }))}>
              <span className={`status-dot ${r.status}`} title={STATUS_LABEL[r.status]} />
              <span className="check-title">{r.title}</span>
              <span className="check-summary">{r.summary}</span>
            </button>
            {open[r.id] && (
              <div className="check-detail">
                {r.evidence.length > 0 && <pre className="evidence">{r.evidence.join("\n")}</pre>}
                {r.table && r.table.rows.length > 0 && <DataTable table={r.table} />}
                {r.followUp.length > 0 && (
                  <div className="follow">
                    <span>Next:</span>
                    {r.followUp.map((c) => (
                      <button key={c} className="btn small" disabled={!sessionId || c.includes("<")} title={c.includes("<") ? "Fill in the placeholder yourself" : "Send to the terminal"} onClick={() => onSend(c)}>{c}</button>
                    ))}
                  </div>
                )}
                {st.report && r.commands.length > 0 && (
                  <details className="raw">
                    <summary>Raw output ({r.commands.join(", ")})</summary>
                    {r.commands.map((c) => (
                      <pre key={c}>{`${c}\n${outputText(st.report!, c) ?? "(not run)"}`}</pre>
                    ))}
                  </details>
                )}
              </div>
            )}
          </div>
        ))}
        {!st.running && results.length === 0 && !st.error && (
          <div className="tools-empty">
            <ChecksIcon />
            <p>Runs read-only <code>show</code> commands on the connected device and grades CPU, memory, hardware, interfaces, layer 2, routing neighbors and the log, with the output lines behind each verdict.</p>
          </div>
        )}
      </div>
    </div>
  );
}

export function DataTable({ table }: { table: Table }) {
  const [filter, setFilter] = useState("");
  const rows = filter ? table.rows.filter((r) => r.some((c) => c.toLowerCase().includes(filter.toLowerCase()))) : table.rows;
  return (
    <div className="data-table">
      <div className="data-table-head">
        <span>{table.title} · {rows.length}{filter ? ` of ${table.rows.length}` : ""}</span>
        {table.rows.length > 8 && <input placeholder="Filter" value={filter} onChange={(e) => setFilter(e.target.value)} spellCheck={false} />}
      </div>
      <div className="data-table-scroll">
        <table>
          <thead><tr>{table.columns.map((c) => <th key={c}>{c}</th>)}</tr></thead>
          <tbody>
            {rows.slice(0, 500).map((r, i) => <tr key={i}>{r.map((c, j) => <td key={j}>{c}</td>)}</tr>)}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ configs */

function Configs({ sessionId, deviceName, site, onToast, st, update }: Props & { st: TabTools; update: (p: Partial<TabTools>) => void }) {
  const [all, setAll] = useState<Backup[]>([]);
  const [onlyThis, setOnlyThis] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [view, setView] = useState<{ backup: Backup; text: string } | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [diff, setDiff] = useState<{ a: Backup | string; b: Backup | string; result: DiffResult } | null>(null);
  const [ignoreVolatile, setIgnoreVolatile] = useState(true);
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [allVersions, setAllVersions] = useState(false);
  const [hits, setHits] = useState<SearchHit[] | null>(null);

  const refresh = useCallback(() => {
    void listConfigs().then(setAll);
  }, []);
  useEffect(refresh, [refresh]);

  const backups = useMemo(() => all.filter((b) => b.kind !== "checks" && (!onlyThis || !deviceName || b.device === safe(deviceName))), [all, onlyThis, deviceName]);

  async function backup(kinds: string[]) {
    if (!sessionId) return;
    setBusy(kinds.join("+"));
    try {
      let facts = st.facts;
      if (!facts) {
        facts = await detectDevice(sessionId);
        update({ facts });
      }
      const saved = await backupConfig(sessionId, facts.os, site, facts.hostname || deviceName, kinds);
      onToast(`Saved ${saved.map((b) => b.kind).join(" and ")} config (${saved.map((b) => `${Math.round(b.size / 1024)} KB`).join(", ")})`);
      refresh();
      // diff against the previous running config straight away
      if (kinds.includes("running")) {
        const prev = all.filter((b) => b.kind === "running" && b.device === saved[0].device && b.site === saved[0].site)[0];
        if (prev) await showDiff(prev, saved.find((b) => b.kind === "running")!);
      }
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function showDiff(a: Backup | string, b: Backup | string) {
    try {
      const [ta, tb] = await Promise.all([typeof a === "string" ? a : readConfig(a.path), typeof b === "string" ? b : readConfig(b.path)]);
      setDiff({ a, b, result: await diffConfigs(ta, tb, ignoreVolatile) });
      setView(null);
    } catch (e) {
      onToast(String(e));
    }
  }

  useEffect(() => {
    if (diff) void showDiff(diff.a, diff.b);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ignoreVolatile]);

  async function compareLive() {
    if (!sessionId || picked.length !== 1) return;
    setBusy("live");
    try {
      const text = await runCommand(sessionId, "show running-config", 300);
      const a = backups.find((b) => b.path === picked[0])!;
      await showDiff(a, text);
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function search(e: React.FormEvent) {
    e.preventDefault();
    try {
      setHits(await searchConfigs(query, regex, allVersions));
    } catch (err) {
      onToast(String(err));
    }
  }

  function pick(path: string) {
    setPicked((p) => (p.includes(path) ? p.filter((x) => x !== path) : [...p.slice(-1), path]));
  }

  const label = (x: Backup | string) => (typeof x === "string" ? "live running-config" : `${x.device} ${x.kind} ${stamp(x.timestamp)}`);

  return (
    <div className="tools-body">
      <div className="tools-actions">
        <button className="btn primary" disabled={!sessionId || !!busy} onClick={() => void backup(["running"])}><SaveIcon /> {busy === "running" ? "Saving…" : "Back up running"}</button>
        <button className="btn" disabled={!sessionId || !!busy} onClick={() => void backup(["running", "startup"])}>{busy === "running+startup" ? "Saving…" : "Running + startup"}</button>
        <span className="grow" />
        <button className="icon-btn" title="Refresh" onClick={refresh}><RefreshIcon /></button>
        <button className="icon-btn" title="Open the config folder" onClick={() => void openConfigFolder()}><FolderIcon /></button>
      </div>

      <form className="tools-search" onSubmit={(e) => void search(e)}>
        <input placeholder="Search saved configs (e.g. snmp-server, 10.1.1.)" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
        <label className="check" title="Regular expression"><input type="checkbox" checked={regex} onChange={(e) => setRegex(e.target.checked)} />.*</label>
        <label className="check" title="Search every backup, not only the newest per device"><input type="checkbox" checked={allVersions} onChange={(e) => setAllVersions(e.target.checked)} />all versions</label>
      </form>
      {hits && (
        <div className="tools-hits">
          <div className="tools-hits-head">
            <span>{hits.length} match{hits.length === 1 ? "" : "es"}{hits.length >= 2000 ? " (showing the first 2000)" : ""}</span>
            <button className="icon-btn" title="Clear" onClick={() => setHits(null)}><CloseIcon /></button>
          </div>
          {hits.slice(0, 500).map((h, i) => (
            <button key={i} className="hit" onClick={() => void readConfig(h.path).then((text) => { setView({ backup: all.find((b) => b.path === h.path) ?? { path: h.path, site: h.site, device: h.device, kind: "running", timestamp: h.timestamp, size: 0 }, text }); setDiff(null); })}>
              <span className="quiet">{h.device} · {stamp(h.timestamp)} · line {h.lineNo}</span>
              <code>{h.line}</code>
            </button>
          ))}
        </div>
      )}

      <div className="tools-list-head">
        <span>{backups.length} backup{backups.length === 1 ? "" : "s"}</span>
        {deviceName && (
          <label className="check"><input type="checkbox" checked={onlyThis} onChange={(e) => setOnlyThis(e.target.checked)} />this device only</label>
        )}
        <span className="grow" />
        <button className="btn small" disabled={picked.length !== 2} onClick={() => { const [a, b] = picked.map((p) => backups.find((x) => x.path === p)!).sort((x, y) => x.timestamp - y.timestamp); void showDiff(a, b); }}><DiffIcon /> Diff selected</button>
        <button className="btn small" disabled={picked.length !== 1 || !sessionId || !!busy} onClick={() => void compareLive()} title="Diff the selected backup against the device's current running config">{busy === "live" ? "Reading…" : "Diff with live"}</button>
      </div>
      <div className="tools-backups">
        {backups.map((b) => (
          <div key={b.path} className={`backup${picked.includes(b.path) ? " picked" : ""}${view?.backup.path === b.path ? " viewing" : ""}`}>
            <input type="checkbox" checked={picked.includes(b.path)} onChange={() => pick(b.path)} title="Select for diff" />
            <button className="backup-main" onClick={() => void readConfig(b.path).then((text) => { setView({ backup: b, text }); setDiff(null); }).catch((e) => onToast(String(e)))}>
              <span className="strong">{b.device}</span>
              <span className="quiet">{b.site} · {b.kind} · {stamp(b.timestamp)} · {Math.max(1, Math.round(b.size / 1024))} KB</span>
            </button>
            <button className="icon-btn" title="Show in folder" onClick={() => void revealConfig(b.path)}><FolderIcon /></button>
            <button className="icon-btn" title="Delete this backup" onClick={() => { if (window.confirm(`Delete ${label(b)}?`)) void deleteConfig(b.path).then(refresh).catch((e) => onToast(String(e))); }}><TrashIcon /></button>
          </div>
        ))}
        {backups.length === 0 && <div className="tools-empty"><p>No backups yet. Back up the running config of a connected device; later backups are compared with the previous one automatically.</p></div>}
      </div>

      {view && (
        <div className="tools-view">
          <div className="tools-view-head">
            <span>{label(view.backup)}</span>
            <button className="icon-btn" onClick={() => setView(null)}><CloseIcon /></button>
          </div>
          <pre>{view.text}</pre>
        </div>
      )}
      {diff && (
        <div className="tools-view">
          <div className="tools-view-head">
            <span title={`${label(diff.a)} → ${label(diff.b)}`}>
              <span className="diff-add">+{diff.result.added}</span> <span className="diff-del">−{diff.result.removed}</span> {label(diff.a)} → {label(diff.b)}
            </span>
            <label className="check" title="Skip timestamps, certificate blocks and other lines that change on every save"><input type="checkbox" checked={ignoreVolatile} onChange={(e) => setIgnoreVolatile(e.target.checked)} />ignore noise</label>
            <button className="icon-btn" onClick={() => setDiff(null)}><CloseIcon /></button>
          </div>
          <pre className="diff">
            {diff.result.added + diff.result.removed === 0 ? <span className="quiet">No differences.</span> : null}
            {diff.result.lines.filter((l, i, arr) => l.tag !== " " || near(arr, i)).map((l, i, arr) => (
              <span key={i} className={l.tag === "+" ? "diff-add" : l.tag === "-" ? "diff-del" : ""}>
                {i > 0 && gap(arr, i) ? "⋯\n" : ""}{l.tag} {l.text}{"\n"}
              </span>
            ))}
          </pre>
        </div>
      )}
    </div>
  );
}

/** keep 3 lines of context around a change */
function near(lines: { tag: string }[], i: number): boolean {
  for (let k = Math.max(0, i - 3); k <= Math.min(lines.length - 1, i + 3); k++) if (lines[k].tag !== " ") return true;
  return false;
}

function gap(shown: { old: number | null; new: number | null }[], i: number): boolean {
  const prev = shown[i - 1];
  const cur = shown[i];
  const p = prev.new ?? prev.old ?? 0;
  const c = cur.new ?? cur.old ?? 0;
  return c - p > 1 && prev.old !== null && cur.old !== null && (cur.old ?? 0) - (prev.old ?? 0) > 1;
}

/** mirror of the Rust safe_name, for matching folder names */
function safe(s: string): string {
  const cleaned = s.trim().replace(/[^\p{L}\p{N}\-_. ]/gu, "_").replace(/^[. ]+|[. ]+$/g, "");
  return cleaned || "unnamed";
}
