import { useEffect, useState } from "react";
import { useTabState } from "../lib/tabstate";
import { loadData, saveData, saveText } from "../lib/backend";
import { classifyCommands, type AuditEntry, type Classified, type Kind } from "../lib/ai";
import { CHECKLIST_TEMPLATES, checklistText, newChecklist, type Checklist, type ItemState } from "../lib/checklists";
import {
  deployFinish, deployRollback, deployRun, firmwareInspect, firmwarePlan, human, SECRET_VARS, templateRender, templatesList,
  type DeployReport, type DeployStep, type FirmwareInfo, type Plan, type Template,
} from "../lib/deploy";
import { isUnlocked, lock, unlock } from "../lib/lock";
import { platformProfile, type Profile } from "../lib/platforms";
import { compareReports, detectDevice, runChecks, stamp, type Change, type CheckReport, type Facts } from "../lib/tools";
import { CloseIcon, RefreshIcon, SaveIcon } from "./Icons";

export type DeploySection = "templates" | "firmware" | "checklists" | "audit";

interface Props {
  sessionId: string | null;
  tabKey: string;
  facts: Facts | null;
  onFacts: (f: Facts) => void;
  deviceName: string;
  site: string;
  environment: string;
  onToast: (t: string) => void;
}

const KIND_LABEL: Record<Kind, string> = { "read-only": "read-only", config: "configuration change", disruptive: "disruptive" };

export default function DeployPanel(props: Props) {
  const [dp, update] = useTabState<{ section: DeploySection }>("deploy", props.tabKey, () => ({ section: "templates" }));
  const section = dp.section;
  const setSection = (section: DeploySection) => update({ section });
  return (
    <div className="tools-body">
      <div className="dialog-tabs small">
        {(["templates", "firmware", "checklists", "audit"] as DeploySection[]).map((s) => (
          <button key={s} className={section === s ? "on" : ""} onClick={() => setSection(s)}>{s === "templates" ? "Templates" : s === "firmware" ? "Firmware" : s === "checklists" ? "Checklists" : "Audit"}</button>
        ))}
      </div>
      {section === "templates" && <Templates key={props.tabKey} {...props} />}
      {section === "firmware" && <Firmware key={props.tabKey} {...props} />}
      {section === "checklists" && <Checklists {...props} />}
      {section === "audit" && <Audit {...props} />}
    </div>
  );
}

/* --------------------------------------------------------- deploy flow */

interface FlowProps extends Props {
  title: string;
  commands: string[];
  verify: string[];
  /** a reload or install that restarts the device */
  disruptive?: boolean;
  onClose: () => void;
  onDone?: (report: DeployReport) => void;
}

