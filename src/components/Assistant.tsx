import { useEffect, useRef, useState } from "react";
import { loadData, saveData } from "../lib/backend";
import {
  aiCancel, aiChat, autoRunFor, builtinProvider, classifyCommands, commandBlocks, evidenceCheck, isLocal, normalizeCommand, QUICK_ACTIONS, systemPrompt, userTurn,
  type AiSettings, type AuditEntry, type ChatMessage, type Classified, type Context, type Kind, type Provider, type ToolCall,
} from "../lib/ai";
import { executeTool, MAX_ROUNDS, TOOLS, type ToolContext } from "../lib/agent";
import { downloads, formatBytes, localStatus, modelDownload, onDownload, type LocalStatus } from "../lib/local";
import { isUnlocked, lock, unlock } from "../lib/lock";
import { backupConfig, runCommand, type CheckReport, type Facts } from "../lib/tools";
import { CloseIcon, StopIcon } from "./Icons";
import Markdown from "./Markdown";

interface Props {
  sessionId: string | null;
  tabKey: string;
  deviceId: string;
  facts: Facts | null;
  report: CheckReport | null;
  onReport: (r: CheckReport) => void;
  hostname: string;
  deviceName: string;
  site: string;
  environment: string;
  ai: AiSettings;
  onAiChange: (ai: AiSettings) => void;
  getSelection: () => string;
  getRecent: () => string;
  onToast: (t: string) => void;
  onOpenSettings: () => void;
  /** a question handed over from another panel section */
  pendingAsk: { question: string; output: string } | null;
  onAskConsumed: () => void;
}

/** One thing the assistant did on its own while answering. */
interface Step {
  label: string;
  status: "running" | "done" | "blocked" | "failed";
  /** the command running right now */
  detail: string;
  outputs: { command: string; output: string; error?: boolean }[];
  /** text the model wrote before making the call */
  note: string;
  ms: number;
}

interface Turn {
  role: "user" | "assistant";
  content: string;
  /** what the chat shows for an internal follow-up instead of the prompt text */
  display?: string;
  /** reviewed command blocks found in an assistant reply */
  blocks?: Review[];
  steps?: Step[];
  streaming?: boolean;
  redacted?: number;
  /** Verified bullets that quote nothing, or quote text absent from the output the model saw */
  evidence?: { unquoted: string[]; notFound: string[] };
  /** the full exchange, tool traffic included, for the next question */
  history?: ChatMessage[];
}

interface Review {
  title: string;
  items: Classified[];
  kind: Kind;
  state: "proposed" | "running" | "done" | "failed" | "dismissed" | "superseded";
  outputs: { command: string; output: string; error?: boolean }[];
  backup: string | null;
  /** how many automatic follow-ups led here (bounded, so a wrong command cannot loop forever) */
  depth: number;
}

interface TabAi {
  turns: Turn[];
  /** every command sent to the device from this conversation, with its output */
  seen: Map<string, string>;
  /** the provider rejected tool definitions once; plain answers from then on */
  noTools: boolean;
}

const cache = new Map<string, TabAi>();

/** `<server>`-style placeholders the model left for the engineer to fill in */
const hasPlaceholder = (c: string) => /<[a-z][\w .\/-]*>/i.test(c);

const KIND_LABEL: Record<Kind, string> = { "read-only": "read-only", config: "configuration change", disruptive: "disruptive" };

