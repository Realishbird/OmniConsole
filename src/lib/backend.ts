// Thin wrappers around the Rust commands (src-tauri/src/commands.rs).
import { Channel, invoke } from "@tauri-apps/api/core";
import type { ConnectSpec, Device, SerialPortInfo, SessionEvent } from "./types";

export async function openSession(
  spec: ConnectSpec,
  onData: (bytes: Uint8Array) => void,
  onEvent: (event: SessionEvent) => void,
): Promise<string> {
  const data = new Channel<ArrayBuffer>();
  data.onmessage = (buf) => onData(new Uint8Array(buf));
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  return invoke<string>("session_open", { spec, data, events });
}

export const writeSession = (id: string, data: string) => invoke("session_write", { id, data });
export const resizeSession = (id: string, cols: number, rows: number) => invoke("session_resize", { id, cols, rows });
export const breakSession = (id: string) => invoke("session_break", { id });
export const closeSession = (id: string) => invoke("session_close", { id });
export const answerSession = (id: string, answer: string | null) => invoke("session_answer", { id, answer });

export const listSerialPorts = () => invoke<SerialPortInfo[]>("serial_ports");
export const loadDevices = () => invoke<Device[]>("devices_load");
export const saveDevices = (devices: Device[]) => invoke("devices_save", { devices });

export type DataKind = "snippets" | "history" | "settings" | "workspace" | "audit";
export const loadData = <T,>(kind: DataKind) => invoke<T | null>("data_load", { kind });
export const saveData = (kind: DataKind, value: unknown) => invoke("data_save", { kind, value });

export type SecretKind = "password" | "passphrase" | "apikey";
export const setSecret = (kind: SecretKind, deviceId: string, value: string) => invoke("secret_set", { kind, deviceId, value });
export const deleteSecret = (kind: SecretKind, deviceId: string) => invoke("secret_delete", { kind, deviceId });
export const secretExists = (kind: SecretKind, deviceId: string) => invoke<boolean>("secret_exists", { kind, deviceId });

/** Opens a save dialog; resolves to the path, or null when cancelled. */
export const saveText = (name: string, contents: string, maskSecrets: boolean) =>
  invoke<string | null>("save_text", { name, contents, maskSecrets });
export const exportDevices = (devices: Device[]) => invoke<string | null>("devices_export", { devices });
export const importDevices = () => invoke<Device[] | null>("devices_import");
export const logFolder = () => invoke<string>("log_folder");
export const openLogFolder = () => invoke("open_log_folder");
export const revealLog = (path: string) => invoke("reveal_log", { path });
