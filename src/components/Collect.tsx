import { useEffect, useState } from "react";
import { saveText } from "../lib/backend";
import { collectedText, collectRun, platformProfile, type Collected, type Collection, type Profile } from "../lib/platforms";
import { useTabState } from "../lib/tabstate";
import { cancelChecks, detectDevice, OS_CHOICES, OS_LABEL, saveCapture, stamp, type Facts, type Os } from "../lib/tools";
import { FolderIcon, PlayIcon, SaveIcon, StopIcon } from "./Icons";

interface Props {
  sessionId: string | null;
  tabKey: string;
  facts: Facts | null;
  onFacts: (f: Facts) => void;
  deviceName: string;
  site: string;
  maskSecrets: boolean;
  onToast: (t: string) => void;
  onSend: (command: string) => void;
  onAsk: (question: string, output: string) => void;
}

interface State {
  os: Os | null;
  running: string | null;
  progress: { command: string; index: number; total: number } | null;
  /** latest result per collection id */
  results: Record<string, Collected>;
  open: string | null;
  error: string | null;
}

/** Quick actions per platform: the read-only command sets of the profile, run and kept per tab. */
export default function Collect({ sessionId, tabKey, facts, onFacts, deviceName, site, maskSecrets, onToast, onSend, onAsk }: Props) {
  const [st, update] = useTabState<State>("collect", tabKey, () => ({ os: null, running: null, progress: null, results: {}, open: null, error: null }));
  const [profile, setProfile] = useState<Profile | null>(null);
  const [tab, setTab] = useState<"collections" | "troubleshooting" | "workflow">("collections");
  const os: Os | null = st.os ?? facts?.os ?? null;

  useEffect(() => {
    void platformProfile(os).then(setProfile);
  }, [os]);

  async function detect() {
    if (!sessionId) return;
    try {
      const f = await detectDevice(sessionId);
      onFacts(f);
      update({ os: null });
    } catch (e) {
      update({ error: String(e) });
    }
  }

  async function run(c: Collection) {
    if (!sessionId || st.running) return;
    let target = os;
    if (!target || target === "unknown") {
      try {
        const f = await detectDevice(sessionId);
        onFacts(f);
        target = f.os;
      } catch (e) {
        update({ error: String(e) });
        return;
      }
      if (!target || target === "unknown") {
        update({ error: "The platform could not be detected. Pick it from the list above, then run again." });
        return;
      }
    }
    update({ running: c.id, error: null, progress: null, open: c.id });
    try {
      const r = await collectRun(sessionId, target, c.id, (e) => {
        if (e.type === "command") update({ progress: { command: e.command, index: e.index, total: e.total } });
      });
      update({ results: { ...st.results, [c.id]: r }, running: null, progress: null });
      const failed = r.outputs.filter(([, o]) => "Err" in o).length;
      onToast(failed ? `${c.title}: ${failed} of ${r.outputs.length} commands failed` : `${c.title}: ${r.outputs.length} commands collected`);
    } catch (e) {
      update({ running: null, progress: null, error: String(e) });
    }
  }

  async function save(c: Collected) {
    const name = `${deviceName || "device"}_${c.id}_${new Date(c.finished * 1000).toISOString().replace(/[:T]/g, "-").slice(0, 19)}.txt`;
    const path = await saveText(name, `${c.title} · ${deviceName} · ${stamp(c.finished)}\n\n${collectedText(c)}`, maskSecrets);
    if (path) onToast(`Saved ${path}`);
  }

  async function capture(c: Collected) {
    try {
      const b = await saveCapture(site, facts?.hostname || deviceName || "device", c.id, `${c.title} · ${deviceName} · ${stamp(c.finished)}\n\n${collectedText(c)}`);
      onToast(`Capture saved with the device's configs (${Math.max(1, Math.round(b.size / 1024))} KB)`);
    } catch (e) {
      onToast(String(e));
    }
  }

  const list = tab === "collections" ? profile?.collections ?? [] : tab === "troubleshooting" ? profile?.troubleshooting ?? [] : [];
  const wf = profile?.workflow;

  return (
    <div className="tools-body">
      <div className="tools-actions">
        <select className="os-pick" value={os ?? "unknown"} title="Platform: detected from show version. Pick one when detection is inconclusive." onChange={(e) => update({ os: e.target.value as Os })}>
          {(!os || os === "unknown") && <option value="unknown">Unknown platform (choose)</option>}
          {OS_CHOICES.map((o) => <option key={o} value={o}>{OS_LABEL[o]}{o === facts?.os ? " (detected)" : ""}</option>)}
        </select>
        <button className="btn small" disabled={!sessionId} onClick={() => void detect()} title="Run show version again and detect the platform">Detect</button>
        <span className="grow" />
        {st.running && <button className="btn small" onClick={() => sessionId && void cancelChecks(sessionId)}><StopIcon /> Stop</button>}
      </div>
      <div className="dialog-tabs small">
        <button className={tab === "collections" ? "on" : ""} onClick={() => setTab("collections")}>Quick actions</button>
        <button className={tab === "troubleshooting" ? "on" : ""} onClick={() => setTab("troubleshooting")}>Troubleshooting</button>
        <button className={tab === "workflow" ? "on" : ""} onClick={() => setTab("workflow")}>Change workflow</button>
      </div>
      {profile && <div className="quiet small">{profile.vendor} {profile.label}{profile.reference ? ` · ${profile.reference}` : ""}</div>}
      {st.progress && (
        <div className="tools-progress">
          <div className="bar"><div style={{ width: `${((st.progress.index + 1) / Math.max(1, st.progress.total)) * 100}%` }} /></div>
          <span>{st.progress.index + 1}/{st.progress.total} · {st.progress.command}</span>
        </div>
      )}
      {st.error && <div className="tools-error">{st.error}</div>}

      {tab !== "workflow" && (
        <div className="tools-results">
          {list.map((c) => {
            const r = st.results[c.id];
            const isOpen = st.open === c.id;
            const failed = r ? r.outputs.filter(([, o]) => "Err" in o).length : 0;
            return (
              <div key={c.id} className={`check-row${isOpen ? " open" : ""}${c.commands.length ? "" : " not-applicable"}`}>
                <button className="check-head" onClick={() => update({ open: isOpen ? null : c.id })}>
                  <span className={`status-dot ${r ? (failed ? "warning" : "healthy") : c.commands.length ? "" : "not-applicable"}`} />
                  <span className="check-title">{c.title}</span>
                  <span className="check-summary">{r ? `${r.outputs.length} commands · ${stamp(r.finished)}${failed ? ` · ${failed} failed` : ""}` : c.commands.length ? c.commands.join(" · ") : c.note}</span>
                </button>
                {isOpen && (
                  <div className="check-detail">
                    {c.commands.length > 0 ? (
                      <div className="follow">
                        <button className="btn small primary" disabled={!sessionId || !!st.running} onClick={() => void run(c)}><PlayIcon /> {r ? "Collect again" : "Collect"}</button>
                        {c.commands.map((cmd) => <button key={cmd} className="btn small" disabled={!sessionId} title="Send to the terminal" onClick={() => onSend(cmd)}>{cmd}</button>)}
                      </div>
                    ) : (
                      <div className="quiet">{c.note || "Nothing to collect on this platform."}</div>
                    )}
                    {r && (
                      <>
                        <div className="row">
                          <span className="quiet small">Collected {stamp(r.finished)}{r.cancelled ? " (stopped early)" : ""}</span>
                          <span className="grow" />
                          <button className="btn small" onClick={() => onAsk(`Interpret this ${c.title.toLowerCase()} collection from the device: what it shows, anything abnormal, and what to check next.`, collectedText(r))}>Ask the assistant</button>
                          <button className="icon-btn" title="Save as a text file" onClick={() => void save(r)}><SaveIcon /></button>
                          <button className="icon-btn" title="Keep as a timestamped capture next to this device's config backups (Tools → Configs)" onClick={() => void capture(r)}><FolderIcon /></button>
                        </div>
                        {r.outputs.map(([cmd, o]) => (
                          <details key={cmd} className="raw" open={"Err" in o}>
                            <summary>{cmd}{"Err" in o ? " · failed" : ""}</summary>
                            <pre className={"Err" in o ? "rejected" : ""}>{"Ok" in o ? o.Ok || "(no output)" : o.Err}</pre>
                          </details>
                        ))}
                      </>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {list.length === 0 && <div className="tools-empty"><p>{profile ? "No command sets for this platform yet." : "Pick or detect the platform to see its command sets."}</p></div>}
        </div>
      )}

      {tab === "workflow" && wf && (
        <div className="check-detail workflow-info">
          <p>{wf.candidate ? "Candidate configuration: lines are staged and nothing is live until the commit." : "Lines apply as they are typed."} Deploy (Tools → Deploy) follows this on every change.</p>
          <table className="data-table">
            <tbody>
              <tr><th>Enter</th><td><code>{wf.enter.join(" ; ") || "—"}</code></td></tr>
              <tr><th>Exit</th><td><code>{wf.exit.join(" ; ") || "—"}</code></td></tr>
              {wf.candidate && <tr><th>Commit</th><td><code>{wf.commit.join(" ; ") || "—"}</code></td></tr>}
              {wf.commitConfirmed && <tr><th>Timed commit</th><td><code>{wf.commitConfirmed.replace("{m}", "N").replace("{name}", "<name>")}</code></td></tr>}
              <tr><th>Save</th><td><code>{wf.save.join(" ; ") || "not needed: a commit is persistent"}</code></td></tr>
              {wf.checkpoint && <tr><th>Checkpoint</th><td><code>{wf.checkpoint.replace("{name}", "<name>")}</code></td></tr>}
              <tr><th>Rollback</th><td><code>{wf.rollback ? wf.rollback.replace("{name}", "<name>") : "restore the saved configuration text"}</code></td></tr>
              <tr><th>Restorable backup</th><td><code>{wf.restorable}</code></td></tr>
            </tbody>
          </table>
          {wf.notes.length > 0 && <ul>{wf.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
          {profile && profile.configs.length > 0 && (
            <div className="follow">
              <span>Capture:</span>
              {profile.configs.map((k) => <button key={k.kind} className="btn small" disabled={!sessionId} title={k.command} onClick={() => onSend(k.command)}>{k.label}</button>)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
