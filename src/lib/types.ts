export type Protocol = "ssh" | "telnet" | "serial";
export type AuthMethod = "password" | "key" | "agent";
export type Environment = "" | "production" | "staging" | "lab" | "customer";
export type Parity = "none" | "odd" | "even";
export type FlowControl = "none" | "software" | "hardware";

export interface Device {
  id: string;
  name: string;
  protocol: Protocol;
  host: string;
  port: number;
  username: string;

  customer: string;
  site: string;
  building: string;
  rack: string;
  role: string;
  tags: string[];
  environment: Environment;
  color: string;
  favorite: boolean;
  lastConnected?: number | null;
  notes: string;

  authMethod: AuthMethod;
  keyPath: string;
  jumpDeviceId: string;
  keepalive: number;

  serialPort: string;
  baud: number;
  dataBits: number;
  parity: Parity;
  stopBits: number;
  flowControl: FlowControl;

  autoReconnect: boolean;
  logSession: boolean;
}

export interface JumpSpec {
  host: string;
  port: number;
  username: string;
  deviceId?: string;
  authMethod: AuthMethod;
  keyPath: string;
}

export interface ConnectSpec {
  protocol: Protocol;
  label: string;
  host: string;
  port: number;
  username: string;
  deviceId?: string;
  password?: string;
  authMethod: AuthMethod;
  keyPath: string;
  jump?: JumpSpec | null;
  keepalive: number;
  serialPort: string;
  baud: number;
  dataBits: number;
  parity: Parity;
  stopBits: number;
  flowControl: FlowControl;
  log: boolean;
  logTimestamps: boolean;
  cols: number;
  rows: number;
}

export type PromptKind = "password" | "username" | "passphrase" | "hostKeyNew" | "hostKeyChanged" | "keyboard";

export type SessionEvent =
  | { type: "status"; state: "connecting" | "connected" | "closed" | "error"; message: string }
  | { type: "prompt"; kind: PromptKind; message: string; fingerprint?: string; echo: boolean }
  | { type: "log"; path: string };

export interface SerialPortInfo {
  name: string;
  description: string;
}

export interface Snippet {
  id: string;
  name: string;
  body: string;
  description: string;
}

export interface HistoryEntry {
  command: string;
  device: string;
  at: number;
}

export const DEFAULT_PORT: Record<Protocol, number> = { ssh: 22, telnet: 23, serial: 0 };

export const ENVIRONMENTS: { value: Environment; label: string; short: string; color: string }[] = [
  { value: "", label: "Not set", short: "", color: "" },
  { value: "production", label: "Production", short: "PROD", color: "#e5534b" },
  { value: "staging", label: "Staging", short: "STAGE", color: "#d4a72c" },
  { value: "lab", label: "Lab", short: "LAB", color: "#57ab5a" },
  { value: "customer", label: "Customer", short: "CUST", color: "#6cb6ff" },
];

export function envInfo(env: string) {
  return ENVIRONMENTS.find((e) => e.value === env) ?? ENVIRONMENTS[0];
}

export function newDevice(protocol: Protocol = "ssh", site = ""): Device {
  return {
    id: crypto.randomUUID(),
    name: "",
    protocol,
    host: "",
    port: DEFAULT_PORT[protocol],
    username: "",
    customer: "",
    site,
    building: "",
    rack: "",
    role: "",
    tags: [],
    environment: "",
    color: "",
    favorite: false,
    lastConnected: null,
    notes: "",
    authMethod: "password",
    keyPath: "",
    jumpDeviceId: "",
    keepalive: 30,
    serialPort: "",
    baud: 9600,
    dataBits: 8,
    parity: "none",
    stopBits: 1,
    flowControl: "none",
    autoReconnect: false,
    logSession: false,
  };
}

/** "9600 8N1" */
export function serialLabel(d: Pick<Device, "baud" | "dataBits" | "parity" | "stopBits">): string {
  return `${d.baud} ${d.dataBits}${d.parity[0].toUpperCase()}${d.stopBits}`;
}

export function deviceTarget(d: Pick<Device, "protocol" | "host" | "port" | "serialPort" | "baud" | "dataBits" | "parity" | "stopBits">): string {
  if (d.protocol === "serial") return `${d.serialPort} ${serialLabel(d)}`;
  const port = d.port && d.port !== DEFAULT_PORT[d.protocol] ? `:${d.port}` : "";
  return `${d.host}${port}`;
}
