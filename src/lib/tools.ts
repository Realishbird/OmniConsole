// Types and wrappers for the Tools panel commands (src-tauri/src/tools.rs).
import { Channel, invoke } from "@tauri-apps/api/core";

export type Os = "ios" | "ios-xe" | "nx-os" | "ios-xr" | "asa" | "small-business" | "cat-os" | "aos-s" | "aos-cx" | "junos" | "eos" | "sonic" | "cumulus" | "unknown";

export const OS_LABEL: Record<Os, string> = {
  ios: "IOS", "ios-xe": "IOS-XE", "nx-os": "NX-OS", "ios-xr": "IOS-XR", asa: "ASA",
  "small-business": "Small Business", "cat-os": "CatOS", "aos-s": "Aruba AOS-S", "aos-cx": "Aruba AOS-CX",
  junos: "Junos", eos: "Arista EOS", sonic: "SONiC", cumulus: "Cumulus Linux", unknown: "Unknown",
};

export const OS_VENDOR: Record<Os, string> = {
  ios: "Cisco", "ios-xe": "Cisco", "nx-os": "Cisco", "ios-xr": "Cisco", asa: "Cisco", "small-business": "Cisco", "cat-os": "Cisco",
  "aos-s": "Aruba (HPE)", "aos-cx": "Aruba (HPE)", junos: "Juniper", eos: "Arista", sonic: "Cisco (SONiC)", cumulus: "NVIDIA", unknown: "",
};

/** Every platform the checks know, for the manual override when detection is inconclusive. */
export const OS_CHOICES: Os[] = ["ios", "ios-xe", "nx-os", "ios-xr", "asa", "small-business", "aos-s", "aos-cx", "junos", "eos", "sonic", "cumulus"];

export interface Facts {
  os: Os | null;
  hostname: string;
  model: string;
  version: string;
  image: string;
  uptime: string;
  serial: string;
  reloadReason: string;
}

export type Status = "healthy" | "warning" | "critical" | "not-applicable" | "unsupported" | "not-collected";

export const STATUS_LABEL: Record<Status, string> = { healthy: "Healthy", warning: "Warning", critical: "Critical", "not-applicable": "Not applicable", unsupported: "Unsupported", "not-collected": "Not collected" };
/** worst first */
export const STATUS_ORDER: Record<Status, number> = { critical: 0, warning: 1, "not-collected": 2, healthy: 3, unsupported: 4, "not-applicable": 5 };
export type Tier = "quick" | "detailed";

export interface Table {
  title: string;
  columns: string[];
  rows: string[][];
}

export interface CheckResult {
  id: string;
  title: string;
  category: string;
  status: Status;
  summary: string;
  evidence: string[];
  followUp: string[];
  commands: string[];
  table?: Table;
  /** unix seconds when the commands were collected */
  collectedAt: number;
  /** what to do about it, when the status is not healthy */
  nextStep: string;
}

export interface CheckInfo {
  id: string;
  title: string;
  category: string;
  quick: boolean;
  commands: string[];
}

export type CheckProgress =
  | { type: "command"; command: string; index: number; total: number }
  | { type: "output"; command: string; ok: boolean; bytes: number }
  | { type: "result"; result: CheckResult };

export interface CheckReport {
  os: Os;
  facts: Facts;
  started: number;
  finished: number;
  cancelled: boolean;
  results: CheckResult[];
  /** [command, {Ok: text} | {Err: message}] */
  outputs: [string, { Ok: string } | { Err: string }][];
}

export interface Backup {
  path: string;
  site: string;
  device: string;
  kind: string;
  timestamp: number;
  size: number;
}

export interface DiffLine {
  tag: " " | "+" | "-";
  old: number | null;
  new: number | null;
  text: string;
}

export interface DiffResult {
  added: number;
  removed: number;
  lines: DiffLine[];
  unified: string;
}

export interface SearchHit {
  path: string;
  site: string;
  device: string;
  timestamp: number;
  lineNo: number;
  line: string;
}

export interface BundleFile {
  name: string;
  contents: string;
}

export const runCommand = (id: string, command: string, timeoutSecs?: number) =>
  invoke<string>("run_command", { id, command, timeoutSecs });
