// Workstation tools (run on this computer, src-tauri/src/nettools.rs) and
// the subnet calculator, which needs no backend.
import { Channel, invoke } from "@tauri-apps/api/core";

export type Line = { type: "line"; text: string } | { type: "done"; ok: boolean; summary: string };

function streaming<T>(cmd: string, args: Record<string, unknown>, onLine: (l: Line) => void): Promise<T> {
  const events = new Channel<Line>();
  events.onmessage = onLine;
  return invoke<T>(cmd, { ...args, events });
}

export const netPing = (id: string, target: string, count: number, size: number | null, onLine: (l: Line) => void) =>
  streaming<string>("net_ping", { id, target, count, size }, onLine);
export const netTraceroute = (id: string, target: string, maxHops: number, onLine: (l: Line) => void) =>
  streaming<string>("net_traceroute", { id, target, maxHops }, onLine);
export const netMtu = (id: string, target: string, onLine: (l: Line) => void) => streaming<number>("net_mtu", { id, target }, onLine);
export const netCancel = (id: string) => invoke("net_cancel", { id });

export interface DnsResult {
  query: string;
  addresses: string[];
  names: string[];
  elapsedMs: number;
  detail: string;
}
export const netDns = (target: string, record: string, server: string) => invoke<DnsResult>("net_dns", { target, record, server: server || null });

export interface PortResult {
  port: number;
  open: boolean;
  elapsedMs: number;
  detail: string;
}
export const netPorts = (target: string, ports: number[], timeoutMs?: number) => invoke<PortResult[]>("net_ports", { target, ports, timeoutMs });

/** "22, 80-443, 8080" → ports */
export function parsePorts(text: string): number[] {
  const out = new Set<number>();
  for (const part of text.split(/[\s,]+/).filter(Boolean)) {
    const m = part.match(/^(\d+)(?:-(\d+))?$/);
    if (!m) continue;
    const a = Number(m[1]);
    const b = m[2] ? Number(m[2]) : a;
    for (let p = Math.min(a, b); p <= Math.max(a, b) && out.size < 1024; p++) if (p >= 1 && p <= 65535) out.add(p);
  }
  return [...out].sort((x, y) => x - y);
}

export const COMMON_PORTS = "22, 23, 80, 443, 161, 830, 8443";

/* ------------------------------------------------------ subnet math */

export interface Subnet {
  network: string;
  broadcast: string;
  mask: string;
  wildcard: string;
  prefix: number;
  firstHost: string;
  lastHost: string;
  hosts: number;
  cidr: string;
  /** the input address, when one was given */
  address: string;
  class: string;
  private: boolean;
}

const toNum = (ip: string) => ip.split(".").reduce((n, o) => n * 256 + Number(o), 0) >>> 0;
const toIp = (n: number) => [24, 16, 8, 0].map((s) => (n >>> s) & 255).join(".");
const isIp = (s: string) => /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.test(s) && s.split(".").every((o) => Number(o) <= 255);

/** Accepts 10.1.2.3/24, 10.1.2.3 255.255.255.0, or 10.1.2.3 and a mask. */
export function subnet(input: string): Subnet | null {
  const t = input.trim().replace(/\s+/g, " ");
  let ip = "";
  let prefix = -1;
  const cidr = t.match(/^([\d.]+)\/(\d{1,2})$/);
  const pair = t.match(/^([\d.]+) ([\d.]+)$/);
  if (cidr) {
    ip = cidr[1];
    prefix = Number(cidr[2]);
  } else if (pair) {
    ip = pair[1];
    if (!isIp(pair[2])) return null;
    const m = toNum(pair[2]);
    // a wildcard mask has its zeros first
    const mask = (m & 1) === 1 && m !== 0xffffffff ? ~m >>> 0 : m;
    prefix = 32 - Math.log2((~mask >>> 0) + 1);
    if (!Number.isInteger(prefix)) return null;
  } else if (isIp(t)) {
    ip = t;
    const first = Number(t.split(".")[0]);
    prefix = first < 128 ? 8 : first < 192 ? 16 : 24;
  } else return null;
  if (!isIp(ip) || prefix < 0 || prefix > 32) return null;
  const maskN = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
  const ipN = toNum(ip);
  const net = (ipN & maskN) >>> 0;
  const bcast = (net | (~maskN >>> 0)) >>> 0;
  const hosts = prefix >= 31 ? (prefix === 31 ? 2 : 1) : Math.max(0, bcast - net - 1);
  const first = Number(ip.split(".")[0]);
  const cls = first < 128 ? "A" : first < 192 ? "B" : first < 224 ? "C" : first < 240 ? "D (multicast)" : "E";
  const priv = (first === 10) || (first === 172 && Number(ip.split(".")[1]) >= 16 && Number(ip.split(".")[1]) <= 31) || (first === 192 && Number(ip.split(".")[1]) === 168) || (first === 169 && Number(ip.split(".")[1]) === 254);
  return {
    network: toIp(net),
    broadcast: toIp(bcast),
    mask: toIp(maskN),
    wildcard: toIp(~maskN >>> 0),
    prefix,
    firstHost: prefix >= 31 ? toIp(net) : toIp(net + 1),
    lastHost: prefix >= 31 ? toIp(bcast) : toIp(bcast - 1),
    hosts,
    cidr: `${toIp(net)}/${prefix}`,
    address: ip,
    class: cls,
    private: priv,
  };
}

/** Split a network into equal subnets of the given prefix. */
export function split(s: Subnet, newPrefix: number): string[] {
  if (newPrefix <= s.prefix || newPrefix > 32) return [];
  const count = Math.min(256, 2 ** (newPrefix - s.prefix));
  const size = 2 ** (32 - newPrefix);
  const base = toNum(s.network);
  return Array.from({ length: count }, (_, i) => `${toIp(base + i * size)}/${newPrefix}`);
}
