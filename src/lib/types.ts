export type Protocol = "ssh" | "telnet" | "serial";

export interface Device {
  id: string;
  name: string;
  site: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  serialPort: string;
  baud: number;
  notes: string;
}

export interface ConnectSpec {
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  deviceId?: string;
  password?: string;
  serialPort: string;
  baud: number;
  cols: number;
  rows: number;
}

export type PromptKind = "password" | "username" | "hostKeyNew" | "hostKeyChanged" | "keyboard";

export type SessionEvent =
  | { type: "status"; state: "connecting" | "connected" | "closed" | "error"; message: string }
  | { type: "prompt"; kind: PromptKind; message: string; fingerprint?: string; echo: boolean };

export interface SerialPortInfo {
  name: string;
  description: string;
}

export const DEFAULT_PORT: Record<Protocol, number> = { ssh: 22, telnet: 23, serial: 0 };

export function newDevice(protocol: Protocol = "ssh", site = ""): Device {
  return {
    id: crypto.randomUUID(),
    name: "",
    site,
    protocol,
    host: "",
    port: DEFAULT_PORT[protocol],
    username: "",
    serialPort: "",
    baud: 9600,
    notes: "",
  };
}

export function deviceTarget(d: Pick<Device, "protocol" | "host" | "port" | "serialPort" | "baud">): string {
  if (d.protocol === "serial") return `${d.serialPort} ${d.baud}`;
  const port = d.port && d.port !== DEFAULT_PORT[d.protocol] ? `:${d.port}` : "";
  return `${d.host}${port}`;
}
