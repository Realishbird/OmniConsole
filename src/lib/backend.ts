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
export const resizeSession = (id: string, cols: number, rows: number) =>
  invoke("session_resize", { id, cols, rows });
export const closeSession = (id: string) => invoke("session_close", { id });
export const answerSession = (id: string, answer: string | null) => invoke("session_answer", { id, answer });

export const listSerialPorts = () => invoke<SerialPortInfo[]>("serial_ports");
export const loadDevices = () => invoke<Device[]>("devices_load");
export const saveDevices = (devices: Device[]) => invoke("devices_save", { devices });
export const setPassword = (deviceId: string, password: string) => invoke("password_set", { deviceId, password });
export const deletePassword = (deviceId: string) => invoke("password_delete", { deviceId });
export const passwordExists = (deviceId: string) => invoke<boolean>("password_exists", { deviceId });
export const saveTextFile = (path: string, contents: string) => invoke("save_text_file", { path, contents });
