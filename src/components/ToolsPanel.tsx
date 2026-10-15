import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChecksIcon, CloseIcon, DiffIcon, FolderIcon, PlayIcon, RefreshIcon, SaveIcon, StopIcon, TrashIcon } from "./Icons";
import Assistant from "./Assistant";
import Workstation from "./Workstation";
import Workflows from "./Workflows";
import DeployPanel, { DeployFlow } from "./DeployPanel";
import { platformProfile, type Profile } from "../lib/platforms";
import FilesPanel from "./FilesPanel";
import Collect from "./Collect";
import type { SessionSpec } from "../lib/terminal";
import type { AiSettings, AuditEntry } from "../lib/ai";
import { loadData, saveData } from "../lib/backend";
import {
  backupConfig, bundleFiles, cancelChecks, checksCatalog, compareReports, deleteConfig, detectDevice, diffConfigs, fileStamp,
  lifecycleRun, listConfigs, OS_CHOICES, OS_LABEL, openConfigFolder, outputText, readConfig, restorableLines, revealConfig, runChecks, runCommand, saveBundle, saveReport,
  searchConfigs, stamp, STATUS_LABEL, STATUS_ORDER, applyExpectedAps, isWirelessController, mergeApiResults, runIseApiChecks, saveIseApiPassword, type Backup, type Change, type CheckInfo, type CheckProgress, type CheckReport,
  type CheckResult, type DiffResult, type Facts, type Os, type SearchHit, type Status, type Table, type Tier,
} from "../lib/tools";

export type ToolsSection = "checks" | "collect" | "configs" | "assistant" | "workstation" | "workflows" | "deploy" | "files";


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
  onAiChange: (ai: AiSettings) => void;
  /** the saved device behind the active tab, for per-device assistant settings */
  deviceId: string;
  getSelection: () => string;
  getRecent: () => string;
  onOpenSettings: () => void;
  /** the active tab's connection, for the file transfer connection */
  spec: SessionSpec | null;
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
  /** quick or detailed set when no explicit selection */
  tier: Tier;
  /** manual platform choice when detection is inconclusive or wrong */
  osOverride: Os | null;
  /** the "pre" snapshot of a pre/post comparison, kept apart from saved baselines */
  pre: CheckReport | null;
  /** wireless controllers: how many APs the site should have (0 = not graded) */
  expectedAps: number;
  /** ISE: admin API access; the password is only ever in the OS keychain */
  ise: { host: string; username: string; insecure: boolean; saved: boolean; running: boolean; error: string | null };
}

const cache = new Map<string, TabTools>();

/** The platform detected on a console tab, if its tools have run (used by shortcuts that depend on the device). */
export function detectedOs(tabKey: string): string | null {
  const t = cache.get(tabKey);
  return t?.osOverride ?? t?.facts?.os ?? null;
}

function fresh(): TabTools {
  return { facts: null, report: null, baseline: null, baselineLabel: "", running: false, progress: null, live: [], selected: [], error: null, every: 0, tier: "quick", osOverride: null, pre: null, expectedAps: 0, ise: { host: "", username: "", insecure: false, saved: false, running: false, error: null } };
}

