// Installation and cutover checklists with recorded results, saved per device.

export type ItemState = "open" | "done" | "skipped" | "failed";

export interface ChecklistItem {
  text: string;
  state: ItemState;
  note: string;
  time: number | null;
}

export interface Checklist {
  id: string;
  kind: string;
  title: string;
  device: string;
  site: string;
  created: number;
  items: ChecklistItem[];
}

export const CHECKLIST_TEMPLATES: { kind: string; title: string; items: string[] }[] = [
  {
    kind: "install",
    title: "Switch installation",
    items: [
      "Rack position, power and labels match the design",
      "Both power supplies connected (where fitted) and PSU status OK",
      "Console access confirmed",
      "Firmware version matches the standard",
      "Initial setup applied: hostname, management IP, gateway, DNS, NTP",
      "SSH login works with AAA / local account; Telnet disabled",
      "Logging and SNMP reach the servers",
      "Uplinks connected, trunks up, allowed VLANs correct",
      "Spanning tree root as designed, no blocked ports where none expected",
      "Port configuration applied from templates",
      "PoE budget sufficient for connected devices",
      "Health check passed and saved as baseline",
      "Running config backed up and saved to startup",
      "Documentation and inventory updated (serial, location, ports)",
    ],
  },
  {
    kind: "cutover",
    title: "Cutover",
    items: [
      "Change window confirmed and stakeholders informed",
      "Rollback plan written and backups of old and new config taken",
      "Pre-change health check saved as baseline (old device)",
      "Patching plan: every port mapped old → new",
      "New device pre-staged: config loaded, uplinks tested",
      "Move uplinks; trunks and routing neighbors up",
      "Move access ports; verify link, VLAN, PoE for each group",
      "Spot-check users: DHCP, DNS, internet, key applications",
      "Post-change health check compared with the baseline",
      "Monitoring shows the new device, alarms cleared",
      "Configuration saved; backup taken of the final state",
      "Old device decommissioned or kept powered for the rollback window",
    ],
  },
  {
    kind: "upgrade",
    title: "Firmware upgrade",
    items: [
      "Release notes and compatibility matrix read for this model",
      "Image checksum verified against Cisco's MD5",
      "Enough free flash on every stack member",
      "Config backed up; baseline health check saved",
      "Maintenance window agreed; console access available",
      "Boot variable set and confirmed with show boot",
      "Reload done; device back with the new version",
      "Post-upgrade health check compared with the baseline",
      "Interfaces, neighbors and routing as before",
      "Old image removed once the new one is confirmed",
    ],
  },
];

export function newChecklist(kind: string, device: string, site: string): Checklist {
  const t = CHECKLIST_TEMPLATES.find((x) => x.kind === kind) ?? CHECKLIST_TEMPLATES[0];
  return { id: crypto.randomUUID(), kind: t.kind, title: t.title, device, site, created: Math.floor(Date.now() / 1000), items: t.items.map((text) => ({ text, state: "open", note: "", time: null })) };
}

export function checklistText(c: Checklist): string {
  const mark: Record<ItemState, string> = { open: "[ ]", done: "[x]", skipped: "[-]", failed: "[!]" };
  const lines = [`${c.title} — ${c.device}${c.site ? ` (${c.site})` : ""}`, `Created ${new Date(c.created * 1000).toLocaleString()}`, ""];
  for (const it of c.items) {
    lines.push(`${mark[it.state]} ${it.text}${it.time ? `  (${new Date(it.time * 1000).toLocaleString()})` : ""}${it.note ? `\n      ${it.note}` : ""}`);
  }
  const done = c.items.filter((i) => i.state === "done").length;
  lines.push("", `${done} of ${c.items.length} done, ${c.items.filter((i) => i.state === "failed").length} failed, ${c.items.filter((i) => i.state === "skipped").length} skipped`);
  return lines.join("\n");
}
