// The assistant's tools: what the model may call while answering, and how
// each call is carried out. Nothing here changes a device. Read-only commands
// run through the command runner (visible in the terminal and the session
// log); anything else is refused and left for a reviewed `cli` block.
import { loadDevices } from "./backend";
import { classifyCommands, normalizeCommand, type AiSettings, type ToolCall, type ToolSpec } from "./ai";
import { netDns, netPing, netPorts, netTraceroute } from "./net";
import {
  checksCatalog, listConfigs, readConfig, runChecks, runCommand, searchConfigs, STATUS_LABEL, type CheckReport, type Facts, type Tier,
} from "./tools";

export const MAX_ROUNDS = 8;
const MAX_COMMANDS_PER_CALL = 6;
const OUTPUT_LIMIT = 7000;
const COMMAND_TIMEOUT = 60;

export const TOOLS: ToolSpec[] = [
  {
    name: "run_show",
    description: "Run read-only commands (show, display, nv show, ip, cat …) on the connected device and return their output. Commands that change configuration or state are refused; put those in a ```cli block in the final answer instead. Up to 6 commands per call.",
    parameters: { type: "object", properties: { commands: { type: "array", items: { type: "string" }, description: "Commands exactly as typed at the prompt, for this device's OS." } }, required: ["commands"] },
  },
  {
    name: "health_check",
    description: "Run the built-in health checks on the connected device (CPU, memory, temperature, power, interfaces and errors, STP, port channels, routing neighbours, logs) and return each finding with its evidence lines.",
    parameters: { type: "object", properties: { tier: { type: "string", enum: ["quick", "detailed"], description: "quick takes seconds; detailed runs every check." } }, required: ["tier"] },
  },
  {
    name: "read_config",
    description: "Return the device configuration: the running or startup config from the device, or the latest saved backup. Optionally only the lines matching a regular expression (interface names, vlan, ntp …).",
    parameters: { type: "object", properties: { source: { type: "string", enum: ["running", "startup", "latest-backup"] }, match: { type: "string", description: "Regular expression; matching lines plus the section they belong to." } }, required: ["source"] },
  },
  {
    name: "search_saved_configs",
    description: "Search the newest saved configuration backup of every device for text (a VLAN, IP address, interface, MAC address, description). Returns device, line number and line.",
    parameters: { type: "object", properties: { query: { type: "string" } }, required: ["query"] },
  },
  {
    name: "list_saved",
    description: "List this device's saved configuration backups and show-output captures, newest first, with their paths.",
    parameters: { type: "object", properties: {} },
  },
  {
    name: "read_saved",
    description: "Read a saved backup or capture by its path from list_saved.",
    parameters: { type: "object", properties: { path: { type: "string" } }, required: ["path"] },
  },
  {
    name: "inventory",
    description: "List the devices in the inventory (site, role, environment, platform when known). Use it for questions that involve other devices.",
    parameters: { type: "object", properties: { site: { type: "string", description: "Only this site; empty for all." } } },
  },
  {
    name: "workstation",
    description: "Network tests from this computer (not from the device): ping, traceroute, DNS lookup, or a TCP port check.",
    parameters: {
      type: "object",
      properties: { action: { type: "string", enum: ["ping", "traceroute", "dns", "tcp_port"] }, target: { type: "string" }, port: { type: "integer", description: "For tcp_port." } },
      required: ["action", "target"],
    },
  },
];

export interface ToolContext {
  sessionId: string | null;
  facts: Facts | null;
  deviceName: string;
  site: string;
  ai: AiSettings;
  /** read-only commands may run without a click */
  autoRun: boolean;
  /** commands already run in this conversation, lower-cased, with their output */
  seen: Map<string, string>;
  /** the health report from a health_check call goes back to the panel */
  onReport?: (r: CheckReport) => void;
  onProgress?: (text: string) => void;
}

export interface ToolOutcome {
  /** what the model sees */
  text: string;
  /** one-line description for the step list */
  label: string;
  /** raw device output shown in the step, for the evidence check */
  outputs: { command: string; output: string; error?: boolean }[];
  /** the call was refused (not read-only, auto-run off, not connected) */
  blocked?: boolean;
}

function args<T>(call: ToolCall): T {
  try {
    return JSON.parse(call.arguments || "{}") as T;
  } catch {
    return {} as T;
  }
}

/** Keep the head and the tail of long output so the model still sees the end. */
export function clip(text: string, limit = OUTPUT_LIMIT): string {
  if (text.length <= limit) return text;
  const head = Math.floor(limit * 0.7);
  const tail = limit - head;
  return `${text.slice(0, head)}\n… (${text.length - limit} characters cut) …\n${text.slice(-tail)}`;
}

