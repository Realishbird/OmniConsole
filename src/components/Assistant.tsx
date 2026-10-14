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
  outputs: { command: string; output: string; error?: boolean }[];
  backup: string | null;
  /** how many automatic follow-ups led here (bounded, so a wrong command cannot loop forever) */
  depth: number;
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

  async function ask(question: string, output = "", outputLabel = "Terminal output", depth = 0) {
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
        const reviews = await Promise.all(blocks.map(async (b) => {
          const items = await classifyCommands(b.commands);
          const kind: Kind = items.some((i) => i.kind === "disruptive") ? "disruptive" : items.some((i) => i.kind === "config") ? "config" : "read-only";
          return { title: b.title, items, kind, state: "proposed" as const, outputs: [], backup: null, depth };
        }));
        // one card for all the read-only commands of a reply; each change keeps its own
        const readOnly = reviews.filter((r) => r.kind === "read-only");
        const merged: Review[] = readOnly.length > 1
          ? [{ ...readOnly[0], title: readOnly.length === reviews.length ? "" : "Read-only", items: readOnly.flatMap((r) => r.items).filter((it, i, all) => all.findIndex((x) => x.command === it.command) === i) }]
          : readOnly;
        reply.blocks = [...merged, ...reviews.filter((r) => r.kind !== "read-only")];
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
    const rejected: string[] = [];
    try {
      if (withBackup && review.kind !== "read-only") {
        const saved = await backupConfig(sessionId, facts?.os ?? null, site, facts?.hostname || deviceName, ["running"]);
        review.backup = saved[0]?.path ?? null;
        refresh();
      }
      for (const command of commands) {
        try {
          const output = await runCommand(sessionId, command, 120);
          review.outputs.push({ command, output });
        } catch (e) {
          // a rejected show command is not a reason to skip the rest; a
          // rejected configuration line is, since later lines may depend on it
          const msg = String(e);
          review.outputs.push({ command, output: msg, error: true });
          rejected.push(`${command}\n${msg}`);
          if (review.kind !== "read-only" || !/rejected/.test(msg)) throw e;
        } finally {
          refresh();
        }
      }
      review.state = rejected.length ? "failed" : "done";
    } catch (e) {
      review.state = "failed";
      result = "failed";
      note = String(e);
    }
    refresh();
    if (review.kind !== "read-only") {
      void audit({ time: Math.floor(Date.now() / 1000), device: facts?.hostname || deviceName, environment, kind: review.kind, commands, backup: review.backup, result, note });
      onToast(result === "sent" ? `Applied ${commands.length} line${commands.length === 1 ? "" : "s"}${review.backup ? " (backup saved first)" : ""}` : `Stopped: ${note}`);
    }
    // the assistant looks at what came back on its own: it corrects commands
    // the device rejected and interprets the rest, a few rounds at most
    if (review.depth < 3 && review.outputs.length > 0) void followUp(review);
  }

  function outputsText(review: Review) {
    return review.outputs.map((o) => `${o.command}\n${o.output || "(no output)"}`).join("\n\n");
  }

  async function followUp(review: Review) {
    const bad = review.outputs.filter((o) => o.error);
    const question = bad.length
      ? `The device rejected ${bad.length === 1 ? "this command" : "these commands"}: ${bad.map((o) => `\`${o.command}\``).join(", ")}. Give the corrected command for this exact OS and version in a new \`cli\` block (or say it does not exist here and what to use instead), then interpret the output that did come back.`
      : review.kind === "read-only"
        ? "Here is the output of the commands you proposed. Interpret it: what it confirms, what it rules out, and the next command if one is needed."
        : "The configuration was applied; here is what the device printed. Confirm it took effect from the output and propose the show command that verifies it.";
    await ask(question, outputsText(review), "Command output", review.depth + 1);
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
            <p>Ask about the device, or select output and choose <em>Explain this output</em>. Proposed commands appear as cards: read-only ones run when you approve, changes need a second confirmation and a backup.</p>
          </div>
        )}
        {st.turns.map((t, ti) => (
          <div key={ti} className={`turn ${t.role}`}>
            {t.role === "user" ? <pre className="user-text">{t.content.split("\n\n")[0]}{t.content.includes("```") ? <span className="quiet"> · with output</span> : ""}</pre> : (
              <>
                <Markdown text={t.content || (t.streaming ? "…" : "")} hideCommands={!!t.blocks?.length} />
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
                        <li key={k} className={it.kind}><code>{it.command}</code>{it.kind !== "read-only" && <span className="reason">{it.reason}</span>}</li>
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
                        {b.outputs.map((o, k) => <pre key={k} className={o.error ? "rejected" : ""}>{o.command ? `${o.command}\n` : ""}{o.output || "(no output)"}</pre>)}
                      </details>
                    )}
                    {(b.state === "done" || b.state === "failed") && b.outputs.length > 0 && b.depth >= 3 && (
                      <div className="row">
                        <button className="btn small" onClick={() => void followUp(b)} disabled={!!busy}>Ask about this output</button>
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

