import { useTabState } from "../lib/tabstate";
import { runCommand, type Facts } from "../lib/tools";
import { commandFor, INPUT_LABEL, WORKFLOWS, type Step, type Workflow } from "../lib/workflows";

interface Props {
  sessionId: string | null;
  tabKey: string;
  facts: Facts | null;
  onToast: (t: string) => void;
  onAsk: (question: string, output: string) => void;
}

/** Guided troubleshooting: run each step on the device and read the hint. */
interface WfState {
  wf: Workflow | null;
  values: Record<string, string>;
  outputs: Record<number, { command: string; text: string; error?: boolean }>;
  running: number | null;
}

export default function Workflows({ sessionId, tabKey, facts, onToast, onAsk }: Props) {
  const [st, update] = useTabState<WfState>("workflows", tabKey, () => ({ wf: null, values: {}, outputs: {}, running: null }));
  const { wf, values, outputs, running } = st;
  const setWf = (wf: Workflow | null) => update({ wf });
  const setRunning = (running: number | null) => update({ running });
  const setValues = (f: (v: Record<string, string>) => Record<string, string>) => update({ values: f(st.values) });
  const setOutputs = (f: (o: WfState["outputs"]) => WfState["outputs"]) => update({ outputs: f(st.outputs) });

  async function run(i: number, step: Step) {
    if (!sessionId || running !== null) return;
    const cmd = commandFor(step, facts?.os ?? null, values);
    if (!cmd) {
      onToast(cmd === null && /\{(\w+)\}/.test(step.command) ? "Fill in the fields above first" : "Not available on this platform");
      return;
    }
    setRunning(i);
    try {
      const text = await runCommand(sessionId, cmd, 120);
      setOutputs((o) => ({ ...o, [i]: { command: cmd, text } }));
      // the MAC trace fills later steps from what it finds
      if (wf?.id === "mac-trace") {
        const mac = text.match(/\b([0-9a-f]{4}\.[0-9a-f]{4}\.[0-9a-f]{4})\b/i)?.[1];
        const port = text.match(/\b((?:Gi|Te|Fa|Fo|Hu|Tw|Eth|Po|Ethernet|GigabitEthernet|TenGigabitEthernet|FastEthernet|Port-channel)\S*\d)\s*$/im)?.[1];
        setValues((v) => ({ ...v, ...(mac && !v.mac ? { mac } : {}), ...(port && i === 1 ? { if: port } : {}) }));
      }
    } catch (e) {
      setOutputs((o) => ({ ...o, [i]: { command: cmd, text: String(e), error: true } }));
    } finally {
      setRunning(null);
    }
  }

  async function runAll() {
    if (!wf) return;
    for (let i = 0; i < wf.steps.length; i++) {
      if (!sessionId) break;
      // steps needing a value that is still blank are skipped
      if (!commandFor(wf.steps[i], facts?.os ?? null, values)) continue;
      // eslint-disable-next-line no-await-in-loop
      await run(i, wf.steps[i]);
    }
  }

  if (!wf) {
    return (
      <div className="tools-body">
        <div className="quiet small">Step-by-step read-only checks for common problems. Each step says what to look for; the assistant can interpret any step's output.</div>
        <div className="wf-list">
          {WORKFLOWS.map((w) => (
            <button key={w.id} className="wf-card" onClick={() => update({ wf: w, values: {}, outputs: {} })}>
              <span className="strong">{w.title}</span>
              <span className="quiet">{w.description}</span>
              <span className="quiet small">{w.steps.length} steps</span>
            </button>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="tools-body">
      <div className="tools-actions">
        <button className="btn small" onClick={() => setWf(null)}>← Workflows</button>
        <span className="strong">{wf.title}</span>
        <span className="grow" />
        <button className="btn small primary" disabled={!sessionId || running !== null} onClick={() => void runAll()}>Run all steps</button>
      </div>
      {wf.inputs.length > 0 && (
        <div className="wf-inputs">
          {wf.inputs.map((k) => (
            <label key={k}>{INPUT_LABEL[k]}<input value={values[k] ?? ""} onChange={(e) => setValues((v) => ({ ...v, [k]: e.target.value }))} placeholder={k === "if" ? "Gi1/0/1" : k === "mac" ? "any format" : ""} spellCheck={false} /></label>
          ))}
        </div>
      )}
      <ol className="wf-steps">
        {wf.steps.map((s, i) => {
          const cmd = commandFor(s, facts?.os ?? null, values);
          const out = outputs[i];
          return (
            <li key={i} className={out ? (out.error ? "failed" : "done") : ""}>
              <div className="wf-step-head">
                <span className="strong">{s.title}</span>
                <code className="quiet">{cmd ?? s.command}</code>
                <span className="grow" />
                <button className="btn small" disabled={!sessionId || running !== null || !cmd} onClick={() => void run(i, s)}>{running === i ? "Running…" : out ? "Again" : "Run"}</button>
              </div>
              <div className="wf-look">Look for: {s.lookFor}</div>
              {out && (
                <>
                  <pre className={out.error ? "warn-text" : ""}>{out.text || "(no output)"}</pre>
                  {!out.error && <button className="btn small" onClick={() => onAsk(`In the "${wf.title}" workflow, step "${s.title}" (${s.lookFor}). Interpret this output.`, `${out.command}\n${out.text}`)}>Ask the assistant</button>}
                </>
              )}
            </li>
          );
        })}
      </ol>
      {!sessionId && <div className="quiet small">Connect to a device to run the steps.</div>}
    </div>
  );
}