export default function Assistant(props: Props) {
  const { sessionId, tabKey, deviceId, facts, report, onReport, hostname, deviceName, site, environment, ai, onAiChange, getSelection, getRecent, onToast, onOpenSettings, pendingAsk, onAskConsumed } = props;
  const [, setTick] = useState(0);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const stop = useRef(false);
  const [confirming, setConfirming] = useState<{ turn: number; block: number; backup: boolean; reviewed: boolean; typed: string } | null>(null);
  const [unlockTyped, setUnlockTyped] = useState<string | null>(null);
  const [local, setLocal] = useState<LocalStatus | null>(null);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [downloadError, setDownloadError] = useState<string>("");
  const bottom = useRef<HTMLDivElement>(null);
  // the download listener outlives renders; it must see the latest settings
  const aiRef = useRef(ai);
  aiRef.current = ai;
  if (!cache.has(tabKey)) cache.set(tabKey, { turns: [], seen: new Map(), noTools: false });
  const st = cache.get(tabKey)!;
  const refresh = () => setTick((t) => t + 1);
  const provider = ai.providers.find((p) => p.id === ai.activeId) ?? ai.providers[0];
  const production = environment === "production";
  const autoRun = autoRunFor(ai, deviceId);

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

  // the first-run offer and the model picker need to know what is on disk
  useEffect(() => {
    void localStatus().then(setLocal).catch(() => setLocal(null));
    let off: (() => void) | undefined;
    void onDownload((e) => {
      if (e.type === "progress") downloads.set(e.id, { received: e.received, total: e.total, bytesPerSec: e.bytesPerSec });
      else downloads.delete(e.id);
      if (e.type === "done") {
        void localStatus().then(setLocal).catch(() => {});
        useBuiltin(e.id);
        setDownloading(null);
      }
      if (e.type === "error") {
        setDownloading(null);
        setDownloadError(e.message);
      }
      if (e.type === "cancelled") setDownloading(null);
      refresh();
    }).then((fn) => { off = fn; });
    return () => off?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Make the built-in runtime the active provider with this model. */
  function useBuiltin(modelId: string) {
    const cur = aiRef.current;
    const existing = builtinProvider(cur);
    const p: Provider = existing ? { ...existing, model: modelId } : { id: crypto.randomUUID(), name: "Built-in", kind: "builtin", baseUrl: "", model: modelId, hasKey: false };
    const providers = existing ? cur.providers.map((x) => (x.id === p.id ? p : x)) : [...cur.providers, p];
    onAiChange({ ...cur, providers, activeId: p.id });
  }

  async function downloadRecommended(id: string) {
    setDownloadError("");
    setDownloading(id);
    try {
      await modelDownload(id);
    } catch (e) {
      if (!/cancelled/.test(String(e))) setDownloadError(String(e));
      setDownloading(null);
    }
  }

  const ctx = (output: string, outputLabel: string): Context => ({ facts, hostname, environment, report, output, outputLabel, settings: ai, agent: !st.noTools, connected: !!sessionId });

  /** every command already sent to the device from this conversation (ran or rejected) */
  function alreadyRun(): Set<string> {
    const seen = new Set<string>(st.seen.keys());
    for (const t of st.turns) for (const b of t.blocks ?? []) for (const o of b.outputs) seen.add(o.command.trim().toLowerCase());
    return seen;
  }

  /** The conversation so far as messages, tool traffic included for recent turns. */
  function priorHistory(): ChatMessage[] {
    const done = st.turns.filter((t) => !t.streaming);
    const recent = done.slice(-12);
    const out: ChatMessage[] = [];
    for (const t of recent) {
      if (t.role === "assistant" && t.history) out.push(...t.history);
      else out.push({ role: t.role, content: t.content });
    }
    // tool traffic from old turns is bulky; keep it only for the last two exchanges
    const cutoff = Math.max(0, out.length - 24);
    return out.map((m, i) => {
      if (m.role !== "tool") return m;
      if (i < cutoff) return { role: "tool" as const, content: "(earlier output, omitted)", toolCallId: m.toolCallId };
      // a refusal from when auto-run was off would teach the model to keep proposing
      if (autoRun && /^Auto-run is off/.test(m.content)) return { role: "tool" as const, content: "(auto-run was off then; it is on now, so call run_show for read-only commands)", toolCallId: m.toolCallId };
      return m;
    });
  }

  async function ask(question: string, output = "", outputLabel = "Terminal output", depth = 0, display?: string) {
    if (!provider) {
      onToast("Add an AI provider in Settings → Assistant first");
      onOpenSettings();
      return;
    }
    if (busy) return;
    stop.current = false;
    const c = ctx(output, outputLabel);
    const user: Turn = { role: "user", content: userTurn(question, c), display };
    const reply: Turn = { role: "assistant", content: "", streaming: true, steps: [] };
    st.turns.push(user, reply);
    refresh();
    const history: ChatMessage[] = [{ role: "system", content: systemPrompt(c) }, ...priorHistory(), { role: "user", content: user.content }];
    const exchange: ChatMessage[] = [{ role: "user", content: user.content }];
    const toolCtx: ToolContext = {
      sessionId, facts, deviceName: facts?.hostname || deviceName, site, ai, autoRun, seen: st.seen,
      onReport,
      onProgress: (text) => {
        const s = reply.steps![reply.steps!.length - 1];
        if (s) { s.detail = text; refresh(); }
      },
    };
    const id = crypto.randomUUID();
    setBusy(id);
    const shown: string[] = [output];
    let autoRunBlock: { turn: number; block: number } | null = null;
    try {
      let tools = st.noTools ? [] : TOOLS;
      for (let round = 0; round <= MAX_ROUNDS; round++) {
        if (stop.current) break;
        // out of rounds: one last call without tools, so the engineer gets an answer
        const last = round === MAX_ROUNDS;
        const msgs = last && tools.length ? [...history, { role: "system" as const, content: "You have used every tool call available for this question. Answer now from what you gathered; propose remaining commands in a cli block." }] : history;
        reply.content = "";
        const res = await aiChat(id, provider, msgs, last ? [] : tools, (e) => {
          if (e.type === "token") {
            reply.content += e.text;
            refresh();
          } else if (e.type === "toolCall") {
            reply.steps!.push({ label: pendingLabel(e.call), status: "running", detail: "", outputs: [], note: "", ms: 0 });
            refresh();
          }
        });
        if (res.toolsDropped) {
          st.noTools = true;
          tools = [];
        }
        if (!res.toolCalls.length || stop.current) {
          reply.redacted = res.redacted;
          exchange.push({ role: "assistant", content: res.text });
          break;
        }
        const assistantMsg: ChatMessage = { role: "assistant", content: res.text, toolCalls: res.toolCalls };
        history.push(assistantMsg);
        exchange.push(assistantMsg);
        // steps announced during streaming match the calls in order; add any that were not
        while (reply.steps!.filter((s) => s.status === "running").length < res.toolCalls.length) reply.steps!.push({ label: "", status: "running", detail: "", outputs: [], note: "", ms: 0 });
        const running = reply.steps!.filter((s) => s.status === "running");
        for (let i = 0; i < res.toolCalls.length; i++) {
          const call = res.toolCalls[i];
          const step = running[i];
          step.note = i === 0 ? res.text.trim() : "";
          step.label = step.label || pendingLabel(call);
          refresh();
          const started = Date.now();
          let text: string;
          try {
            const out = await executeTool(call, toolCtx);
            text = out.text;
            step.label = out.label;
            step.outputs = out.outputs;
            step.status = out.blocked ? "blocked" : "done";
            shown.push(...out.outputs.map((o) => o.output));
          } catch (e) {
            text = `The tool failed: ${String(e)}`;
            step.status = "failed";
            step.outputs = [{ command: call.name, output: String(e), error: true }];
          }
          step.ms = Date.now() - started;
          step.detail = "";
          refresh();
          const toolMsg: ChatMessage = { role: "tool", content: text, toolCallId: call.id };
          history.push(toolMsg);
          exchange.push(toolMsg);
        }
      }
      reply.streaming = false;
      reply.history = exchange;
      // tie conclusions to evidence: what the model quoted must exist in what it was shown
      shown.push(...st.turns.flatMap((t) => (t.blocks ?? []).flatMap((b) => b.outputs.map((o) => o.output))), ...(report ? report.results.flatMap((r) => r.evidence) : []), ...st.seen.values());
      const ev = evidenceCheck(reply.content, shown);
      if (ev.unquoted.length || ev.notFound.length) reply.evidence = ev;
      const seen = alreadyRun();
      const blocks = commandBlocks(reply.content)
        .map((b) => ({ ...b, commands: b.commands.map((cmd) => normalizeCommand(cmd, facts?.os ?? null)).filter((cmd, i, all) => all.indexOf(cmd) === i && !seen.has(cmd.toLowerCase())) }))
        .filter((b) => b.commands.length);
      if (blocks.length) {
        // a newer proposal replaces the read-only cards still waiting from earlier turns
        for (const t of st.turns) for (const b of t.blocks ?? []) if (b.state === "proposed" && b.kind === "read-only") b.state = "superseded";
        // show commands the model put inside a configuration block are read-only all the same
        const isShow = (c: string) => /^(show|dir|ping|traceroute|display|get|nv show|net show|cat |ip |sudo (vtysh|ethtool|smonctl|lldpctl|l1-show))/i.test(c);
        const split = blocks.flatMap((b) => {
          const shows = b.commands.filter(isShow);
          const rest = b.commands.filter((c) => !isShow(c));
          return shows.length && rest.length ? [{ title: "", commands: shows }, { ...b, commands: rest }] : [b];
        });
        const reviews = await Promise.all(split.map(async (b) => {
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
        // with auto-run on, a read-only proposal runs at once (some models propose
        // instead of calling run_show) and the output comes back for interpretation
        const ro = reply.blocks.findIndex((b) => b.kind === "read-only" && b.state === "proposed");
        if (autoRun && ro >= 0 && sessionId && depth < 3 && !stop.current && !reply.blocks[ro].items.some((i) => hasPlaceholder(i.command))) {
          reply.blocks[ro].title = reply.blocks[ro].title || "Ran on its own";
          autoRunBlock = { turn: st.turns.indexOf(reply), block: ro };
        }
      }
    } catch (e) {
      reply.streaming = false;
      reply.history = exchange;
      for (const s of reply.steps ?? []) if (s.status === "running") s.status = "failed";
      reply.content = reply.content || `Request failed: ${String(e)}`;
    } finally {
      setBusy(null);
      refresh();
    }
    if (autoRunBlock) await run(autoRunBlock.turn, autoRunBlock.block, false);
  }

  function pendingLabel(call: ToolCall): string {
    let a: Record<string, unknown> = {};
    try { a = JSON.parse(call.arguments || "{}"); } catch { /* shown generically */ }
    switch (call.name) {
      case "run_show": {
        const cmds = Array.isArray(a.commands) ? (a.commands as string[]) : [];
        return cmds.length === 1 ? `Running \`${cmds[0]}\`` : `Running ${cmds.length || "some"} commands`;
      }
      case "health_check": return `Running the ${a.tier === "detailed" ? "detailed" : "quick"} health check`;
      case "read_config": return `Reading the ${String(a.source ?? "running")} config`;
      case "search_saved_configs": return `Searching saved configs for "${String(a.query ?? "")}"`;
      case "list_saved": return "Listing saved backups and captures";
      case "read_saved": return "Reading a saved file";
      case "inventory": return "Reading the inventory";
      case "workstation": return `Running ${String(a.action ?? "a test")} from this computer`;
      default: return call.name;
    }
  }

  function stopNow() {
    stop.current = true;
    if (busy) void aiCancel(busy);
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
          st.seen.set(command.toLowerCase(), output);
        } catch (e) {
          // a rejected show command is not a reason to skip the rest; a
          // rejected configuration line is, since later lines may depend on it
          const msg = String(e);
          review.outputs.push({ command, output: msg, error: true });
          st.seen.set(command.toLowerCase(), msg);
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

  function outputSummary(b: Review): string {
    const rejected = b.outputs.filter((o) => o.error).length;
    if (b.state === "failed" && b.kind !== "read-only") return "Stopped: a line was rejected";
    if (rejected) return `Finished, ${rejected} of ${b.outputs.length} command${b.outputs.length === 1 ? "" : "s"} rejected`;
    return b.state === "running" ? "Output so far" : "Output";
  }

  function outputsText(review: Review) {
    return review.outputs.map((o) => `${o.command}\n${o.output || "(no output)"}`).join("\n\n");
  }

  async function followUp(review: Review) {
    const bad = review.outputs.filter((o) => o.error);
    const good = review.outputs.filter((o) => !o.error);
    const ran = Array.from(alreadyRun()).map((c) => `\`${c}\``).join(", ");
    const question = bad.length
      ? `${bad.length === 1 ? "One command was" : `${bad.length} commands were`} rejected by the device: ${bad.map((o) => `\`${o.command}\``).join(", ")}. ${good.length ? `The other ${good.length} ran; interpret ALL of their output first under **Verified**, command by command. ` : ""}Then, under **Corrected command**, give one replacement that exists on this exact OS and version in a \`cli\` block, or say the information is not available here. Do not propose any command that already ran: ${ran}.`
      : review.kind === "read-only"
        ? `Here is the output of the commands you proposed. Interpret all of it: what each command confirms, what it rules out, and the next command only if one is needed. Already run, do not repeat: ${ran}.`
        : "The configuration was applied; here is what the device printed. Confirm it took effect from the output and propose the show command that verifies it.";
    const display = bad.length
      ? `Following up: ${bad.length} rejected, ${good.length} with output`
      : `Following up on the output of ${review.outputs.length} command${review.outputs.length === 1 ? "" : "s"}`;
    await ask(question, outputsText(review), "Command output", review.depth + 1, display);
  }

  function toggleAutoRun() {
    if (!deviceId) {
      onAiChange({ ...ai, autoRun: !autoRun });
      return;
    }
    const off = new Set(ai.autoRunOff ?? []);
    if (autoRun) off.add(deviceId);
    else {
      off.delete(deviceId);
      if (!(ai.autoRun ?? true)) {
        onAiChange({ ...ai, autoRun: true, autoRunOff: [...off] });
        return;
      }
    }
    onAiChange({ ...ai, autoRunOff: [...off] });
  }

  /** Picker entries: every provider, and one line per downloaded built-in model. */
  const choices: { value: string; label: string }[] = [];
  for (const p of ai.providers) {
    if (p.kind === "builtin") {
      for (const m of local?.models.filter((m) => m.downloaded) ?? []) choices.push({ value: `builtin:${m.id}`, label: `Built-in · ${m.name}` });
    } else {
      choices.push({ value: p.id, label: `${p.name} · ${p.model || "no model"}` });
    }
  }
  if (!builtinProvider(ai)) for (const m of local?.models.filter((m) => m.downloaded) ?? []) choices.push({ value: `builtin:${m.id}`, label: `Built-in · ${m.name}` });
  const current = provider?.kind === "builtin" ? `builtin:${provider.model}` : provider?.id ?? "";
  function pick(value: string) {
    if (value === "__settings") {
      onOpenSettings();
      return;
    }
    if (value.startsWith("builtin:")) useBuiltin(value.slice("builtin:".length));
    else onAiChange({ ...ai, activeId: value });
  }

  const recommended = local?.models.find((m) => m.id === local.recommended);
  const progress = downloading ? downloads.get(downloading) : undefined;
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
      <div className="assistant-bar">
        <select className="model-pick" value={choices.some((c) => c.value === current) ? current : ""} onChange={(e) => pick(e.target.value)} title="Which model answers">
          {!choices.length && <option value="">no model yet</option>}
          {choices.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
          <option value="__settings">Models and providers…</option>
        </select>
        {provider && <span className="quiet small">{isLocal(provider) ? "local · nothing leaves this computer" : "cloud · shared output is sent"} · secrets redacted{!ai.shareOutput && " · output sharing off"}</span>}
        <span className="grow" />
        <label
          className="check small auto-run"
          title={autoRun ? "On: the assistant runs read-only show commands on its own while answering (this device). Untick to have it propose them for you to run." : "Off: read-only commands are proposed as cards for you to run on this device. Tick to let the assistant run them itself."}
        >
          <input type="checkbox" checked={autoRun} onChange={toggleAutoRun} />
          Auto-run show commands
        </label>
      </div>

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
        {st.turns.length === 0 && !provider && (
          <div className="tools-empty setup">
            <p className="strong">Set up the assistant</p>
            {local?.runtime.available ? (
              <>
                <p>The app can run a model on this computer; nothing leaves it. Detected: {local.hardware.summary}.</p>
                {recommended && !downloading && (
                  <div className="row">
                    <button className="btn primary" onClick={() => void downloadRecommended(recommended.id)}>Download {recommended.name} ({formatBytes(recommended.sizeBytes)})</button>
                    <button className="btn" onClick={onOpenSettings}>Other models and cloud providers…</button>
                  </div>
                )}
                {downloading && (
                  <div className="download">
                    <div className="row"><span>Downloading {local.models.find((m) => m.id === downloading)?.name}…</span><span className="grow" />{progress && <span className="quiet small">{formatBytes(progress.received)} of {formatBytes(progress.total)} · {formatBytes(progress.bytesPerSec)}/s</span>}</div>
                    <progress value={progress?.received ?? 0} max={progress?.total || 1} />
                  </div>
                )}
                {downloadError && <p className="warn-text">{downloadError}</p>}
                <p className="quiet small">{recommended?.blurb} Downloads resume if interrupted and are checked against the publisher's checksum.</p>
              </>
            ) : (
              <p>Add a provider in <button className="link" onClick={onOpenSettings}>Settings → Assistant</button>: Ollama or LM Studio on this computer, or a cloud provider with a key in the keychain.{local?.runtime.note ? ` ${local.runtime.note}` : ""}</p>
            )}
          </div>
        )}
        {st.turns.length === 0 && provider && (
          <div className="tools-empty">
            <p>Ask about the device, or select output and choose <em>Explain output</em>. {autoRun ? "The assistant runs the read-only commands it needs on its own and shows each step here; configuration changes appear as cards that wait for your approval." : "Proposed commands appear as cards: read-only ones run when you approve, changes need a second confirmation and a backup."}</p>
          </div>
        )}
        {st.turns.map((t, ti) => (
          <div key={ti} className={`turn ${t.role}`}>
            {t.role === "user" ? (
              t.display ? <div className="user-note">{t.display}</div> : <pre className="user-text">{t.content.split("\n\n")[0]}{t.content.includes("```") ? <span className="quiet"> · with output</span> : ""}</pre>
            ) : (
              <>
                {t.steps && t.steps.length > 0 && (
                  <div className="steps">
                    {t.steps.map((s, si) => (
                      <details key={si} className={`step ${s.status}`}>
                        <summary>
                          <span className="step-icon" aria-hidden="true">{s.status === "running" ? "…" : s.status === "done" ? "✓" : s.status === "blocked" ? "!" : "✗"}</span>
                          <span className="step-label"><Markdown text={s.label} /></span>
                          {s.status === "running" && s.detail && <span className="quiet small"> · {s.detail}</span>}
                          {s.status !== "running" && s.ms > 0 && <span className="quiet small"> · {s.ms >= 1000 ? `${(s.ms / 1000).toFixed(1)} s` : `${s.ms} ms`}</span>}
                          {s.outputs.length > 0 && <span className="quiet small"> · {s.outputs.reduce((n, o) => n + o.output.split("\n").length, 0)} lines</span>}
                        </summary>
                        {s.note && <div className="quiet small step-note">{s.note}</div>}
                        {s.outputs.map((o, k) => <pre key={k} className={o.error ? "rejected" : ""}>{o.command ? `${o.command}\n` : ""}{o.output || "(no output)"}</pre>)}
                      </details>
                    ))}
                  </div>
                )}
                <Markdown text={t.content || (t.streaming ? "…" : "")} hideCommands={!!t.blocks?.length} />
                {t.redacted ? <div className="quiet small">{t.redacted} secret{t.redacted === 1 ? "" : "s"} redacted before sending</div> : null}
                {t.evidence && (
                  <div className="evidence-note" title={[...t.evidence.notFound.map((q) => `not in the output: ${q}`), ...t.evidence.unquoted.map((q) => `quotes nothing: ${q}`)].join("\n")}>
                    {t.evidence.notFound.length > 0 && <span>{t.evidence.notFound.length} "Verified" quote{t.evidence.notFound.length === 1 ? "" : "s"} not found in the output the assistant saw. </span>}
                    {t.evidence.unquoted.length > 0 && <span>{t.evidence.unquoted.length} "Verified" claim{t.evidence.unquoted.length === 1 ? "" : "s"} quote{t.evidence.unquoted.length === 1 ? "s" : ""} no output line. </span>}
                    Treat those as hypotheses until you see the lines yourself.
                  </div>
                )}
                {t.blocks?.map((b, bi) => b.state !== "dismissed" && (
                  <div key={bi} className={`review ${b.kind} ${b.state}`}>
                    <div className="review-head">
                      <span className={`kind-chip ${b.kind}`}>{KIND_LABEL[b.kind]}</span>
                      <span className="strong">{b.title || `${b.items.length} command${b.items.length === 1 ? "" : "s"}`}</span>
                      {b.state === "superseded" && <span className="quiet small">superseded by a newer proposal</span>}
                      <span className="grow" />
                      {b.state !== "superseded" && <span className={`target-chip${environment === "production" ? " production" : ""}`} title="Where these commands run">→ {facts?.hostname || deviceName || hostname || "this device"}{environment ? ` · ${environment}` : ""}</span>}
                      {b.state === "proposed" && <button className="icon-btn" title="Dismiss" onClick={() => { b.state = "dismissed"; refresh(); }}><CloseIcon /></button>}
                    </div>
                    {b.state !== "superseded" && <ol className="review-lines">
                      {b.items.map((it, k) => (
                        <li key={k} className={it.kind}><code>{it.command}</code>{hasPlaceholder(it.command) && <span className="reason warn-text">fill in the placeholder first</span>}{it.kind !== "read-only" && <span className="reason">{it.reason}</span>}</li>
                      ))}
                    </ol>}
                    {b.state === "proposed" && (
                      <div className="row">
                        <button className="btn small" onClick={() => void navigator.clipboard.writeText(b.items.map((i) => i.command).join("\n"))}>Copy</button>
                        {b.kind === "read-only" ? (
                          <button className="btn small primary" disabled={!canRun || b.items.some((i) => hasPlaceholder(i.command))} onClick={() => void run(ti, bi, false)}>Run on the device</button>
                        ) : (
                          <button className="btn small primary" disabled={!canRun || b.items.some((i) => hasPlaceholder(i.command))} onClick={() => setConfirming({ turn: ti, block: bi, backup: true, reviewed: false, typed: "" })}>Review and apply…</button>
                        )}
                        {!canRun && <span className="quiet small">connect to run</span>}
                        {canRun && b.items.some((i) => hasPlaceholder(i.command)) && <span className="quiet small">has placeholders: copy and edit it</span>}
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
                        <summary>{outputSummary(b)}{b.backup ? " · backup saved" : ""}</summary>
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
          placeholder={provider ? "Ask about the device… (selected output is attached)" : "Set up a model above or in Settings → Assistant"}
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
          <button type="button" className="btn" onClick={stopNow}><StopIcon /> Stop</button>
        ) : (
          <button type="submit" className="btn primary" disabled={!provider || !input.trim()}>Ask</button>
        )}
        {st.turns.length > 0 && !busy && <button type="button" className="btn" title="Clear the conversation" onClick={() => { st.turns = []; st.seen.clear(); refresh(); }}>Clear</button>}
      </form>
    </div>
  );
}
