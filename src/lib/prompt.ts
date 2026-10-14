// Reads the device prompt from the line the cursor is on: host name and
// privilege level (user EXEC ">", privileged EXEC "#", configuration mode
// "(config...)#"), plus the command typed after it.

export type Privilege = "user" | "privileged" | "config" | "shell" | "unknown";

export interface PromptInfo {
  hostname: string;
  privilege: Privilege;
  mode: string; // "config-if" etc.
  command: string;
}

// sw1#, sw1>, sw1(config-if)#, RP/0/RSP0/CPU0:asr9k#, switch(config)# , user@host:~$
const PROMPT = /^([A-Za-z0-9][\w.\-/:@~]{0,62}?)(?:\(([\w\-.]+)\))?([#>$])\s?(.*)$/;

export function parsePrompt(line: string): PromptInfo | null {
  const m = line.trimEnd().match(PROMPT);
  if (!m) return null;
  const [, host, mode = "", sign, rest] = m;
  // "Password:" and "--More--" never match; avoid sentences like "Note>"
  if (/\s/.test(host)) return null;
  let privilege: Privilege = "unknown";
  if (sign === "#") privilege = mode.startsWith("config") ? "config" : "privileged";
  else if (sign === ">") privilege = "user";
  else if (sign === "$") privilege = "shell";
  const hostname = host.includes("@") ? host.split("@")[1].split(":")[0] : host;
  return { hostname, privilege, mode, command: rest.trim() };
}

export const PRIVILEGE_LABEL: Record<Privilege, string> = {
  user: "User EXEC",
  privileged: "Privileged",
  config: "Config mode",
  shell: "Shell",
  unknown: "",
};