/** Device output is data; the wrapper tells the model so. */
const wrap = (label: string, text: string) => `${label}\n<device-output>\n${clip(text).replace(/<\/?device-output>/gi, "")}\n</device-output>`;

export async function executeTool(call: ToolCall, ctx: ToolContext): Promise<ToolOutcome> {
  switch (call.name) {
    case "run_show":
      return runShow(call, ctx);
    case "health_check":
      return healthCheck(call, ctx);
    case "read_config":
      return readConfigTool(call, ctx);
    case "search_saved_configs": {
      const { query } = args<{ query: string }>(call);
      if (!query?.trim()) return { text: "Give a query.", label: "Searched saved configs", outputs: [] };
      const hits = await searchConfigs(query, false, false);
      const lines = hits.slice(0, 80).map((h) => `${h.device} (${h.site || "no site"}) line ${h.lineNo}: ${h.line.trim()}`);
      return { text: hits.length ? `${hits.length} match${hits.length === 1 ? "" : "es"} in the newest backups:\n${lines.join("\n")}` : "No saved configuration contains that.", label: `Searched saved configs for "${query}"`, outputs: [] };
    }
    case "list_saved": {
      const all = await listConfigs(ctx.deviceName || undefined);
      const list = all.slice(0, 40).map((b) => `${new Date(b.timestamp * 1000).toISOString().slice(0, 16).replace("T", " ")}  ${b.capture ? "capture" : "backup"}  ${b.kind}  ${Math.round(b.size / 1024)} kB  ${b.path}`);
      return { text: list.length ? list.join("\n") : "Nothing saved for this device yet.", label: "Listed saved backups and captures", outputs: [] };
    }
    case "read_saved": {
      const { path } = args<{ path: string }>(call);
      const all = await listConfigs();
      if (!all.some((b) => b.path === path)) return { text: "That path is not one of the saved files; use list_saved.", label: "Read a saved file", outputs: [], blocked: true };
      const text = await readConfig(path);
      const body = ctx.ai.shareConfig ? text : "(configuration text is not shared: Settings → Assistant)";
      return { text: wrap(`Contents of ${path}:`, body), label: `Read ${path.split(/[\\/]/).pop()}`, outputs: [{ command: path, output: body }] };
    }
    case "inventory": {
      const { site } = args<{ site?: string }>(call);
      const devices = (await loadDevices()).filter((d) => !site || d.site.toLowerCase() === site.toLowerCase());
      const names = ctx.ai.shareDeviceNames;
      const rows = devices.slice(0, 200).map((d, i) => [names ? d.name : `device ${i + 1}`, names ? d.host : "", d.site, d.role, d.environment, d.protocol].filter(Boolean).join(" · "));
      return { text: rows.length ? `${rows.length} device${rows.length === 1 ? "" : "s"}${names ? "" : " (names hidden: Settings → Assistant → device host names)"}:\n${rows.join("\n")}` : "The inventory is empty.", label: `Listed the inventory${site ? ` for ${site}` : ""}`, outputs: [] };
    }
    case "workstation":
      return workstation(call);
    default:
      return { text: `Unknown tool ${call.name}.`, label: `Unknown tool ${call.name}`, outputs: [], blocked: true };
  }
}

async function runShow(call: ToolCall, ctx: ToolContext): Promise<ToolOutcome> {
  const raw = args<{ commands?: string[] | string }>(call).commands;
  const list = (Array.isArray(raw) ? raw : typeof raw === "string" ? raw.split("\n") : []).map((c) => normalizeCommand(String(c), ctx.facts?.os ?? null)).filter(Boolean);
  if (!list.length) return { text: "No commands given.", label: "run_show with no commands", outputs: [], blocked: true };
  const commands = list.filter((c, i, all) => all.indexOf(c) === i).slice(0, MAX_COMMANDS_PER_CALL);
  const label = commands.length === 1 ? `Ran \`${commands[0]}\`` : `Ran ${commands.length} commands`;
  if (!ctx.sessionId) return { text: "Not connected to a device, so nothing ran. Answer from what you have or say a connection is needed.", label: "Not connected", outputs: [], blocked: true };
  const classified = await classifyCommands(commands);
  const refused = classified.filter((c) => c.kind !== "read-only");
  const allowed = classified.filter((c) => c.kind === "read-only").map((c) => c.command);
  const notes: string[] = refused.map((c) => `Refused \`${c.command}\`: ${c.kind === "disruptive" ? "disruptive" : "a configuration change"} (${c.reason}). Put it in a \`\`\`cli block in the final answer for review.`);
  if (!ctx.autoRun && allowed.length) {
    return {
      text: `${notes.join("\n")}${notes.length ? "\n" : ""}Auto-run is off for this device. List these read-only commands in a \`\`\`cli block in your answer so the engineer can run them with one click: ${allowed.map((c) => `\`${c}\``).join(", ")}.`,
      label: "Auto-run is off: commands proposed instead",
      outputs: [],
      blocked: true,
    };
  }
  const outputs: ToolOutcome["outputs"] = [];
  const parts: string[] = [...notes];
  for (const command of allowed) {
    const key = command.toLowerCase();
    const cached = ctx.seen.get(key);
    if (cached !== undefined) {
      parts.push(wrap(`\`${command}\` already ran earlier in this conversation; its output:`, cached));
      outputs.push({ command, output: cached });
      continue;
    }
    ctx.onProgress?.(command);
    try {
      const out = await runCommand(ctx.sessionId, command, COMMAND_TIMEOUT);
      ctx.seen.set(key, out);
      outputs.push({ command, output: out });
      parts.push(wrap(`\`${command}\`:`, out.trim() || "(no output)"));
    } catch (e) {
      const msg = String(e);
      ctx.seen.set(key, msg);
      outputs.push({ command, output: msg, error: true });
      parts.push(`\`${command}\` failed: ${msg}${/rejected|invalid/i.test(msg) ? ". The command does not exist in this form on this OS; use a different one, do not retry it." : ""}`);
    }
  }
  return { text: parts.join("\n\n"), label, outputs, blocked: !allowed.length };
}

