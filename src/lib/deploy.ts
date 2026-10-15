// Templates, reviewed deployment and the firmware assistant (src-tauri/src/deploy.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Os } from "./tools";

export type VarType = "text" | "int" | "vlan" | "vlans" | "ip" | "prefix" | "interface" | "choice" | "bool";

export interface Var {
  name: string;
  label: string;
  kind: VarType;
  required: boolean;
  default: string;
  help: string;
  choices: string[];
}

export interface Template {
  id: string;
  title: string;
  category: string;
  description: string;
  vars: Var[];
  ios: string[];
  nxos: string[];
}

export interface FieldError {
  name: string;
  message: string;
}

export type RenderResult = { result: "ok"; rendered: { commands: string[]; warnings: string[] } } | { result: "invalid"; errors: FieldError[] };

export const templatesList = () => invoke<Template[]>("templates_list");
export const templateRender = (id: string, os: Os | null, values: Record<string, string>) => invoke<RenderResult>("template_render", { id, os, values });

/** secrets the initial setup asks for at deploy time */
export const SECRET_VARS: { name: string; template: string; label: string }[] = [
  { name: "admin.password", template: "initial-setup", label: "Admin password" },
  { name: "enable.password", template: "initial-setup", label: "Enable secret" },
  { name: "snmp.auth", template: "initial-setup", label: "SNMPv3 auth password" },
  { name: "snmp.priv", template: "initial-setup", label: "SNMPv3 privacy password" },
];

export interface DeployOptions {
  backup: boolean;
  rollbackPoint: boolean;
  safetyReloadMinutes: number;
}

export type DeployEvent =
  | { type: "step"; phase: string; command: string; index: number; total: number }
  | { type: "output"; phase: string; command: string; output: string; ok: boolean };

export interface DeployStep {
  phase: string;
  command: string;
  output: string;
  ok: boolean;
}

export interface DeployReport {
  started: number;
  finished: number;
  backup: string | null;
  rollbackCommand: string | null;
  safetyReload: boolean;
  pending: string;
  saveCommand: string;
  steps: DeployStep[];
  sent: number;
  failed: boolean;
  error: string;
}

export function deployRun(id: string, os: Os | null, site: string, device: string, commands: string[], verify: string[], options: DeployOptions, onEvent: (e: DeployEvent) => void): Promise<DeployReport> {
  const events = new Channel<DeployEvent>();
  events.onmessage = onEvent;
  return invoke<DeployReport>("deploy_run", { id, os, site, device, commands, verify, options, events });
}
export const deployFinish = (id: string, os: Os | null, cancelReload: boolean, save: boolean) => invoke<DeployStep[]>("deploy_finish", { id, os, cancelReload, save });
export const deployRollback = (id: string, os: Os | null, command: string) => invoke<DeployStep>("deploy_rollback", { id, os, command });

export interface FileSystem {
  name: string;
  totalBytes: number;
  freeBytes: number;
  files: [string, number][];
}

export interface FirmwareInfo {
  os: Os | null;
  version: string;
  model: string;
  image: string;
  boot: string;
  mode: string;
  stackMembers: number;
  filesystems: FileSystem[];
}

export interface PlanCheck {
  title: string;
  status: "pass" | "warn" | "fail" | "unknown";
  detail: string;
}

export interface Plan {
  checks: PlanCheck[];
  transfer: string[];
  verify: string[];
  activate: string[];
  reload: string[];
  post: string[];
  notes: string[];
  ok: boolean;
}

export const firmwareInspect = (id: string) => invoke<FirmwareInfo>("firmware_inspect", { id });
export const firmwarePlan = (info: FirmwareInfo, url: string, image: string, size: number, md5: string) => invoke<Plan>("firmware_plan", { info, url, image, size, md5 });

export const human = (b: number) => (b >= 2 ** 30 ? `${(b / 2 ** 30).toFixed(1)} GB` : `${Math.round(b / 2 ** 20)} MB`);
