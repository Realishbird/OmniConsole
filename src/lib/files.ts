// File transfer (SFTP or SCP over a second SSH connection), external RDP/VNC
// clients, extra windows and `?` completion.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { ConnectSpec, SessionEvent } from "./types";

export interface Opened {
  id: string;
  sftp: boolean;
  home: string;
  user: string;
}

export interface Entry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: number | null;
  permissions: string;
}

export type Transfer = { type: "progress"; done: number; total: number } | { type: "done"; path: string; bytes: number };

export function filesOpen(id: string, spec: ConnectSpec, onEvent: (e: SessionEvent) => void): Promise<Opened> {
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  return invoke<Opened>("files_open", { id, spec, events });
}
export const filesClose = (id: string) => invoke("files_close", { id });
export const filesList = (id: string, path: string) => invoke<Entry[]>("files_list", { id, path });
export const filesMkdir = (id: string, path: string) => invoke("files_mkdir", { id, path });
export const filesDelete = (id: string, path: string, isDir: boolean) => invoke("files_delete", { id, path, isDir });
export const filesRename = (id: string, from: string, to: string) => invoke("files_rename", { id, from, to });

function progress(onProgress: (t: Transfer) => void) {
  const events = new Channel<Transfer>();
  events.onmessage = onProgress;
  return events;
}
export const filesDownload = (id: string, remote: string, onProgress: (t: Transfer) => void) =>
  invoke<string | null>("files_download", { id, remote, events: progress(onProgress) });
export const filesUpload = (id: string, remoteDir: string, onProgress: (t: Transfer) => void) =>
  invoke<string | null>("files_upload", { id, remoteDir, events: progress(onProgress) });

export const launchExternal = (protocol: string, host: string, port: number, username: string) =>
  invoke<string>("launch_external", { protocol, host, port, username });
export const openWindow = (query: string) => invoke<string>("open_window", { query });

export interface Completion {
  word: string;
  help: string;
}
export const complete = (id: string, partial: string) => invoke<Completion[]>("complete", { id, partial });

export function fileSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** The parent of a remote path: "/a/b" → "/a", "flash:/x" → "flash:/", "/" stays. */
export function parentPath(p: string): string {
  const t = p.replace(/\/+$/, "");
  const i = t.lastIndexOf("/");
  if (i < 0) return t.endsWith(":") ? t : "/";
  if (i === 0) return "/";
  const head = t.slice(0, i);
  return head.endsWith(":") ? `${head}/` : head;
}