export const detectDevice = (id: string) => invoke<Facts>("device_detect", { id });
export const checksCatalog = (os: Os | null) => invoke<CheckInfo[]>("checks_catalog", { os });
export const cancelChecks = (id: string) => invoke("checks_cancel", { id });

export function runChecks(id: string, facts: Facts | null, ids: string[], tier: Tier | null, onProgress: (p: CheckProgress) => void, osOverride: Os | null = null): Promise<CheckReport> {
  const events = new Channel<CheckProgress>();
  events.onmessage = onProgress;
  return invoke<CheckReport>("checks_run", { id, os: osOverride ?? facts?.os ?? null, facts, ids, tier, events });
}

export const backupConfig = (id: string, os: Os | null, site: string, device: string, kinds: string[]) =>
  invoke<Backup[]>("config_backup", { id, os, site, device, kinds });
export const saveReport = (site: string, device: string, report: CheckReport) =>
  invoke<Backup>("report_save", { site, device, report });
export const listConfigs = (device?: string) => invoke<Backup[]>("configs_list", { device: device ?? null });
export const readConfig = (path: string) => invoke<string>("config_read", { path });
export const deleteConfig = (path: string) => invoke("config_delete", { path });
export const diffConfigs = (oldText: string, newText: string, ignoreVolatile: boolean) =>
  invoke<DiffResult>("config_diff", { old: oldText, new: newText, ignoreVolatile });
export const searchConfigs = (query: string, regex: boolean, allVersions: boolean) =>
  invoke<SearchHit[]>("configs_search", { query, regex, allVersions });
export const configFolder = () => invoke<string>("config_folder");
export const openConfigFolder = () => invoke("open_config_folder");
export const revealConfig = (path: string) => invoke("reveal_config", { path });
export const saveBundle = (name: string, files: BundleFile[], maskSecrets: boolean) =>
  invoke<string | null>("save_bundle", { name, files, maskSecrets });

export function outputText(report: CheckReport, command: string): string | null {
  const entry = report.outputs.find(([c]) => c === command);
  if (!entry) return null;
  const v = entry[1];
  return "Ok" in v ? v.Ok : `(failed: ${v.Err})`;
}

export function stamp(ts: number): string {
  const d = new Date(ts * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

export function fileStamp(ts: number): string {
  return stamp(ts).replace(" ", "_").replaceAll(":", "-");
}

/** Build the files of a diagnostic bundle from a report (and optional configs). */
export function bundleFiles(report: CheckReport, extras: BundleFile[] = []): BundleFile[] {
  const files: BundleFile[] = [
    { name: "summary.txt", contents: summaryText(report) },
    { name: "facts.json", contents: JSON.stringify(report.facts, null, 2) },
    { name: "checks.json", contents: JSON.stringify(report.results, null, 2) },
  ];
  for (const [command, v] of report.outputs) {
    files.push({ name: `${command.replace(/[^\w.-]+/g, "_")}.txt`, contents: "Ok" in v ? v.Ok : `(failed: ${v.Err})` });
  }
  return files.concat(extras);
}

export function summaryText(report: CheckReport): string {
  const f = report.facts;
  const lines = [
    `OmniConsole health check  ${stamp(report.finished)}`,
    `${f.hostname || "device"}  ${OS_LABEL[report.os]} ${f.version}  ${f.model}${f.serial ? `  serial ${f.serial}` : ""}`,
    f.uptime ? `Uptime: ${f.uptime}` : "",
    report.cancelled ? "(cancelled before every command ran)" : "",
    "",
  ];
  for (const r of report.results) {
    lines.push(`[${STATUS_LABEL[r.status].toUpperCase().padEnd(7)}] ${r.title}: ${r.summary}`);
    for (const e of r.evidence.slice(0, 8)) lines.push(`           ${e}`);
    if (r.followUp.length) lines.push(`           next: ${r.followUp.join("  |  ")}`);
  }
  return lines.join("\n");
}

export type Change = { id: string; title: string; from: Status; to: Status; summary: string };

/** Status changes between a baseline and a new report. */
export function compareReports(baseline: CheckReport, current: CheckReport): Change[] {
  const out: Change[] = [];
  for (const r of current.results) {
    const b = baseline.results.find((x) => x.id === r.id);
    if (b && b.status !== r.status) out.push({ id: r.id, title: r.title, from: b.status, to: r.status, summary: r.summary });
  }
  return out;
}