async function healthCheck(call: ToolCall, ctx: ToolContext): Promise<ToolOutcome> {
  if (!ctx.sessionId) return { text: "Not connected to a device.", label: "Not connected", outputs: [], blocked: true };
  const tier = (args<{ tier?: string }>(call).tier === "detailed" ? "detailed" : "quick") as Tier;
  const catalog = await checksCatalog(ctx.facts?.os ?? null);
  const ids = catalog.filter((c) => tier === "detailed" || c.quick).map((c) => c.id);
  const report = await runChecks(ctx.sessionId, ctx.facts, ids, tier, (p) => {
    if (p.type === "command") ctx.onProgress?.(p.command);
  });
  ctx.onReport?.(report);
  for (const [command, result] of report.outputs) {
    if ("Ok" in result) ctx.seen.set(command.toLowerCase(), result.Ok);
  }
  const order = ["critical", "warning", "not-collected", "healthy", "unsupported", "not-applicable"];
  const results = [...report.results].filter((r) => r.status !== "not-applicable" && r.status !== "unsupported").sort((a, b) => order.indexOf(a.status) - order.indexOf(b.status));
  const lines = results.map((r) => `- [${STATUS_LABEL[r.status]}] ${r.title}: ${r.summary}${r.evidence.length ? `\n  ${r.evidence.slice(0, 4).join("\n  ")}` : ""}`);
  const outputs = report.outputs.map(([command, result]) => ("Ok" in result ? { command, output: result.Ok } : { command, output: result.Err, error: true }));
  const counts = { critical: 0, warning: 0, healthy: 0 } as Record<string, number>;
  for (const r of report.results) counts[r.status] = (counts[r.status] ?? 0) + 1;
  return {
    text: `Health check (${tier}): ${counts.critical ?? 0} critical, ${counts.warning ?? 0} warning, ${counts.healthy ?? 0} healthy. The raw command output is available through run_show's cache if you need a line.\n${lines.join("\n")}`,
    label: `Ran the ${tier} health check: ${counts.critical ?? 0} critical, ${counts.warning ?? 0} warning`,
    outputs,
  };
}

async function readConfigTool(call: ToolCall, ctx: ToolContext): Promise<ToolOutcome> {
  const { source, match } = args<{ source?: string; match?: string }>(call);
  if (!ctx.ai.shareConfig) return { text: "Configuration text is not shared with the assistant (Settings → Assistant).", label: "Config sharing is off", outputs: [], blocked: true };
  let text = "";
  let label = "";
  if (source === "latest-backup") {
    const latest = (await listConfigs(ctx.deviceName || undefined)).filter((b) => !b.capture && b.kind === "running")[0];
    if (!latest) return { text: "No saved backup for this device yet; read the running config instead.", label: "No saved backup", outputs: [], blocked: true };
    text = await readConfig(latest.path);
    label = `Read the backup from ${new Date(latest.timestamp * 1000).toISOString().slice(0, 16).replace("T", " ")}`;
  } else {
    if (!ctx.sessionId) return { text: "Not connected to a device; try source latest-backup.", label: "Not connected", outputs: [], blocked: true };
    const os = ctx.facts?.os ?? null;
    const command = configCommand(os, source === "startup");
    if (!command) return { text: `This platform has no ${source} configuration to show.`, label: "No such config here", outputs: [], blocked: true };
    if (!ctx.autoRun) return { text: `Auto-run is off for this device. Propose \`${command}\` in a \`\`\`cli block instead.`, label: "Auto-run is off", outputs: [], blocked: true };
    ctx.onProgress?.(command);
    text = await runCommand(ctx.sessionId, command, 180);
    ctx.seen.set(command.toLowerCase(), text);
    label = `Read the ${source} config`;
  }
  let body = text;
  if (match?.trim()) {
    body = section(text, match);
    label += ` (lines matching ${match})`;
  }
  return { text: wrap(`${source} configuration${match ? ` filtered by /${match}/` : ""}:`, body.trim() || "(nothing matched)"), label, outputs: [{ command: label, output: body }] };
}