/** Review → confirm → run with progress → verify → finish or roll back. */
export function DeployFlow({ sessionId, tabKey, facts, deviceName, site, environment, onToast, title, commands, verify, disruptive, onClose, onDone }: FlowProps) {
  const [items, setItems] = useState<Classified[]>([]);
  const [backup, setBackup] = useState(true);
  const [rollbackPoint, setRollbackPoint] = useState(true);
  const [safety, setSafety] = useState(0);
  const [reviewed, setReviewed] = useState(false);
  const [typed, setTyped] = useState("");
  const [unlockTyped, setUnlockTyped] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ phase: string; command: string; index: number; total: number } | null>(null);
  const [steps, setSteps] = useState<DeployStep[]>([]);
  const [report, setReport] = useState<DeployReport | null>(null);
  const [finish, setFinish] = useState<DeployStep[] | null>(null);
  const [busy, setBusy] = useState(false);
  // quick health check before and after, so the change is judged by what moved
  const [snapshot, setSnapshot] = useState(true);
  const [pre, setPre] = useState<CheckReport | null>(null);
  const [post, setPost] = useState<{ report: CheckReport; changes: Change[] } | null>(null);
  const [snapping, setSnapping] = useState<"pre" | "post" | null>(null);
  const production = environment === "production";
  const name = facts?.hostname || deviceName;
  const kind: Kind = disruptive || items.some((i) => i.kind === "disruptive") ? "disruptive" : items.some((i) => i.kind === "config") ? "config" : "read-only";
  const [profile, setProfile] = useState<Profile | null>(null);
  useEffect(() => {
    void platformProfile(facts?.os ?? null).then(setProfile).catch(() => setProfile(null));
  }, [facts?.os]);
  // a timed commit exists where the profile has one; the IOS safety reload only on IOS / IOS-XE
  const candidate = !!profile?.workflow.candidate && !!profile.workflow.commitConfirmed;
  const canIosSafety = facts?.os === "ios" || facts?.os === "ios-xe" || !facts?.os || candidate;

  useEffect(() => {
    void classifyCommands(commands).then(setItems);
  }, [commands]);

  async function audit(result: AuditEntry["result"], note: string, backupPath: string | null) {
    try {
      const list = ((await loadData<AuditEntry[]>("audit")) ?? []).slice(-5000);
      list.push({ time: Math.floor(Date.now() / 1000), device: name, environment, kind, commands, backup: backupPath, result, note: `${title}. ${note}`.trim() });
      await saveData("audit", list);
    } catch {
      // never block on the audit file
    }
  }

  async function takeSnapshot(phase: "pre" | "post") {
    if (!sessionId) return null;
    setSnapping(phase);
    try {
      return await runChecks(sessionId, facts, [], "quick", () => {}, null);
    } catch (e) {
      onToast(`Health snapshot (${phase}) failed: ${e}`);
      return null;
    } finally {
      setSnapping(null);
    }
  }

  async function run() {
    if (!sessionId) return;
    setBusy(true);
    setSteps([]);
    try {
      if (snapshot && kind !== "read-only") setPre(await takeSnapshot("pre"));
      const r = await deployRun(sessionId, facts?.os ?? null, site, name, commands, verify, { backup, rollbackPoint, safetyReloadMinutes: safety }, (e) => {
        if (e.type === "step") setProgress({ phase: e.phase, command: e.command, index: e.index, total: e.total });
        else setSteps((s) => [...s, { phase: e.phase, command: e.command, output: e.output, ok: e.ok }]);
      });
      setReport(r);
      void audit(r.failed ? "failed" : "sent", r.failed ? r.error : `${r.sent} lines sent`, r.backup);
      onToast(r.failed ? `Stopped: ${r.error}` : `${r.sent} line${r.sent === 1 ? "" : "s"} sent${r.backup ? ", backup saved" : ""}`);
      onDone?.(r);
    } catch (e) {
      onToast(String(e));
      void audit("failed", String(e), null);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function doFinish(save: boolean) {
    if (!sessionId || !report) return;
    setBusy(true);
    try {
      const s = await deployFinish(sessionId, facts?.os ?? null, report.safetyReload, save);
      setFinish(s);
      onToast(save ? (report.saveCommand ? "Saved" : "Confirmed") : report.safetyReload ? (report.saveCommand ? "Safety reload cancelled" : "Left to revert") : "Done");
      if (pre) {
        const after = await takeSnapshot("post");
        if (after) setPost({ report: after, changes: compareReports(pre, after) });
      }
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function doRollback() {
    if (!sessionId || !report?.rollbackCommand) return;
    setBusy(true);
    try {
      const s = await deployRollback(sessionId, facts?.os ?? null, report.rollbackCommand);
      setFinish((f) => [...(f ?? []), s]);
      void audit(s.ok ? "sent" : "failed", `Rolled back with ${report.rollbackCommand}`, report.backup);
      onToast(s.ok ? "Rolled back" : "Rollback reported a problem, check the output");
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(false);
    }
  }

  const locked = production && !isUnlocked(tabKey) && kind !== "read-only";
  const ready = !!sessionId && !busy && !report && reviewed && !locked && (kind !== "disruptive" || typed.trim() === name);

  return (
    <div className={`review ${kind} flow`}>
      <div className="review-head">
        <span className={`kind-chip ${kind}`}>{KIND_LABEL[kind]}</span>
        <span className="strong">{title}</span>
        <span className="grow" />
        <button className="icon-btn" title="Close" onClick={onClose}><CloseIcon /></button>
      </div>
      <ol className="review-lines">
        {(items.length ? items : commands.map((c) => ({ command: c, kind: "config" as Kind, reason: "" }))).map((it, k) => (
          <li key={k} className={it.kind}><code>{it.command}</code>{it.reason && <span className="quiet"> {it.reason}</span>}</li>
        ))}
      </ol>
      {verify.length > 0 && <div className="quiet small">Verified afterwards with: {verify.join(", ")}</div>}

      {!report && (
        <div className="confirm">
          {locked ? (
            unlockTyped === null ? (
              <p className="warn-text">This is a production device. <button className="link" onClick={() => setUnlockTyped("")}>Unlock configuration changes for this tab</button> first.</p>
            ) : (
              <>
                <p>Type the device name <code>{name}</code> to unlock this tab.</p>
                <div className="row">
                  <input autoFocus value={unlockTyped} onChange={(e) => setUnlockTyped(e.target.value)} spellCheck={false} />
                  <button className="btn small primary" disabled={unlockTyped.trim() !== name} onClick={() => { unlock(tabKey); setUnlockTyped(null); }}>Unlock</button>
                </div>
              </>
            )
          ) : (
            <>
              {production && isUnlocked(tabKey) && <p className="warn-text">Production unlocked for this tab. <button className="link" onClick={() => lock(tabKey)}>Lock again</button></p>}
              <label className="check"><input type="checkbox" checked={backup} onChange={(e) => setBackup(e.target.checked)} />Back up the running config first</label>
              <label className="check" title="IOS: copy running-config to flash and offer configure replace. NX-OS and AOS-CX: a checkpoint. EOS: a saved checkpoint. Junos, IOS-XR and NVUE keep their own history: rollback 1 / the saved startup.">
                <input type="checkbox" checked={rollbackPoint} onChange={(e) => setRollbackPoint(e.target.checked)} />Save a rollback point on the device
              </label>
              <label className="check" title="Runs the Quick health check before the change and again after you finish it, and lists every status that moved">
                <input type="checkbox" checked={snapshot} onChange={(e) => setSnapshot(e.target.checked)} />Health snapshot before and after
              </label>
              {canIosSafety && (
                <label className="check" title={candidate ? "commit confirmed N: the change reverts on its own unless you confirm it" : "reload in N: if you lose access the device reboots to the saved config; cancelled when you confirm the result"}>
                  {candidate ? "Timed commit" : "Safety reload"}
                  <select value={safety} onChange={(e) => setSafety(Number(e.target.value))}>
                    <option value={0}>off</option>
                    <option value={10}>in 10 min</option>
                    <option value={15}>in 15 min</option>
                    <option value={30}>in 30 min</option>
                  </select>
                </label>
              )}
              <label className="check"><input type="checkbox" checked={reviewed} onChange={(e) => setReviewed(e.target.checked)} />I reviewed every line above</label>
              {kind === "disruptive" && (
                <label className="typed">This will disrupt the device. Type <code>{name}</code> to confirm<input value={typed} onChange={(e) => setTyped(e.target.value)} spellCheck={false} /></label>
              )}
              <div className="row">
                <button className="btn small" onClick={() => void navigator.clipboard.writeText(commands.join("\n"))}>Copy</button>
                <button className={`btn small ${kind === "read-only" ? "primary" : "danger-fill"}`} disabled={!ready} onClick={() => void run()}>
                  {busy ? "Sending…" : `Send ${commands.length} line${commands.length === 1 ? "" : "s"}`}
                </button>
                {!sessionId && <span className="quiet small">connect to deploy</span>}
              </div>
            </>
          )}
        </div>
      )}

      {progress && (
        <div className="tools-progress">
          <div className="bar"><div style={{ width: `${progress.phase === "send" ? ((progress.index + 1) / Math.max(1, progress.total)) * 100 : 5}%` }} /></div>
          <span>{progress.phase}: {progress.command}</span>
        </div>
      )}
      {steps.length > 0 && (
        <details className="raw" open={!!report?.failed}>
          <summary>{steps.length} step{steps.length === 1 ? "" : "s"}{report?.failed ? " · stopped with an error" : ""}{report?.backup ? " · backup saved" : ""}</summary>
          {steps.map((s, i) => <pre key={i} className={s.ok ? "" : "warn-text"}>{`[${s.phase}] ${s.command}\n${s.output || "(no output)"}`}</pre>)}
        </details>
      )}
      {report && !finish && (
        <div className="confirm">
          {report.failed ? <p className="warn-text">Stopped at line {report.sent + 1}: {report.error}. Nothing after it was sent.</p> : <p>All {report.sent} lines were accepted. Check the verify output above, then:</p>}
          <div className="row">
            {!report.failed && <button className="btn small primary" disabled={busy} onClick={() => void doFinish(true)}>{report.safetyReload ? (report.saveCommand ? "Looks good: cancel the reload and save" : "Looks good: confirm the commit") : report.saveCommand ? `Looks good: ${report.saveCommand.split("\n")[0]}` : "Looks good"}</button>}
            <button className="btn small" disabled={busy} onClick={() => void doFinish(false)}>{report.safetyReload ? (report.saveCommand ? "Cancel the reload, don't save" : "Let it revert") : "Leave unsaved"}</button>
            {report.rollbackCommand && <button className="btn small danger" disabled={busy} onClick={() => void doRollback()} title={report.rollbackCommand}>Roll back</button>}
            {report.safetyReload && <span className="warn-text small">Pending on the device: {report.pending}.</span>}
          </div>
          {report.backup && <div className="quiet small">Backup: {report.backup}</div>}
        </div>
      )}
      {finish && finish.map((s, i) => <pre key={i} className={s.ok ? "" : "warn-text"}>{`${s.command}\n${s.output}`}</pre>)}
      {snapping && <div className="quiet small">Taking the {snapping === "pre" ? "pre-change" : "post-change"} health snapshot…</div>}
      {pre && !post && !snapping && <div className="quiet small">Pre-change snapshot taken {stamp(pre.finished)}: {pre.results.filter((r) => r.status === "critical").length} critical, {pre.results.filter((r) => r.status === "warning").length} warning. The post-change snapshot runs when you finish.</div>}
      {post && (
        <div className="snapshot-delta">
          <div className={post.changes.length ? "warn-text" : "ok-text"}>{post.changes.length ? `${post.changes.length} health status${post.changes.length === 1 ? "" : "es"} changed after this change` : "No health status changed after this change"} <span className="quiet">(pre {stamp(pre!.finished)} → post {stamp(post.report.finished)})</span></div>
          {post.changes.length > 0 && (
            <ul className="tools-changes">
              {post.changes.map((c) => <li key={c.id}><span className={`status-chip ${c.from}`}>{c.from}</span> → <span className={`status-chip ${c.to}`}>{c.to}</span> {c.title}: {c.summary}</li>)}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

/* ----------------------------------------------------------- templates */

function Templates(props: Props) {
  const { sessionId, facts, onFacts, onToast } = props;
  const [list, setList] = useState<Template[]>([]);
  const [tpl, setTpl] = useState<Template | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [rendered, setRendered] = useState<{ commands: string[]; warnings: string[] } | null>(null);

  useEffect(() => {
    void templatesList().then(setList);
  }, []);

  function pick(t: Template) {
    setTpl(t);
    setValues(Object.fromEntries(t.vars.map((v) => [v.name, v.default])));
    setErrors({});
    setRendered(null);
  }

  async function render() {
    if (!tpl) return;
    let f = facts;
    if (!f && sessionId) {
      try {
        f = await detectDevice(sessionId);
        onFacts(f);
      } catch {
        // render with IOS syntax
      }
    }
    const r = await templateRender(tpl.id, f?.os ?? null, values);
    if (r.result === "ok") {
      setErrors({});
      setRendered(r.rendered);
    } else {
      setErrors(Object.fromEntries(r.errors.map((e) => [e.name, e.message])));
      setRendered(null);
      onToast(r.errors[0].message);
    }
  }

  if (!tpl) {
    const cats = Array.from(new Set(list.map((t) => t.category)));
    return (
      <>
        <div className="quiet small">Fill in the variables, review the exact lines, then deploy with a backup and rollback point. {facts?.os ? `Syntax: ${facts.os === "nx-os" ? "NX-OS" : "IOS"}.` : "Connect to render for the device's OS; IOS syntax otherwise."}</div>
        {cats.map((c) => (
          <div key={c}>
            <div className="tools-pick-title">{c}</div>
            <div className="wf-list">
              {list.filter((t) => t.category === c).map((t) => (
                <button key={t.id} className="wf-card" onClick={() => pick(t)} disabled={facts?.os === "nx-os" && t.nxos.length === 0} title={facts?.os === "nx-os" && t.nxos.length === 0 ? "No NX-OS version" : ""}>
                  <span className="strong">{t.title}</span>
                  <span className="quiet">{t.description}</span>
                </button>
              ))}
            </div>
          </div>
        ))}
      </>
    );
  }

  const secrets = SECRET_VARS.filter((s) => s.template === tpl.id && (!s.name.startsWith("snmp") || values.snmp));
  return (
    <>
      <div className="tools-actions">
        <button className="btn small" onClick={() => setTpl(null)}>← Templates</button>
        <span className="strong">{tpl.title}</span>
      </div>
      <div className="quiet small">{tpl.description}</div>
      <div className="form tpl-form">
        {tpl.vars.map((v) => (
          <FieldRow key={v.name} label={v.label} help={v.help} error={errors[v.name]} required={v.required}>
            {v.kind === "bool" ? (
              <input type="checkbox" checked={!!values[v.name] && values[v.name] !== "false"} onChange={(e) => setValues({ ...values, [v.name]: e.target.checked ? "true" : "" })} />
            ) : v.kind === "choice" ? (
              <select value={values[v.name]} onChange={(e) => setValues({ ...values, [v.name]: e.target.value })}>{v.choices.map((c) => <option key={c}>{c}</option>)}</select>
            ) : (
              <input value={values[v.name] ?? ""} onChange={(e) => setValues({ ...values, [v.name]: e.target.value })} placeholder={v.help} spellCheck={false} className={errors[v.name] ? "invalid" : ""} />
            )}
          </FieldRow>
        ))}
        {secrets.map((s) => (
          <FieldRow key={s.name} label={s.label} help="Used once, never stored" error={errors[s.name]} required>
            <input type="password" value={values[s.name] ?? ""} onChange={(e) => setValues({ ...values, [s.name]: e.target.value })} autoComplete="off" className={errors[s.name] ? "invalid" : ""} />
          </FieldRow>
        ))}
      </div>
      <div className="row">
        <button className="btn small primary" onClick={() => void render()}>Render and review</button>
      </div>
      {rendered && (
        <>
          {rendered.warnings.map((w, i) => <div key={i} className="warn-text small">{w}</div>)}
          <DeployFlow {...props} title={tpl.title} commands={rendered.commands} verify={verifyFor(tpl.id, values)} onClose={() => setRendered(null)} />
        </>
      )}
    </>
  );
}

function verifyFor(id: string, v: Record<string, string>): string[] {
  const first = (v.if ?? "").split(/[,\s]/)[0].replace(/-\d+$/, "");
  switch (id) {
    case "access-port": case "ap-port": case "camera-port": case "voice-port": case "trunk-port": case "uplink-port":
      return first ? [`show interfaces ${first} switchport`, `show interfaces ${first} status`] : [];
    case "port-channel": return [`show etherchannel ${v.po} summary`];
    case "vlan": return [`show vlan id ${v.vlan}`];
    case "initial-setup": return ["show ip interface brief", "show ip ssh", "show ntp status"];
    default: return [];
  }
}

function FieldRow({ label, help, error, required, children }: { label: string; help: string; error?: string; required?: boolean; children: React.ReactNode }) {
  return (
    <>
      <label>{label}{required ? "" : <span className="quiet"> (optional)</span>}</label>
      <div className="field">{children}{error ? <div className="warn-text small">{error}</div> : help && <div className="hint">{help}</div>}</div>
    </>
  );
}

/* ------------------------------------------------------------ firmware */

function Firmware(props: Props) {
  const { sessionId, onToast } = props;
  const [info, setInfo] = useState<FirmwareInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [url, setUrl] = useState("");
  const [image, setImage] = useState("");
  const [size, setSize] = useState("");
  const [md5, setMd5] = useState("");
  const [plan, setPlan] = useState<Plan | null>(null);
  const [stage, setStage] = useState<"transfer" | "verify" | "activate" | "reload" | "post" | null>(null);

  async function inspect() {
    if (!sessionId) return;
    setBusy(true);
    try {
      setInfo(await firmwareInspect(sessionId));
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    const name = url.split("/").pop() ?? "";
    if (name && !image) setImage(name);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url]);

  async function makePlan() {
    if (!info) return;
    const mb = Number(size);
    setPlan(await firmwarePlan(info, url.trim(), image.trim(), Number.isFinite(mb) && mb > 0 ? Math.round(mb * 1024 * 1024) : 0, md5.trim()));
    setStage(null);
  }

  const stageCommands = (s: NonNullable<typeof stage>) => (plan ? plan[s] : []);

  return (
    <>
      <div className="tools-actions">
        <button className="btn small primary" disabled={!sessionId || busy} onClick={() => void inspect()}><RefreshIcon /> {busy ? "Reading…" : info ? "Read again" : "Read the device"}</button>
        <span className="quiet small">Runs show version, dir and show boot.</span>
      </div>
      {info && (
        <div className="tools-facts">
          <span className="strong">{info.model || "unknown model"}</span>
          <span>{info.version}{info.mode ? ` (${info.mode} mode)` : ""}</span>
          {info.boot && <span>boot: {info.boot}</span>}
          {info.filesystems.map((f) => <span key={f.name}>{f.name} {human(f.freeBytes)} free of {human(f.totalBytes)}</span>)}
          {info.stackMembers > 1 && <span>{info.stackMembers} stack members</span>}
        </div>
      )}
      {info && (
        <div className="form tpl-form">
          <FieldRow label="Image source" help="tftp://10.0.0.5/image.bin, scp://user@host/image.bin, http(s)://…, or usb0:/image.bin; reachable from the device" required>
            <input value={url} onChange={(e) => setUrl(e.target.value)} spellCheck={false} />
          </FieldRow>
          <FieldRow label="File name" help="as it will be stored on the device" required>
            <input value={image} onChange={(e) => setImage(e.target.value)} spellCheck={false} />
          </FieldRow>
          <FieldRow label="Size (MB)" help="from the download page, for the storage check">
            <input value={size} onChange={(e) => setSize(e.target.value)} spellCheck={false} />
          </FieldRow>
          <FieldRow label="MD5" help="from the download page; verified on the device after the transfer">
            <input value={md5} onChange={(e) => setMd5(e.target.value)} spellCheck={false} />
          </FieldRow>
        </div>
      )}
      {info && (
        <div className="row"><button className="btn small primary" disabled={!image.trim()} onClick={() => void makePlan()}>Check and plan</button></div>
      )}
      {plan && (
        <>
          <div className="tools-results">
            {plan.checks.map((c, i) => (
              <div key={i} className={`check-row ${c.status}`}>
                <div className="check-head static"><span className={`status-dot ${c.status}`} /><span className="check-title">{c.title}</span><span className="check-summary wrap">{c.detail}</span></div>
              </div>
            ))}
          </div>
          {plan.notes.map((n, i) => <div key={i} className="quiet small">{n}</div>)}
          <div className="fw-stages">
            {(["transfer", "verify", "activate", "reload", "post"] as const).map((s) => (
              stageCommands(s).length > 0 && (
                <button key={s} className={`btn small${stage === s ? " on" : ""}`} disabled={!plan.ok && s !== "post"} onClick={() => setStage(s)}>
                  {s === "transfer" ? "1 · Transfer" : s === "verify" ? "2 · Verify" : s === "activate" ? "3 · Activate" : s === "reload" ? "4 · Reload" : "5 · Validate"}
                </button>
              )
            ))}
          </div>
          {!plan.ok && <div className="warn-text small">Fix the failed checks before transferring or activating.</div>}
          {stage && (
            <DeployFlow
              key={stage}
              {...props}
              title={`Firmware: ${stage}`}
              commands={stageCommands(stage)}
              verify={stage === "activate" ? ["show boot"] : []}
              disruptive={stage === "reload" || (stage === "activate" && (info?.os === "nx-os" || info?.mode === "install"))}
              onClose={() => setStage(null)}
            />
          )}
          {stage === "post" && <div className="quiet small">After the reload: run the health checks (Checks tab) and compare with the baseline you saved before the upgrade.</div>}
        </>
      )}
      {!info && <div className="tools-empty"><p>Read the device first: current version, image, boot variable, free flash and stack members. Then enter where the new image is and the planner checks compatibility, space and checksum before proposing the transfer, verification and activation as reviewed steps. The reload is always its own explicit step.</p></div>}
    </>
  );
}

/* ---------------------------------------------------------- checklists */

function Checklists({ deviceName, site, onToast }: Props) {
  const [lists, setLists] = useState<Checklist[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [onlyThis, setOnlyThis] = useState(true);

  useEffect(() => {
    void loadData<Checklist[]>("checklists").then((l) => setLists(l ?? []));
  }, []);

  function save(next: Checklist[]) {
    setLists(next);
    void saveData("checklists", next);
  }

  const shown = lists.filter((c) => !onlyThis || !deviceName || c.device === deviceName);
  const cur = lists.find((c) => c.id === open);

  function setItem(c: Checklist, i: number, patch: Partial<Checklist["items"][number]>) {
    const items = c.items.map((it, k) => (k === i ? { ...it, ...patch, time: patch.state && patch.state !== "open" ? Math.floor(Date.now() / 1000) : patch.state === "open" ? null : it.time } : it));
    save(lists.map((x) => (x.id === c.id ? { ...x, items } : x)));
  }

  if (cur) {
    const done = cur.items.filter((i) => i.state === "done").length;
    return (
      <>
        <div className="tools-actions">
          <button className="btn small" onClick={() => setOpen(null)}>← Checklists</button>
          <span className="strong">{cur.title}</span>
          <span className="quiet small">{cur.device} · {done}/{cur.items.length}</span>
          <span className="grow" />
          <button className="icon-btn" title="Save as a text file" onClick={() => void saveText(`${cur.device}_${cur.kind}_checklist.txt`, checklistText(cur), false).then((p) => p && onToast(`Saved ${p}`))}><SaveIcon /></button>
        </div>
        <ol className="cl-items">
          {cur.items.map((it, i) => (
            <li key={i} className={it.state}>
              <div className="cl-head">
                <span className="cl-text">{it.text}</span>
                <div className="segmented small">
                  {(["done", "skipped", "failed"] as ItemState[]).map((s) => (
                    <button key={s} className={it.state === s ? "on" : ""} onClick={() => setItem(cur, i, { state: it.state === s ? "open" : s })}>{s === "done" ? "Done" : s === "skipped" ? "Skip" : "Failed"}</button>
                  ))}
                </div>
              </div>
              <input className="cl-note" placeholder="Note (serial, result, who)" value={it.note} onChange={(e) => setItem(cur, i, { note: e.target.value })} spellCheck={false} />
              {it.time && <span className="quiet small">{stamp(it.time)}</span>}
            </li>
          ))}
        </ol>
      </>
    );
  }

  return (
    <>
      <div className="tools-actions">
        <select value="" onChange={(e) => { if (!e.target.value) return; const c = newChecklist(e.target.value, deviceName || "device", site); save([c, ...lists]); setOpen(c.id); }}>
          <option value="">New checklist…</option>
          {CHECKLIST_TEMPLATES.map((t) => <option key={t.kind} value={t.kind}>{t.title}</option>)}
        </select>
        {deviceName && <label className="check small"><input type="checkbox" checked={onlyThis} onChange={(e) => setOnlyThis(e.target.checked)} />this device only</label>}
      </div>
      <div className="tools-backups">
        {shown.map((c) => {
          const done = c.items.filter((i) => i.state === "done").length;
          const failed = c.items.filter((i) => i.state === "failed").length;
          return (
            <div key={c.id} className="backup">
              <button className="backup-main" onClick={() => setOpen(c.id)}>
                <span className="strong">{c.title} · {c.device}</span>
                <span className="quiet">{stamp(c.created)} · {done}/{c.items.length} done{failed ? `, ${failed} failed` : ""}</span>
              </button>
              <button className="icon-btn" title="Delete" onClick={() => window.confirm(`Delete this ${c.title} checklist for ${c.device}?`) && save(lists.filter((x) => x.id !== c.id))}><CloseIcon /></button>
            </div>
          );
        })}
        {shown.length === 0 && <div className="tools-empty"><p>Installation, cutover and upgrade checklists with a recorded result and note per item, saved per device and exportable as text.</p></div>}
      </div>
    </>
  );
}

/* --------------------------------------------------------------- audit */

function Audit({ deviceName, onToast }: Props) {
  const [entries, setEntries] = useState<AuditEntry[]>([]);
  const [onlyThis, setOnlyThis] = useState(false);
  const [open, setOpen] = useState<number | null>(null);

  useEffect(() => {
    void loadData<AuditEntry[]>("audit").then((l) => setEntries((l ?? []).slice().reverse()));
  }, []);

  const shown = entries.filter((e) => !onlyThis || !deviceName || e.device === deviceName);
  const text = () => shown.map((e) => `${stamp(e.time)}  ${e.device}${e.environment ? ` [${e.environment}]` : ""}  ${e.kind}  ${e.result}${e.backup ? `  backup: ${e.backup}` : ""}\n${e.note ? `  ${e.note}\n` : ""}${e.commands.map((c) => `    ${c}`).join("\n")}`).join("\n\n");

  return (
    <>
      <div className="tools-actions">
        <span className="quiet small">{shown.length} configuration change{shown.length === 1 ? "" : "s"} recorded</span>
        {deviceName && <label className="check small"><input type="checkbox" checked={onlyThis} onChange={(e) => setOnlyThis(e.target.checked)} />this device only</label>}
        <span className="grow" />
        <button className="icon-btn" title="Save as a text file" disabled={!shown.length} onClick={() => void saveText("omniconsole_audit.txt", text(), true).then((p) => p && onToast(`Saved ${p}`))}><SaveIcon /></button>
      </div>
      <div className="tools-backups">
        {shown.map((e, i) => (
          <div key={i} className={`audit${open === i ? " open" : ""}`}>
            <button className="backup-main" onClick={() => setOpen(open === i ? null : i)}>
              <span><span className={`kind-chip ${e.kind}`}>{e.kind}</span> <span className="strong">{e.device}</span> <span className={e.result === "sent" ? "quiet" : "warn-text"}>{e.result}</span></span>
              <span className="quiet">{stamp(e.time)} · {e.commands.length} line{e.commands.length === 1 ? "" : "s"}{e.note ? ` · ${e.note}` : ""}</span>
            </button>
            {open === i && <pre>{e.commands.join("\n")}{e.backup ? `\n\nbackup: ${e.backup}` : ""}</pre>}
          </div>
        ))}
        {shown.length === 0 && <div className="tools-empty"><p>Every configuration change sent from the assistant, a template or a deployment is recorded here with its lines, result and backup.</p></div>}
      </div>
    </>
  );
}
