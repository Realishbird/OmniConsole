// Built-in model runtime (src-tauri/src/engine.rs): the model catalog, downloads
// and the llama.cpp server the app runs for itself.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Hardware {
  ramGb: number;
  gpuName: string;
  vramGb: number;
  appleSilicon: boolean;
  summary: string;
}

export interface LocalModel {
  id: string;
  name: string;
  repo: string;
  file: string;
  sizeBytes: number;
  needsGb: number;
  blurb: string;
  downloaded: boolean;
  bytesOnDisk: number;
  custom: boolean;
  path: string;
}

export interface LocalStatus {
  runtime: { available: boolean; version: string; note: string };
  hardware: Hardware;
  recommended: string;
  models: LocalModel[];
  engine: { running: boolean; model: string };
  modelsDir: string;
}

export type DownloadEvent =
  | { type: "progress"; id: string; received: number; total: number; bytesPerSec: number }
  | { type: "done"; id: string; path: string }
  | { type: "error"; id: string; message: string }
  | { type: "cancelled"; id: string };

export const localStatus = () => invoke<LocalStatus>("local_status");
export const modelDownload = (id: string) => invoke<string>("model_download", { id });
export const modelCancel = (id: string) => invoke("model_cancel", { id });
export const modelDelete = (id: string) => invoke("model_delete", { id });
export const modelAddCustom = () => invoke<LocalModel | null>("model_add_custom");
export const engineStart = (id: string) => invoke<string>("engine_start", { id });
export const engineStop = () => invoke("engine_stop");

export function onDownload(handler: (e: DownloadEvent) => void): Promise<UnlistenFn> {
  return listen<DownloadEvent>("model-download", (ev) => handler(ev.payload));
}

export function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 10e9 ? 0 : 1)} GB`;
  if (n >= 1e6) return `${Math.round(n / 1e6)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} kB`;
  return `${n} B`;
}

/** Progress state kept across components while a download runs. */
export interface Progress {
  received: number;
  total: number;
  bytesPerSec: number;
}

export const downloads = new Map<string, Progress>();