export default function ToolsPanel(props: Props) {
  const { sessionId, tabKey, section, onSection, onClose } = props;
  const [pendingAsk, setPendingAsk] = useState<{ question: string; output: string } | null>(null);
  const [, setTick] = useState(0);
  // switching tabs swaps the state object; a tab seen for the first time starts fresh
  if (!cache.has(tabKey)) cache.set(tabKey, fresh());
  const st = cache.get(tabKey)!;
  const update = useCallback((patch: Partial<TabTools>) => {
    const target = cache.get(tabKey);
    if (target) Object.assign(target, patch);
    setTick((t) => t + 1);
  }, [tabKey]);

  return (
    <aside className="tools">
      <div className="tools-head">
        <div className="dialog-tabs">
          <button className={section === "assistant" ? "on" : ""} onClick={() => onSection("assistant")}>Assistant</button>
          <button className={section === "checks" ? "on" : ""} onClick={() => onSection("checks")}>Checks</button>
          <button className={section === "collect" ? "on" : ""} onClick={() => onSection("collect")}>Collect</button>
          <button className={section === "workflows" ? "on" : ""} onClick={() => onSection("workflows")}>Workflows</button>
          <button className={section === "configs" ? "on" : ""} onClick={() => onSection("configs")}>Configs</button>
          <button className={section === "deploy" ? "on" : ""} onClick={() => onSection("deploy")}>Deploy</button>
          <button className={section === "files" ? "on" : ""} onClick={() => onSection("files")}>Files</button>
          <button className={section === "workstation" ? "on" : ""} onClick={() => onSection("workstation")}>Workstation</button>
        </div>
        <button className="icon-btn" title="Close tools" onClick={onClose}><CloseIcon /></button>
      </div>
      {section === "checks" && <Checks {...props} st={st} update={update} />}
      {section === "configs" && <Configs {...props} st={st} update={update} />}
      {section === "collect" && (
        <Collect sessionId={sessionId} tabKey={tabKey} facts={st.facts} onFacts={(facts) => update({ facts })} deviceName={props.deviceName} site={props.site} maskSecrets={props.maskSecrets} onToast={props.onToast} onSend={props.onSend} onAsk={(question, output) => { setPendingAsk({ question, output }); onSection("assistant"); }} />
      )}
      {section === "assistant" && (
        <Assistant
          sessionId={sessionId}
          tabKey={tabKey}
          deviceId={props.deviceId}
          facts={st.facts}
          report={st.report}
          onReport={(report) => update({ report })}
          hostname={props.hostname}
          deviceName={props.deviceName}
          site={props.site}
          environment={props.environment}
          ai={props.ai}
          onAiChange={props.onAiChange}
          getSelection={props.getSelection}
          getRecent={props.getRecent}
          onToast={props.onToast}
          onOpenSettings={props.onOpenSettings}
          pendingAsk={pendingAsk}
          onAskConsumed={() => setPendingAsk(null)}
        />
      )}
      {section === "workflows" && (
        <Workflows sessionId={sessionId} tabKey={tabKey} facts={st.facts} onToast={props.onToast} onAsk={(question, output) => { setPendingAsk({ question, output }); onSection("assistant"); }} />
      )}
      {section === "files" && <FilesPanel tabKey={tabKey} spec={props.spec} deviceName={props.deviceName} onToast={props.onToast} />}
      {section === "workstation" && <Workstation key={tabKey} defaultTarget={props.host} onToast={props.onToast} />}
      {section === "deploy" && (
        <DeployPanel
          sessionId={sessionId}
          tabKey={tabKey}
          facts={st.facts}
          onFacts={(facts) => update({ facts })}
          deviceName={props.deviceName}
          site={props.site}
          environment={props.environment}
          onToast={props.onToast}
        />
      )}
      {!sessionId && section !== "workstation" && section !== "files" && <div className="tools-note">Connect a device to run checks, workflows, backups or proposed commands. Saved configs can still be viewed, searched and compared.</div>}
    </aside>
  );
}

/* ------------------------------------------------------------- checks */