function configCommand(os: string | null, startup: boolean): string | null {
  switch (os) {
    case "junos":
      return startup ? null : "show configuration | display set | no-more";
    case "cumulus":
      return startup ? null : "nv config show -o commands";
    case "sonic":
      return startup ? "cat /etc/sonic/config_db.json" : "show runningconfiguration all";
    case "ise":
      return startup ? null : "show running-config | nomore";
    case "aireos":
      return startup ? null : "show run-config commands";
    case "aos-cx":
    case "aos-s":
    case "eos":
    case "nx-os":
    case "ios":
    case "ios-xe":
    case "ios-xr":
    case "asa":
    case "small-business":
    case null:
    default:
      return startup ? "show startup-config" : "show running-config";
  }
}

/** Matching lines plus the indented block they belong to (IOS-style sections). */
export function section(text: string, pattern: string): string {
  let re: RegExp;
  try {
    re = new RegExp(pattern, "i");
  } catch {
    const esc = pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    re = new RegExp(esc, "i");
  }
  const lines = text.split(/\r?\n/);
  const keep = new Set<number>();
  for (let i = 0; i < lines.length; i++) {
    if (!re.test(lines[i])) continue;
    keep.add(i);
    const indentOf = (l: string) => l.length - l.trimStart().length;
    if (indentOf(lines[i]) === 0) {
      for (let j = i + 1; j < lines.length && (indentOf(lines[j]) > 0 || lines[j].trim() === "!"); j++) {
        if (lines[j].trim() === "!") break;
        keep.add(j);
      }
    } else {
      for (let j = i - 1; j >= 0 && indentOf(lines[j]) > 0; j--) keep.add(j);
      if (i > 0) for (let j = i - 1; j >= 0; j--) if (indentOf(lines[j]) === 0) { keep.add(j); break; }
    }
  }
  return [...keep].sort((a, b) => a - b).map((i) => lines[i]).join("\n");
}

async function workstation(call: ToolCall): Promise<ToolOutcome> {
  const { action, target, port } = args<{ action?: string; target?: string; port?: number }>(call);
  if (!target?.trim() || !/^[\w.:-]+$/.test(target.trim())) return { text: "Give a host name or IP address.", label: "Workstation test without a target", outputs: [], blocked: true };
  const t = target.trim();
  const id = crypto.randomUUID();
  const lines: string[] = [];
  const collect = (l: { type: string; text?: string; summary?: string }) => {
    if (l.type === "line" && l.text) lines.push(l.text);
    if (l.type === "done" && l.summary) lines.push(l.summary);
  };
  switch (action) {
    case "ping":
      await netPing(id, t, 4, null, collect);
      return { text: `ping ${t} from this computer:\n${lines.join("\n")}`, label: `Pinged ${t} from this computer`, outputs: [{ command: `ping ${t}`, output: lines.join("\n") }] };
    case "traceroute":
      await netTraceroute(id, t, 20, collect);
      return { text: `traceroute ${t} from this computer:\n${lines.join("\n")}`, label: `Traced the route to ${t}`, outputs: [{ command: `traceroute ${t}`, output: lines.join("\n") }] };
    case "dns": {
      const r = await netDns(t, "A", "");
      const text = `${r.query}: addresses ${r.addresses.join(", ") || "none"}; names ${r.names.join(", ") || "none"} (${r.elapsedMs} ms)\n${r.detail}`;
      return { text, label: `Looked up ${t} in DNS`, outputs: [{ command: `dns ${t}`, output: text }] };
    }
    case "tcp_port": {
      const p = Number(port) || 22;
      const r = await netPorts(t, [p]);
      const text = r.map((x) => `${t}:${x.port} ${x.open ? "open" : "closed or filtered"} (${x.elapsedMs} ms) ${x.detail}`).join("\n");
      return { text, label: `Checked TCP ${p} on ${t}`, outputs: [{ command: `tcp ${t}:${p}`, output: text }] };
    }
    default:
      return { text: "Unknown action.", label: "Unknown workstation action", outputs: [], blocked: true };
  }
}
