// AI assistant: provider settings, the system prompt, streaming chat and the
// commands a reply proposes. Secrets are redacted on the Rust side before
// anything is sent (src-tauri/src/ai.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import type { CheckReport, Facts } from "./tools";
import { OS_LABEL, STATUS_LABEL } from "./tools";

export type ProviderKind = "ollama" | "lmstudio" | "openai" | "anthropic" | "compatible";

export const PROVIDER_LABEL: Record<ProviderKind, string> = {
  ollama: "Ollama (local)", lmstudio: "LM Studio (local)", openai: "OpenAI", anthropic: "Anthropic", compatible: "OpenAI-compatible endpoint",
};

export const PROVIDER_DEFAULT_URL: Record<ProviderKind, string> = {
  ollama: "http://127.0.0.1:11434", lmstudio: "http://127.0.0.1:1234", openai: "https://api.openai.com", anthropic: "https://api.anthropic.com", compatible: "",
};

export interface Provider {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  model: string;
  /** cloud providers get an API key in the keychain; the UI only knows if one is saved */
  hasKey: boolean;
}

export interface AiSettings {
  providers: Provider[];
  activeId: string;
  /** what may leave the app */
  shareOutput: boolean;
  shareConfig: boolean;
  shareDeviceNames: boolean;
}

export const DEFAULT_AI: AiSettings = { providers: [], activeId: "", shareOutput: true, shareConfig: true, shareDeviceNames: false };

export function newProvider(kind: ProviderKind): Provider {
  return { id: crypto.randomUUID(), name: PROVIDER_LABEL[kind], kind, baseUrl: PROVIDER_DEFAULT_URL[kind], model: kind === "ollama" ? "qwen3:8b" : "", hasKey: false };
}

export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export type AiEvent = { type: "token"; text: string } | { type: "done"; text: string; redacted: number } | { type: "error"; message: string };

const spec = (p: Provider) => ({ id: p.id, kind: p.kind, baseUrl: p.baseUrl, model: p.model });

export const aiModels = (p: Provider) => invoke<string[]>("ai_models", { provider: spec(p) });
export const aiCancel = (id: string) => invoke("ai_cancel", { id });

export function aiChat(id: string, p: Provider, messages: ChatMessage[], onEvent: (e: AiEvent) => void): Promise<string> {
  const events = new Channel<AiEvent>();
  events.onmessage = onEvent;
  return invoke<string>("ai_chat", { id, provider: spec(p), messages, events });
}

export type Kind = "read-only" | "config" | "disruptive";
export interface Classified {
  command: string;
  kind: Kind;
  reason: string;
}
export const classifyCommands = (commands: string[]) => invoke<Classified[]>("classify_commands", { commands });

/* ------------------------------------------------------------- prompt */

export interface Context {
  facts: Facts | null;
  hostname: string;
  environment: string;
  report: CheckReport | null;
  /** terminal selection or recent output */
  output: string;
  outputLabel: string;
  settings: AiSettings;
}

