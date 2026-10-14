import { useEffect, useRef, useState } from "react";
import { loadData, saveData } from "../lib/backend";
import {
  aiCancel, aiChat, classifyCommands, commandBlocks, QUICK_ACTIONS, systemPrompt, userTurn, type AiSettings, type AuditEntry,
  type ChatMessage, type Classified, type Context, type Kind,
} from "../lib/ai";
import { isUnlocked, lock, unlock } from "../lib/lock";
import { backupConfig, runCommand, type CheckReport, type Facts } from "../lib/tools";
import { CloseIcon, StopIcon } from "./Icons";
import Markdown from "./Markdown";

interface Props {
  sessionId: string | null;
  tabKey: string;
  facts: Facts | null;
  report: CheckReport | null;
  hostname: string;
  deviceName: string;
  site: string;
  environment: string;
  ai: AiSettings;
  getSelection: () => string;
  getRecent: () => string;
  onToast: (t: string) => void;
  onOpenSettings: () => void;
  /** a question handed over from another panel section */
  pendingAsk: { question: string; output: string } | null;
  onAskConsumed: () => void;
}

interface Turn {
  role: "user" | "assistant";
  content: string;
  /** reviewed command blocks found in an assistant reply */
  blocks?: Review[];
  streaming?: boolean;
  redacted?: number;
}

interface Review {
  title: string;
  items: Classified[];
  kind: Kind;
  state: "proposed" | "running" | "done" | "failed" | "dismissed";
  outputs: { command: string; output: string }[];
  backup: string | null;
}

interface TabAi {
  turns: Turn[];
}

const cache = new Map<string, TabAi>();

const KIND_LABEL: Record<Kind, string> = { "read-only": "read-only", config: "configuration change", disruptive: "disruptive" };

