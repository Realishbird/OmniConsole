// Platform profiles (src-tauri/src/platforms.rs): what each OS calls things,
// its quick-action collections, troubleshooting sets and change workflow.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Os } from "./tools";

export interface Collection {
  id: string;
  title: string;
  commands: string[];
  /** why there is nothing to collect, when commands is empty */
  note: string;
}

export interface ConfigKind {
  kind: string;
  label: string;
  command: string;
}

export interface Workflow {
  candidate: boolean;
  enter: string[];
  exit: string[];
  commit: string[];
  commitConfirmed: string;
  confirm: string[];
  save: string[];
  checkpoint: string;
  rollback: string;
  restorable: string;
  notes: string[];
}

export interface Profile {
  os: Os;
  label: string;
  vendor: string;
  shell: boolean;
  noPaging: string | null;
  privilegedPrompt: string;
  enable: string | null;
  configs: ConfigKind[];
  collections: Collection[];
  troubleshooting: Collection[];
  workflow: Workflow;
  reference: string;
}

export interface PlatformInfo {
  os: Os;
  label: string;
}

export type CollectEvent =
  | { type: "command"; command: string; index: number; total: number }
  | { type: "output"; command: string; ok: boolean; output: string };

export interface Collected {
  id: string;
  title: string;
  started: number;
  finished: number;
  cancelled: boolean;
  /** [command, {Ok: text} | {Err: message}] */
  outputs: [string, { Ok: string } | { Err: string }][];
}

export const platformProfile = (os: Os | null) => invoke<Profile>("platform_profile", { os });
export const platformsList = () => invoke<PlatformInfo[]>("platforms_list");

export function collectRun(id: string, os: Os, collection: string, onEvent: (e: CollectEvent) => void): Promise<Collected> {
  const events = new Channel<CollectEvent>();
  events.onmessage = onEvent;
  return invoke<Collected>("collect_run", { id, os, collection, events });
}

/** The collected outputs as one text, for saving or handing to the assistant. */
export function collectedText(c: Collected): string {
  return c.outputs.map(([cmd, r]) => `${"=".repeat(8)} ${cmd}\n${"Ok" in r ? r.Ok : `(failed: ${r.Err})`}`).join("\n\n");
}