export function systemPrompt(ctx: Context): string {
  const os = ctx.facts?.os ? `${OS_LABEL[ctx.facts.os]} ${ctx.facts.version}`.trim() : "an unknown OS (ask which, or give IOS syntax and say so)";
  const model = ctx.facts?.model ? ` on a ${ctx.facts.model}` : "";
  const name = ctx.settings.shareDeviceNames && ctx.hostname ? ` named ${ctx.hostname}` : "";
  const env = ctx.environment ? ` in the ${ctx.environment} environment` : "";
  return [
    `You are the assistant inside OmniConsole, a terminal for network engineers. The engineer is connected to a Cisco device${name}${model} running ${os}${env}.`,
    "Tailor every command to that OS and version. Be concrete and brief; the engineer is skilled and reads output themselves.",
    "Structure answers as: **Verified** (only what the provided output shows, quote the lines), **Likely** (inferences, say why), **Next steps** (commands to run). Never present a guess as a fact. If information is missing, ask for the exact show command instead of assuming.",
    "Put every command you propose in a fenced code block tagged `cli`, one command per line, in the order to run them. Use one block for all the read-only (show) commands of a reply, and a separate block for each configuration change; do not scatter single commands across many blocks or repeat a command outside its block. Configuration blocks start with `configure terminal` and end with `end`. Add a one-line comment above a block starting with `!` saying what it does. Never include `reload`, `write erase` or image changes unless explicitly asked, and then say what they do first.",
    "The app shows each block to the engineer for review; nothing runs without approval, and configuration changes need a second confirmation and a backup first. So do not tell them to be careful about running commands; describe the effect instead.",
    "Secrets in the engineer's text have been replaced with <removed>; do not comment on them.",
    ctx.report ? `\nLatest health check (${new Date(ctx.report.finished * 1000).toISOString()}):\n${ctx.report.results.filter((r) => r.status !== "skipped").map((r) => `- [${STATUS_LABEL[r.status]}] ${r.title}: ${r.summary}${r.evidence.length ? `\n  ${r.evidence.slice(0, 4).join("\n  ")}` : ""}`).join("\n")}` : "",
  ].filter(Boolean).join("\n\n");
}

/** The user turn for a quick action, with the shared output attached. */
export function userTurn(question: string, ctx: Context): string {
  const parts = [question];
  if (ctx.output.trim() && ctx.settings.shareOutput) {
    parts.push(`${ctx.outputLabel}:\n\`\`\`\n${ctx.output.trim().slice(-12000)}\n\`\`\``);
  } else if (ctx.output.trim()) {
    parts.push("(The terminal output was not shared: output sharing is off in Settings → Assistant.)");
  }
  return parts.join("\n\n");
}

export const QUICK_ACTIONS: { id: string; title: string; question: string; needsOutput: boolean }[] = [
  { id: "explain", title: "Explain output", question: "Explain this output: what it says, anything abnormal, and what to check next.", needsOutput: true },
  { id: "next", title: "Next steps", question: "Given the latest health check and this output, what should I look at next? Give the commands.", needsOutput: false },
  { id: "diagnose", title: "Diagnostics", question: "Interpret the latest health check results. Which findings matter, what is the likely cause of each, and how do I confirm it?", needsOutput: false },
  { id: "config", title: "Generate config", question: "Generate the configuration for the following, for this device's OS, as a reviewable `cli` block with a backup-friendly structure: ", needsOutput: false },
];

/* ------------------------------------------------------- reply parsing */

export interface Block {
  /** the `!` comment above the block, if any */
  title: string;
  commands: string[];
}

/** Fenced `cli` (or untagged, command-looking) blocks in a reply. */
export function commandBlocks(text: string): Block[] {
  const out: Block[] = [];
  const re = /```([\w-]*)\n([\s\S]*?)```/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const tag = m[1].toLowerCase();
    const body = m[2];
    const lines = body.split("\n").map((l) => l.replace(/^\s*[\w./-]+[#>]\s?/, "").trimEnd());
    const comments = lines.filter((l) => l.trim().startsWith("!")).map((l) => l.replace(/^\s*!\s?/, ""));
    const commands = lines.filter((l) => l.trim() && !l.trim().startsWith("!") && !l.trim().startsWith("#"));
    if (!commands.length) continue;
    const ok = tag === "cli" || tag === "cisco" || tag === "ios" || tag === "" || tag === "text" || tag === "bash" || tag === "shell";
    if (!ok) continue;
    if (tag === "" && !commands.every((c) => /^[a-z]/i.test(c)) ) continue;
    out.push({ title: comments[0] ?? "", commands });
  }
  return out;
}

/* ------------------------------------------------------------ audit */

export interface AuditEntry {
  time: number;
  device: string;
  environment: string;
  kind: Kind;
  commands: string[];
  backup: string | null;
  result: "sent" | "cancelled" | "failed";
  note: string;
}