export default function Assistant(props: Props) {
  const { sessionId, tabKey, facts, report, hostname, deviceName, site, environment, ai, getSelection, getRecent, onToast, onOpenSettings, pendingAsk, onAskConsumed } = props;
  const [, setTick] = useState(0);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<{ turn: number; block: number; backup: boolean; reviewed: boolean; typed: string } | null>(null);
  const [unlockTyped, setUnlockTyped] = useState<string | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  if (!cache.has(tabKey)) cache.set(tabKey, { turns: [] });
  const st = cache.get(tabKey)!;
  const refresh = () => setTick((t) => t + 1);
  const provider = ai.providers.find((p) => p.id === ai.activeId) ?? ai.providers[0];
  const production = environment === "production";

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [st.turns.length, busy]);

  useEffect(() => {
    if (pendingAsk && !busy) {
      onAskConsumed();
      void ask(pendingAsk.question, pendingAsk.output, "Command output");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingAsk]);

  const ctx = (output: string, outputLabel: string): Context => ({ facts, hostname, environment, report, output, outputLabel, settings: ai });

  async function ask(question: string, output = "", outputLabel = "Terminal output") {
    if (!provider) {
      onToast("Add an AI provider in Settings → Assistant first");
      onOpenSettings();
      return;
    }
    if (busy) return;
    const c = ctx(output, outputLabel);
    const user: Turn = { role: "user", content: userTurn(question, c) };
    const reply: Turn = { role: "assistant", content: "", streaming: true };
    st.turns.push(user, reply);
    refresh();
    const history: ChatMessage[] = [{ role: "system", content: systemPrompt(c) }, ...st.turns.filter((t) => !t.streaming).slice(-12).map((t) => ({ role: t.role, content: t.content }))];
    const id = crypto.randomUUID();
    setBusy(id);
    try {
      await aiChat(id, provider, history, (e) => {
        if (e.type === "token") {
          reply.content += e.text;
          refresh();
        } else if (e.type === "done") {
          reply.redacted = e.redacted;
        }
      });
      reply.streaming = false;
      const blocks = commandBlocks(reply.content);
      if (blocks.length) {
        reply.blocks = await Promise.all(blocks.map(async (b) => {
          const items = await classifyCommands(b.commands);
          const kind: Kind = items.some((i) => i.kind === "disruptive") ? "disruptive" : items.some((i) => i.kind === "config") ? "config" : "read-only";
          return { title: b.title, items, kind, state: "proposed" as const, outputs: [], backup: null };
        }));
      }
    } catch (e) {
      reply.streaming = false;
      reply.content = reply.content || `Request failed: ${String(e)}`;
    } finally {
      setBusy(null);
      refresh();
    }
  }

  function quick(id: string) {
    const action = QUICK_ACTIONS.find((a) => a.id === id)!;
    if (id === "config") {
      setInput(action.question);
      return;
    }
    const sel = getSelection();
    const output = sel || (action.needsOutput || id === "next" ? getRecent() : "");
    if (action.needsOutput && !output.trim()) {
      onToast("Select some output in the terminal first");
      return;
    }
    void ask(action.question, output, sel ? "Selected output" : "Recent terminal output");
  }

  async function audit(entry: AuditEntry) {
    try {
      const list = ((await loadData<AuditEntry[]>("audit")) ?? []).slice(-5000);
      list.push(entry);
      await saveData("audit", list);
    } catch {
      // the audit trail must never block the work; the terminal log still has it
    }
  }

  async function run(turnIdx: number, blockIdx: number, withBackup: boolean) {
    const review = st.turns[turnIdx].blocks![blockIdx];
    if (!sessionId) return;
    review.state = "running";
    review.outputs = [];
    refresh();
    setConfirming(null);
    const commands = review.items.map((i) => i.command);
    let result: AuditEntry["result"] = "sent";
    let note = "";
    try {
      if (withBackup && review.kind !== "read-only") {
        const saved = await backupConfig(sessionId, facts?.os ?? null, site, facts?.hostname || deviceName, ["running"]);
        review.backup = saved[0]?.path ?? null;
        refresh();
      }
      for (const command of commands) {
        const output = await runCommand(sessionId, command, 120);
        review.outputs.push({ command, output });
        refresh();
      }
      review.state = "done";
    } catch (e) {
      review.state = "failed";
      result = "failed";
      note = String(e);
      review.outputs.push({ command: "", output: `Stopped: ${note}` });
    }
    refresh();
    if (review.kind !== "read-only") {
      void audit({ time: Math.floor(Date.now() / 1000), device: facts?.hostname || deviceName, environment, kind: review.kind, commands, backup: review.backup, result, note });
      onToast(result === "sent" ? `Applied ${commands.length} line${commands.length === 1 ? "" : "s"}${review.backup ? " (backup saved first)" : ""}` : `Stopped: ${note}`);
    }
  }

  const canRun = !!sessionId;
  const quickDisabled = !provider || !!busy;

  return (
    <div className="tools-body assistant">
      <div className="tools-actions">
        {QUICK_ACTIONS.map((a) => (
          <button key={a.id} className="btn small" disabled={quickDisabled} onClick={() => quick(a.id)} title={a.question}>{a.title}</button>
        ))}
        <span className="grow" />
        {production && (
          isUnlocked(tabKey) ? (
            <button className="btn small warn-text" title="Lock configuration changes on this production device again" onClick={() => { lock(tabKey); refresh(); }}>Production unlocked · lock</button>
          ) : (
            <button className="btn small" title="Configuration changes on production devices are locked until you unlock this tab" onClick={() => setUnlockTyped("")}>Production locked</button>
          )
        )}
      </div>
      {provider ? (
        <div className="quiet small">
          {provider.name} · {provider.model || "no model chosen"}{provider.kind === "ollama" || provider.kind === "lmstudio" ? " · local" : ""} · secrets redacted
          {!ai.shareOutput && " · output sharing off"}
        </div>
      ) : (
        <div className="tools-note inline">No AI provider yet. <button className="link" onClick={onOpenSettings}>Add one in Settings</button> (Ollama or LM Studio on this computer, or a cloud provider with a key in the keychain).</div>
      )}

      {unlockTyped !== null && (
        <div className="review confirm">
          <div className="strong">Unlock configuration changes on this production device for this tab</div>
          <p>Type the device name <code>{facts?.hostname || deviceName}</code> to confirm. Read-only commands never need this.</p>
          <input autoFocus value={unlockTyped} onChange={(e) => setUnlockTyped(e.target.value)} spellCheck={false} />
          <div className="row">
            <button className="btn" onClick={() => setUnlockTyped(null)}>Cancel</button>
            <button className="btn primary" disabled={unlockTyped.trim() !== (facts?.hostname || deviceName)} onClick={() => { unlock(tabKey); setUnlockTyped(null); refresh(); }}>Unlock for this tab</button>
          </div>
        </div>
      )}

      <div className="chat">
        {st.turns.length === 0 && (
          <div className="tools-empty">
            <p>Ask about the device, select output and choose <em>Explain this output</em>, or describe a configuration to generate. Commands the assistant proposes appear as cards for review: read-only ones run on approval, configuration changes need a second confirmation and get a backup first.</p>
          </div>
        )}
        {st.turns.map((t, ti) => (
          <div key={ti} className={`turn ${t.role}`}>
            {t.role === "user" ? <pre className="user-text">{t.content.split("\n\n")[0]}{t.content.includes("```") ? "\n(with terminal output)" : ""}</pre> : (
              <>
                <Markdown text={t.content || (t.streaming ? "…" : "")} />
                {t.redacted ? <div className="quiet small">{t.redacted} secret{t.redacted === 1 ? "" : "s"} redacted before sending</div> : null}
                {t.blocks?.map((b, bi) => b.state !== "dismissed" && (
                  <div key={bi} className={`review ${b.kind} ${b.state}`}>
                    <div className="review-head">
                      <span className={`kind-chip ${b.kind}`}>{KIND_LABEL[b.kind]}</span>
                      <span className="strong">{b.title || `${b.items.length} command${b.items.length === 1 ? "" : "s"}`}</span>
                      <span className="grow" />
                      {b.state === "proposed" && <button className="icon-btn" title="Dismiss" onClick={() => { b.state = "dismissed"; refresh(); }}><CloseIcon /></button>}
                    </div>
                    <ol className="review-lines">
                      {b.items.map((it, k) => (
                        <li key={k} className={it.kind}><code>{it.command}</code><span className="quiet"> {it.reason}</span></li>
                      ))}
                    </ol>
                    {b.state === "proposed" && (
                      <div className="row">
                        <button className="btn small" onClick={() => void navigator.clipboard.writeText(b.items.map((i) => i.command).join("\n"))}>Copy</button>
                        {b.kind === "read-only" ? (
                          <button className="btn small primary" disabled={!canRun} onClick={() => void run(ti, bi, false)}>Run on the device</button>
                        ) : (
                          <button className="btn small primary" disabled={!canRun} onClick={() => setConfirming({ turn: ti, block: bi, backup: true, reviewed: false, typed: "" })}>Review and apply…</button>
                        )}
                        {!canRun && <span className="quiet small">connect to run</span>}
                      </div>
                    )}
                    {confirming && confirming.turn === ti && confirming.block === bi && (
                      <div className="confirm">
                        {production && !isUnlocked(tabKey) ? (
                          <p className="warn-text">This is a production device. Unlock configuration changes for this tab first (button at the top), then come back here.</p>
                        ) : (
                          <>
                            <p>{b.kind === "disruptive" ? "This includes disruptive commands (see the reasons above). " : ""}The lines run one by one in the terminal; the output stays visible. An audit entry is written.</p>
                            <label className="check"><input type="checkbox" checked={confirming.backup} onChange={(e) => setConfirming({ ...confirming, backup: e.target.checked })} />Back up the running config first</label>
                            <label className="check"><input type="checkbox" checked={confirming.reviewed} onChange={(e) => setConfirming({ ...confirming, reviewed: e.target.checked })} />I reviewed every line above</label>
                            {b.kind === "disruptive" && (
                              <label className="typed">Type the device name <code>{facts?.hostname || deviceName}</code> to confirm<input value={confirming.typed} onChange={(e) => setConfirming({ ...confirming, typed: e.target.value })} spellCheck={false} /></label>
                            )}
                          </>
                        )}
                        <div className="row">
                          <button className="btn small" onClick={() => setConfirming(null)}>Cancel</button>
                          <button
                            className="btn small danger-fill"
                            disabled={!canRun || (production && !isUnlocked(tabKey)) || !confirming.reviewed || (b.kind === "disruptive" && confirming.typed.trim() !== (facts?.hostname || deviceName))}
                            onClick={() => void run(ti, bi, confirming.backup)}
                          >
                            Apply {b.items.length} line{b.items.length === 1 ? "" : "s"}
                          </button>
                        </div>
                      </div>
                    )}
                    {b.state === "running" && <div className="quiet small">Running… {b.outputs.length}/{b.items.length}</div>}
                    {b.outputs.length > 0 && (
                      <details className="raw" open={b.state === "failed"}>
                        <summary>{b.state === "failed" ? "Stopped with an error" : "Output"}{b.backup ? " · backup saved" : ""}</summary>
                        {b.outputs.map((o, k) => <pre key={k}>{o.command ? `${o.command}\n` : ""}{o.output || "(no output)"}</pre>)}
                      </details>
                    )}
                    {b.state === "done" && b.outputs.length > 0 && (
                      <div className="row">
                        <button className="btn small" onClick={() => void ask("Here is the output of the commands you proposed. Interpret it.", b.outputs.map((o) => `${o.command}\n${o.output}`).join("\n\n"), "Command output")} disabled={!!busy}>Ask about this output</button>
                      </div>
                    )}
                  </div>
                ))}
              </>
            )}
          </div>
        ))}
        <div ref={bottom} />
      </div>

      <form
        className="chat-input"
        onSubmit={(e) => {
          e.preventDefault();
          const q = input.trim();
          if (!q) return;
          setInput("");
          const sel = getSelection();
          void ask(q, sel, "Selected output");
        }}
      >
        <textarea
          value={input}
          placeholder={provider ? "Ask about the device, or describe a configuration to generate. Selected terminal output is attached." : "Add a provider in Settings → Assistant"}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              e.currentTarget.form?.requestSubmit();
            }
          }}
          rows={2}
          disabled={!provider}
        />
        {busy ? (
          <button type="button" className="btn" onClick={() => void aiCancel(busy)}><StopIcon /> Stop</button>
        ) : (
          <button type="submit" className="btn primary" disabled={!provider || !input.trim()}>Ask</button>
        )}
        {st.turns.length > 0 && !busy && <button type="button" className="btn" title="Clear the conversation" onClick={() => { st.turns = []; refresh(); }}>Clear</button>}
      </form>
    </div>
  );
}

