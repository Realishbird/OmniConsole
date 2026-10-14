// Quick connect box: "ssh admin@10.0.0.1", "10.0.0.1:2222", "telnet 10.0.0.5 2001",
// "COM3 115200", "serial /dev/ttyUSB0".
import { DEFAULT_PORT, type Protocol } from "./types";

export interface QuickTarget {
  protocol: Protocol;
  host: string;
  port: number;
  username: string;
  serialPort: string;
  baud: number;
}

const SERIAL_NAME = /^(com\d+|\/dev\/\S+)$/i;

export function parseQuick(input: string): QuickTarget | null {
  const words = input.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return null;

  let protocol: Protocol | null = null;
  const first = words[0].toLowerCase();
  if (first === "ssh" || first === "telnet" || first === "serial") {
    protocol = first;
    words.shift();
  }
  if (words.length === 0) return null;

  if (protocol === "serial" || (protocol === null && SERIAL_NAME.test(words[0]))) {
    const baud = words[1] ? parseInt(words[1], 10) : 9600;
    return {
      protocol: "serial",
      host: "",
      port: 0,
      username: "",
      serialPort: /^com\d+$/i.test(words[0]) ? words[0].toUpperCase() : words[0],
      baud: Number.isFinite(baud) && baud > 0 ? baud : 9600,
    };
  }

  protocol ??= "ssh";
  let target = words[0];
  let username = "";
  const at = target.lastIndexOf("@");
  if (at > 0) {
    username = target.slice(0, at);
    target = target.slice(at + 1);
  }
  let host = target;
  let port = DEFAULT_PORT[protocol];
  // [v6addr]:port, host:port (a bare IPv6 address has several colons)
  const v6 = target.match(/^\[([^\]]+)\](?::(\d+))?$/);
  if (v6) {
    host = v6[1];
    if (v6[2]) port = parseInt(v6[2], 10);
  } else if ((target.match(/:/g) ?? []).length === 1) {
    const [h, p] = target.split(":");
    host = h;
    if (p) port = parseInt(p, 10);
  }
  if (words[1] && /^\d+$/.test(words[1])) port = parseInt(words[1], 10);
  if (!host || !Number.isFinite(port) || port <= 0 || port > 65535) return null;
  return { protocol, host, port, username, serialPort: "", baud: 9600 };
}