function Checks({ sessionId, tabKey, deviceName, site, host, maskSecrets, onSend, onToast, st, update }: Props & { st: TabTools; update: (p: Partial<TabTools>) => void }) {
  const [catalog, setCatalog] = useState<CheckInfo[]>([]);
  const [isePassword, setIsePassword] = useState("");
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

  const run = useCallback(async (tier?: Tier) => {
    if (!sessionId || st.running) return;
    const t = tier ?? st.tier;
    update({ running: true, error: null, live: [], progress: null, tier: t });
    try {
      let facts = st.facts;
      if (!facts) {
        facts = await detectDevice(sessionId);
        update({ facts });
      }
      let report = await runChecks(sessionId, facts, st.selected, st.selected.length ? null : t, (p: CheckProgress) => {
        const cur = cache.get(tabKey)!;
        if (p.type === "command") update({ progress: { command: p.command, index: p.index, total: p.total } });
        else if (p.type === "result") update({ live: [...cur.live, p.result] });
      }, st.osOverride);
      report = applyExpectedAps(report, st.expectedAps);
      // keep the API rows of the previous run until the API runs again
      const api = st.report?.results.filter((r) => r.category === "ISE API") ?? [];
      if (api.length) report = mergeApiResults(report, api);
      update({ report, facts: st.osOverride ? { ...report.facts, os: st.osOverride } : report.facts, running: false, progress: null });
      const crit = report.results.filter((r) => r.status === "critical").length;
      const warns = report.results.filter((r) => r.status === "warning").length;
      const missing = report.results.filter((r) => r.status === "not-collected").length;
      const tail = missing ? `, ${missing} not collected` : "";
      onToast(crit ? `${crit} critical, ${warns} warning${warns === 1 ? "" : "s"}${tail}` : warns ? `${warns} warning${warns === 1 ? "" : "s"}, nothing critical${tail}` : missing ? `Nothing wrong found${tail}` : "All checks healthy");
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
    const c: Record<Status, number> = { healthy: 0, warning: 0, critical: 0, "not-applicable": 0, unsupported: 0, "not-collected": 0 };
    for (const r of results) c[r.status]++;
    return c;
  }, [results]);
  const compareWith = st.pre ?? st.baseline;
  const changes: Change[] = useMemo(() => (compareWith && st.report && compareWith !== st.report ? compareReports(compareWith, st.report) : []), [compareWith, st.report]);
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
  const os = st.osOverride ?? f?.os ?? null;
  const wireless = isWirelessController(f, os);
  const iseHost = st.ise.host || host;

  async function savePassword() {
    if (!iseHost) return;
    try {
      const saved = await saveIseApiPassword(iseHost, isePassword);
      setIsePassword("");
      update({ ise: { ...st.ise, host: iseHost, saved, error: null } });
      onToast(saved ? "API password saved to the OS keychain" : "API password removed");
    } catch (e) {
      update({ ise: { ...st.ise, error: String(e) } });
    }
  }

  async function runApi() {
    if (!iseHost || !st.ise.username) {
      update({ ise: { ...st.ise, error: "Host and username are needed" } });
      return;
    }
    update({ ise: { ...st.ise, host: iseHost, running: true, error: null } });
    try {
      const api = await runIseApiChecks(iseHost, st.ise.username, st.ise.insecure);
      const base: CheckReport = st.report ?? { os: os ?? "ise", facts: f ?? { os: "ise", hostname: iseHost, model: "", version: "", image: "", uptime: "", serial: "", reloadReason: "" }, started: Math.floor(Date.now() / 1000), finished: Math.floor(Date.now() / 1000), cancelled: false, results: [], outputs: [] };
      const report = mergeApiResults(base, api.results);
      update({ report, ise: { ...st.ise, host: iseHost, running: false, saved: true } });
      const bad = api.results.filter((r) => r.status === "critical" || r.status === "warning").length;
      onToast(bad ? `API checks: ${bad} need attention` : "API checks healthy");
    } catch (e) {
      update({ ise: { ...st.ise, running: false, error: String(e) } });
    }
  }

  return (
    <div className="tools-body">
      <div className="tools-actions">
        {st.running ? (
          <button className="btn" onClick={() => sessionId && void cancelChecks(sessionId)}><StopIcon /> Stop</button>
        ) : (
          <>
            <button className="btn primary" disabled={!sessionId} title="System, resources, hardware, interface errors, logs, time: the checks that matter every visit" onClick={() => void run("quick")}><PlayIcon /> Quick</button>
            <button className="btn" disabled={!sessionId} title="Every check for this platform" onClick={() => void run("detailed")}>Detailed</button>
            {st.pre ? (
              <button className="btn" disabled={!sessionId} title="Run the same checks again and compare with the pre-change snapshot" onClick={() => void run()}>Post-change</button>
            ) : (
              <button className="btn" disabled={!st.report} title="Keep the latest run as the pre-change snapshot; the next run is compared with it" onClick={() => update({ pre: st.report })}>Pre-change</button>
            )}
          </>
        )}
        <button className={`btn${pickOpen ? " on" : ""}`} onClick={() => setPickOpen((v) => !v)} title="Choose which checks run">
          {st.selected.length ? `${st.selected.length} of ${catalog.length}` : "Pick"}
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
          <select className="os-pick" value={st.osOverride ?? f.os ?? "unknown"} title="Platform: detected from show version. Pick one when detection is inconclusive or wrong." onChange={(e) => { const v = e.target.value as Os; update({ osOverride: v === (f.os ?? "unknown") ? null : v }); }}>
            {(f.os === null || f.os === "unknown") && <option value="unknown">Unknown OS (choose)</option>}
            {OS_CHOICES.map((o) => <option key={o} value={o}>{OS_LABEL[o]}{o === f.os ? " (detected)" : ""}</option>)}
          </select>
          <span>{f.version}</span>
          {f.model && <span>{f.model}</span>}
          {f.serial && <span>SN {f.serial}</span>}
          {f.uptime && <span>up {f.uptime}</span>}
          {wireless && (
            <label className="expected-aps" title="How many APs this controller should have joined. The Access points check turns warning or critical when fewer are joined.">
              Expected APs
              <input type="number" min={0} value={st.expectedAps || ""} placeholder="any" onChange={(e) => {
                const n = Math.max(0, Number(e.target.value) || 0);
                update({ expectedAps: n, report: st.report ? applyExpectedAps({ ...st.report, results: st.report.results.map((r) => r.id === "ap-inventory" && /expected/.test(r.summary) ? { ...r, status: r.status === "critical" || r.status === "warning" ? "healthy" : r.status, summary: r.summary.replace(/ \(expected \d+\)$/, "").replace(/^(\d+) of \d+ expected APs joined.*$/, "$1 APs joined"), nextStep: "" } : r) }, n) : null });
              }} />
            </label>
          )}
        </div>
      )}

      {os === "ise" && (
        <details className="ise-api" open={!st.report}>
          <summary>ISE API checks: deployment sync, certificates, AD, licensing, live sessions</summary>
          <div className="ise-api-form">
            <input placeholder="Admin node (host or address)" value={st.ise.host} onChange={(e) => update({ ise: { ...st.ise, host: e.target.value } })} spellCheck={false} />
            <input placeholder="API user (ERS Admin / Super Admin)" value={st.ise.username} onChange={(e) => update({ ise: { ...st.ise, username: e.target.value } })} spellCheck={false} autoComplete="off" />
            <input type="password" placeholder={st.ise.saved ? "Password saved in the OS keychain" : "Password (saved to the OS keychain)"} value={isePassword} onChange={(e) => setIsePassword(e.target.value)} autoComplete="new-password" />
            <button className="btn small" disabled={!iseHost || !isePassword} onClick={() => void savePassword()}>Save</button>
            <label className="check"><input type="checkbox" checked={st.ise.insecure} onChange={(e) => update({ ise: { ...st.ise, insecure: e.target.checked } })} /> Accept self-signed admin certificate</label>
            <button className="btn primary small" disabled={st.ise.running || !iseHost || !st.ise.username} onClick={() => void runApi()}>{st.ise.running ? "Running…" : "Run API checks"}</button>
          </div>
          <div className="quiet small">Uses the OpenAPI (443) and ERS (9060) with the saved password. Enable both under Administration › System › Settings › API Settings. Nothing is changed on ISE.</div>
          {st.ise.error && <div className="tools-error">{st.ise.error}</div>}
        </details>
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
          {(["critical", "warning", "not-collected", "healthy", "unsupported", "not-applicable"] as Status[]).map((s) => counts[s] > 0 && <span key={s} className={`status-chip ${s}`}>{counts[s]} {STATUS_LABEL[s].toLowerCase()}</span>)}
          {st.report && <span className="quiet">{stamp(st.report.finished)}{st.report.cancelled ? " (stopped early)" : ""}{st.selected.length ? "" : st.tier === "quick" ? " · quick" : " · detailed"}</span>}
        </div>
      )}

      {st.pre && (
        <div className="tools-baseline">
          <span>Pre-change snapshot {stamp(st.pre.finished)}</span>
          {st.report && st.report !== st.pre && <span className={changes.length ? "warn-text" : "quiet"}>{changes.length ? `${changes.length} change${changes.length === 1 ? "" : "s"} since` : "no status changes since"}</span>}
          <button className="btn small" onClick={() => update({ pre: null })}>Forget</button>
        </div>
      )}
      {!st.pre && (baselines.length > 0 || st.baseline) && (
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
                {r.nextStep && <div className="next-step"><span className="strong">Next step:</span> {r.nextStep}</div>}
                <div className="quiet small">{STATUS_LABEL[r.status]}{r.collectedAt ? ` · collected ${stamp(r.collectedAt)}` : ""}</div>
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

function Configs({ sessionId, tabKey, deviceName, site, environment, onToast, st, update }: Props & { st: TabTools; update: (p: Partial<TabTools>) => void }) {
  const [all, setAll] = useState<Backup[]>([]);
  const [onlyThis, setOnlyThis] = useState(true);
  const [show, setShow] = useState<"backups" | "captures" | "all">("backups");
  const [profile, setProfile] = useState<Profile | null>(null);
  const [lifecycle, setLifecycle] = useState<{ action: string; steps: { command: string; output: string; ok: boolean }[] } | null>(null);
  const [lifeAsk, setLifeAsk] = useState<{ action: "save" | "checkpoint" | "rollback"; name: string; typed: string } | null>(null);
  const [restore, setRestore] = useState<{ backup: Backup; lines: string[] } | null>(null);
  const [checkpoints, setCheckpoints] = useState<string[]>([]);

  useEffect(() => {
    void platformProfile(st.facts?.os ?? null).then(setProfile).catch(() => setProfile(null));
  }, [st.facts?.os]);
  const wf = profile?.workflow;
  // kinds whose text is the restorable configuration on this platform
  const restorableKinds = useMemo(() => {
    if (!profile) return ["running"];
    const k = profile.configs.filter((c) => c.command === profile.workflow.restorable).map((c) => c.kind);
    return k.length ? k : [profile.configs[0]?.kind ?? "running"];
  }, [profile]);

  async function runLifecycle(action: "save" | "checkpoint" | "rollback" | "history", name = "") {
    if (!sessionId) return;
    setBusy(action);
    try {
      const steps = await lifecycleRun(sessionId, st.facts?.os ?? null, action, name);
      setLifecycle({ action, steps });
      if (action === "history") {
        // checkpoint names for the rollback picker: first word of each listed row that looks like a name
        const names = steps.flatMap((s) => s.output.split(/\r?\n/)).map((l) => l.trim()).filter((l) => /^[A-Za-z0-9][\w.-]*(\s|$)/.test(l) && !/^(name|id|user|-{3,}|total|checkpoint\s+summary|directory|\d+\s+bytes)/i.test(l)).map((l) => l.split(/\s+/)[0]).filter((n, i, a) => a.indexOf(n) === i);
        setCheckpoints(names.slice(0, 50));
      } else {
        const ok = steps.every((x) => x.ok);
        onToast(ok ? `${action === "save" ? "Configuration saved" : action === "checkpoint" ? `Checkpoint ${name} created` : "Rolled back"}` : `${action} reported a problem; check the output`);
        try {
          const list = ((await loadData<AuditEntry[]>("audit")) ?? []).slice(-5000);
          list.push({ time: Math.floor(Date.now() / 1000), device: st.facts?.hostname || deviceName, environment, kind: action === "rollback" ? "disruptive" : "config", commands: steps.map((x) => x.command), backup: null, result: ok ? "sent" : "failed", note: `Lifecycle: ${action}${name ? ` ${name}` : ""}` });
          await saveData("audit", list);
        } catch {
          // the audit trail never blocks the work
        }
      }
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openRestore(b: Backup) {
    try {
      const lines = restorableLines(await readConfig(b.path));
      if (!lines.length) return onToast("This backup has no configuration lines to restore");
      setRestore({ backup: b, lines });
    } catch (e) {
      onToast(String(e));
    }
  }
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

  const backups = useMemo(() => all.filter((b) => b.kind !== "checks" && (show === "all" || (show === "captures") === b.capture) && (!onlyThis || !deviceName || b.device === safe(deviceName))), [all, onlyThis, deviceName, show]);

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

      {wf && sessionId && (
        <div className="lifecycle">
          <span className="quiet small">{profile!.label} lifecycle:</span>
          {wf.save.length > 0 && <button className="btn small" disabled={!!busy} title={wf.save.join(" ; ")} onClick={() => setLifeAsk({ action: "save", name: "", typed: "" })}>Save config</button>}
          {wf.checkpoint && <button className="btn small" disabled={!!busy} title={wf.checkpoint} onClick={() => setLifeAsk({ action: "checkpoint", name: `omni-${new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "")}`, typed: "" })}>Checkpoint…</button>}
          {wf.rollback && <button className="btn small danger" disabled={!!busy} title={wf.rollback} onClick={() => { setLifeAsk({ action: "rollback", name: checkpoints[0] ?? "", typed: "" }); if (wf.history && !checkpoints.length) void runLifecycle("history"); }}>Roll back…</button>}
          {wf.history && <button className="btn small" disabled={!!busy} title={wf.history} onClick={() => void runLifecycle("history")}>{busy === "history" ? "Reading…" : "History"}</button>}
          {wf.candidate && <span className="quiet small">commits are persistent; rollback re-applies the previous commit</span>}
        </div>
      )}
      {lifeAsk && (
        <div className="review confirm lifecycle-ask">
          <div className="strong">{lifeAsk.action === "save" ? `Save the configuration with ${wf?.save.join(" ; ")}` : lifeAsk.action === "checkpoint" ? "Create a rollback point on the device" : "Roll the device back"}</div>
          {lifeAsk.action === "checkpoint" && <div className="row"><input value={lifeAsk.name} onChange={(e) => setLifeAsk({ ...lifeAsk, name: e.target.value })} spellCheck={false} placeholder="checkpoint name" /><code className="quiet small">{wf?.checkpoint.replace("{name}", lifeAsk.name || "<name>")}</code></div>}
          {lifeAsk.action === "rollback" && (
            <div className="row">
              {wf?.rollback.includes("{name}") ? (
                <>
                  <select value={lifeAsk.name} onChange={(e) => setLifeAsk({ ...lifeAsk, name: e.target.value })}>
                    <option value="">{checkpoints.length ? "Pick a checkpoint" : "No checkpoints listed (press History)"}</option>
                    {checkpoints.map((c) => <option key={c} value={c}>{c}</option>)}
                  </select>
                  <input value={lifeAsk.name} onChange={(e) => setLifeAsk({ ...lifeAsk, name: e.target.value })} placeholder="or type the name" spellCheck={false} />
                </>
              ) : <code>{wf?.rollback}{wf?.candidate ? ` ; ${wf.commit.join(" ; ")}` : ""}</code>}
            </div>
          )}
          <p className="quiet small">{lifeAsk.action === "rollback" ? "The running configuration is replaced on the device. Take a backup first if you have not." : "Runs on the device after you confirm."}</p>
          {environment === "production" && lifeAsk.action !== "save" && (
            <div className="row"><span className="warn-text small">Production device: type its name to confirm.</span><input value={lifeAsk.typed} onChange={(e) => setLifeAsk({ ...lifeAsk, typed: e.target.value })} spellCheck={false} /></div>
          )}
          <div className="row">
            <button className="btn small" onClick={() => setLifeAsk(null)}>Cancel</button>
            <button className={`btn small ${lifeAsk.action === "rollback" ? "danger-fill" : "primary"}`} disabled={!!busy || (lifeAsk.action !== "save" && wf?.[lifeAsk.action === "checkpoint" ? "checkpoint" : "rollback"].includes("{name}") && !lifeAsk.name.trim()) || (environment === "production" && lifeAsk.action !== "save" && lifeAsk.typed.trim() !== (st.facts?.hostname || deviceName))} onClick={() => { const a = lifeAsk; setLifeAsk(null); void runLifecycle(a.action, a.name.trim()); }}>{lifeAsk.action === "save" ? "Save" : lifeAsk.action === "checkpoint" ? "Create checkpoint" : "Roll back now"}</button>
          </div>
        </div>
      )}
      {lifecycle && (
        <details className="raw" open>
          <summary>{lifecycle.action}: {lifecycle.steps.length} command{lifecycle.steps.length === 1 ? "" : "s"}{lifecycle.steps.every((x) => x.ok) ? "" : " · reported a problem"} <button className="link" onClick={(e) => { e.preventDefault(); setLifecycle(null); }}>hide</button></summary>
          {lifecycle.steps.map((x, i) => <pre key={i} className={x.ok ? "" : "warn-text"}>{`${x.command}\n${x.output || "(no output)"}`}</pre>)}
        </details>
      )}
      {restore && (
        <DeployFlow
          sessionId={sessionId}
          tabKey={tabKey}
          facts={st.facts}
          onFacts={(facts) => update({ facts })}
          deviceName={deviceName}
          site={site}
          environment={environment}
          onToast={onToast}
          title={`Restore ${restore.backup.kind} backup from ${stamp(restore.backup.timestamp)} (${restore.lines.length} lines)`}
          commands={restore.lines}
          verify={wf?.restorable ? [wf.restorable] : []}
          onClose={() => setRestore(null)}
          onDone={() => refresh()}
        />
      )}

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
            <button key={i} className="hit" onClick={() => void readConfig(h.path).then((text) => { setView({ backup: all.find((b) => b.path === h.path) ?? { path: h.path, site: h.site, device: h.device, kind: "running", timestamp: h.timestamp, size: 0, capture: false }, text }); setDiff(null); })}>
              <span className="quiet">{h.device} · {stamp(h.timestamp)} · line {h.lineNo}</span>
              <code>{h.line}</code>
            </button>
          ))}
        </div>
      )}

      <div className="tools-list-head">
        <select className="os-pick" value={show} onChange={(e) => setShow(e.target.value as "backups" | "captures" | "all")} title="Backups are configuration text you can restore; captures are timestamped show output kept for the record">
          <option value="backups">{all.filter((b) => !b.capture && b.kind !== "checks").length} backups</option>
          <option value="captures">{all.filter((b) => b.capture).length} captures</option>
          <option value="all">all</option>
        </select>
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
              <span className="strong">{b.device}{b.capture ? <span className="cred-badge capture">capture</span> : restorableKinds.includes(b.kind) && (!deviceName || b.device === safe(deviceName)) ? <span className="cred-badge restorable">restorable</span> : null}</span>
              <span className="quiet">{b.site} · {b.kind} · {stamp(b.timestamp)} · {Math.max(1, Math.round(b.size / 1024))} KB</span>
            </button>
            {!b.capture && restorableKinds.includes(b.kind) && sessionId && (!deviceName || b.device === safe(deviceName)) && <button className="btn small" title="Review and push these configuration lines back to the connected device, with a fresh backup and rollback point first" onClick={() => void openRestore(b)}>Restore…</button>}
            <button className="icon-btn" title="Show in folder" onClick={() => void revealConfig(b.path)}><FolderIcon /></button>
            <button className="icon-btn" title="Delete this backup" onClick={() => { if (window.confirm(`Delete ${label(b)}?`)) void deleteConfig(b.path).then(refresh).catch((e) => onToast(String(e))); }}><TrashIcon /></button>
          </div>
        ))}
        {backups.length === 0 && <div className="tools-empty"><p>{show === "captures" ? "No captures yet. In Collect, the folder button on a result keeps it here as a timestamped capture." : "No backups yet. Back up the running config of a connected device; later backups are compared with the previous one automatically."}</p></div>}
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
