// Guided troubleshooting: ordered read-only steps per topic, with what to
// look for in each. `{if}`, `{vlan}`, `{ip}`, `{mac}` are filled in by the
// user before a step runs.
import type { Os } from "./tools";

export interface Step {
  title: string;
  /** IOS command; nxos overrides when NX-OS differs */
  command: string;
  nxos?: string;
  asa?: string;
  lookFor: string;
}

export interface Workflow {
  id: string;
  title: string;
  description: string;
  inputs: ("if" | "vlan" | "ip" | "mac" | "user")[];
  steps: Step[];
}

export const WORKFLOWS: Workflow[] = [
  {
    id: "mac-trace", title: "Trace a device to its port", description: "From an IP or MAC address to the switch port it sits on, and the neighbor behind that port if it is an uplink.", inputs: ["ip", "mac"],
    steps: [
      { title: "Find the MAC for the IP", command: "show ip arp {ip}", nxos: "show ip arp {ip}", asa: "show arp | include {ip}", lookFor: "The hardware address. If the entry is Incomplete the host is not answering ARP from this device." },
      { title: "Find the port for the MAC", command: "show mac address-table address {mac}", nxos: "show mac address-table address {mac}", lookFor: "The port column. A port-channel or an uplink means the host is behind another switch." },
      { title: "Who is behind that port", command: "show cdp neighbors {if} detail", nxos: "show cdp neighbors interface {if} detail", lookFor: "A neighbor means hop to that switch and repeat. No neighbor means the host is directly connected (or the neighbor does not speak CDP; try LLDP)." },
      { title: "LLDP on that port", command: "show lldp neighbors {if} detail", nxos: "show lldp neighbors interface {if} detail", lookFor: "System name and management address of a non-Cisco neighbor." },
      { title: "Port state and errors", command: "show interfaces {if}", nxos: "show interface {if}", lookFor: "Up/up, speed and duplex, input errors, CRC, last input/output times." },
    ],
  },
  {
    id: "dhcp", title: "DHCP", description: "A client is not getting an address.", inputs: ["vlan", "if"],
    steps: [
      { title: "Helper address on the client VLAN", command: "show running-config interface Vlan{vlan}", nxos: "show running-config interface Vlan{vlan}", lookFor: "ip helper-address (IOS) or ip dhcp relay address (NX-OS) pointing at the right server." },
      { title: "DHCP snooping", command: "show ip dhcp snooping", nxos: "show ip dhcp snooping", lookFor: "If snooping is on, the port towards the server must be trusted; untrusted uplinks drop offers." },
      { title: "Snooping bindings", command: "show ip dhcp snooping binding", nxos: "show ip dhcp snooping binding", lookFor: "Leases seen on the client port." },
      { title: "Local pool (if this device serves DHCP)", command: "show ip dhcp pool", lookFor: "Leased addresses vs the pool size, exclusions." },
      { title: "Conflicts", command: "show ip dhcp conflict", lookFor: "Addresses declined by clients, a sign of a rogue server or static overlap." },
      { title: "Client port", command: "show interfaces {if} switchport", nxos: "show interface {if} switchport", lookFor: "Access VLAN matches the intended VLAN; port is not a trunk by accident." },
      { title: "Relay and server counters", command: "show ip dhcp server statistics", lookFor: "Discover/offer/request/ack counts moving." },
    ],
  },
  {
    id: "dns", title: "DNS", description: "Names do not resolve from the device or its clients.", inputs: [],
    steps: [
      { title: "Name servers configured", command: "show running-config | include name-server|domain", nxos: "show running-config | include name-server|domain", asa: "show running-config dns", lookFor: "ip name-server entries and ip domain lookup not disabled." },
      { title: "Cached hosts", command: "show hosts", nxos: "show hosts", lookFor: "Entries with a (temp) flag resolve; none means lookups are failing." },
      { title: "Reach the server", command: "ping {ip}", nxos: "ping {ip}", lookFor: "Replies from the DNS server; no replies points at routing or an ACL." },
      { title: "Test a lookup", command: "ping www.cisco.com", nxos: "ping www.cisco.com", lookFor: "Translating … means the lookup ran; an unknown host means the server did not answer." },
    ],
  },
  {
    id: "vlan", title: "VLAN", description: "A host in a VLAN cannot talk to others in the same VLAN.", inputs: ["vlan", "if"],
    steps: [
      { title: "VLAN exists and is active", command: "show vlan id {vlan}", nxos: "show vlan id {vlan}", lookFor: "Status active and the expected ports listed." },
      { title: "Port VLAN membership", command: "show interfaces {if} switchport", nxos: "show interface {if} switchport", lookFor: "Operational mode and access VLAN; a trunk with the VLAN pruned looks the same as a wrong access VLAN." },
      { title: "MAC addresses learned in the VLAN", command: "show mac address-table vlan {vlan}", nxos: "show mac address-table vlan {vlan}", lookFor: "Both hosts' MACs. A missing MAC means that host's frames never arrive." },
      { title: "Spanning tree for the VLAN", command: "show spanning-tree vlan {vlan}", nxos: "show spanning-tree vlan {vlan}", lookFor: "The port is FWD, not BLK or BKN." },
      { title: "Port state", command: "show interfaces {if} status", nxos: "show interface {if} status", lookFor: "connected, right VLAN, no err-disabled." },
    ],
  },
  {
    id: "trunk", title: "Trunking", description: "VLANs do not pass between switches.", inputs: ["if", "vlan"],
    steps: [
      { title: "Trunk status", command: "show interfaces trunk", nxos: "show interface trunk", lookFor: "The uplink is trunking, the VLAN is allowed and active, native VLANs match on both ends." },
      { title: "Switchport details", command: "show interfaces {if} switchport", nxos: "show interface {if} switchport", lookFor: "Administrative vs operational mode, native VLAN, pruning." },
      { title: "Native VLAN mismatch messages", command: "show logging | include NATIVE|CDP-4", lookFor: "CDP-4-NATIVE_VLAN_MISMATCH points at the other end." },
      { title: "Spanning tree on the trunk", command: "show spanning-tree interface {if}", nxos: "show spanning-tree interface {if}", lookFor: "FWD for the VLAN in question; BLK means another path is preferred." },
      { title: "VLAN known on this switch", command: "show vlan brief | include {vlan}", nxos: "show vlan brief | include {vlan}", lookFor: "The VLAN exists here; a trunk cannot carry a VLAN the switch does not know." },
    ],
  },
  {
    id: "stp", title: "Spanning tree", description: "Loops, flapping or slow convergence.", inputs: ["if"],
    steps: [
      { title: "Summary", command: "show spanning-tree summary", nxos: "show spanning-tree summary", lookFor: "Mode (rapid-pvst is expected), root for which VLANs, BPDU guard and loop guard state." },
      { title: "Root bridges", command: "show spanning-tree root", nxos: "show spanning-tree root", lookFor: "Root is the intended core, same for every VLAN. Priority 32768 everywhere means nobody set one." },
      { title: "Inconsistent ports", command: "show spanning-tree inconsistentports", nxos: "show spanning-tree inconsistentports", lookFor: "Any port listed is blocked for a reason: root guard, loop guard, PVID mismatch." },
      { title: "Recent topology changes", command: "show spanning-tree detail | include changes|occurred|from", nxos: "show spanning-tree detail | include changes|occurred|from", lookFor: "Where the last change came from; follow that port to the flapping device." },
      { title: "A specific port", command: "show spanning-tree interface {if} detail", nxos: "show spanning-tree interface {if} detail", lookFor: "Port type (edge expected on access ports), guards, BPDUs sent/received." },
    ],
  },
  {
    id: "routing", title: "Routing", description: "A destination is unreachable or takes a wrong path.", inputs: ["ip"],
    steps: [
      { title: "Route to the destination", command: "show ip route {ip}", nxos: "show ip route {ip}", asa: "show route {ip}", lookFor: "The matching prefix, next hop and protocol. No match plus no default means a black hole." },
      { title: "CEF path", command: "show ip cef {ip} detail", lookFor: "The adjacency actually used for forwarding." },
      { title: "Protocols and neighbors", command: "show ip protocols", nxos: "show ip ospf neighbors", lookFor: "Timers, networks advertised, neighbors in FULL/Established." },
      { title: "OSPF neighbors", command: "show ip ospf neighbor", nxos: "show ip ospf neighbors", lookFor: "FULL (or 2WAY on broadcast non-DR). Stuck in EXSTART is an MTU mismatch." },
      { title: "BGP summary", command: "show ip bgp summary", nxos: "show ip bgp summary", lookFor: "Up/Down time and prefix count; Active or Idle means the session is not established." },
      { title: "ARP for the next hop", command: "show ip arp {ip}", nxos: "show ip arp {ip}", lookFor: "A resolved next hop; Incomplete means layer 2 is the problem." },
      { title: "Trace from the device", command: "traceroute {ip} numeric", nxos: "traceroute {ip}", lookFor: "Where the path stops or loops." },
    ],
  },
  {
    id: "acl", title: "ACL", description: "Traffic is dropped by an access list.", inputs: ["if"],
    steps: [
      { title: "ACLs applied on the interface", command: "show ip interface {if} | include access list", nxos: "show running-config interface {if}", asa: "show running-config access-group", lookFor: "Inbound and outbound ACL names." },
      { title: "ACL contents and hit counts", command: "show ip access-lists", nxos: "show ip access-lists", asa: "show access-list", lookFor: "Matches on a deny line growing while you test; the implicit deny at the end catches everything not permitted." },
      { title: "Log messages", command: "show logging | include SEC-6-IPACCESSLOG|ACL", nxos: "show logging last 50", lookFor: "Logged denies with source and destination." },
      { title: "Object groups (if used)", command: "show object-group", asa: "show running-config object-group", lookFor: "The host or network actually in the group." },
    ],
  },
  {
    id: "wireless", title: "Wireless", description: "Access points or clients on a Catalyst 9800 or embedded controller.", inputs: ["mac"],
    steps: [
      { title: "APs joined", command: "show ap summary", lookFor: "Every AP Registered with the expected IP and location." },
      { title: "AP join problems", command: "show ap join stats summary", lookFor: "Discovery and join failures per AP." },
      { title: "WLANs", command: "show wlan summary", lookFor: "WLANs enabled and mapped to the right policy profile." },
      { title: "A client", command: "show wireless client mac-address {mac} detail", lookFor: "Policy manager state RUN; stuck in authentication or DHCP tells where it fails." },
      { title: "Client count and RF", command: "show wireless summary", lookFor: "Client totals and channel utilisation." },
      { title: "Switch side: PoE for APs", command: "show power inline", lookFor: "APs drawing power; off or faulty ports." },
    ],
  },
  {
    id: "auth", title: "Authentication (AAA, 802.1X)", description: "Users cannot log in or ports fail authentication.", inputs: ["if", "user"],
    steps: [
      { title: "AAA servers", command: "show aaa servers", nxos: "show tacacs-server", asa: "show aaa-server", lookFor: "Servers UP with recent successful transactions; DEAD means the switch gave up on it." },
      { title: "Server groups and method lists", command: "show running-config | include ^aaa|tacacs|radius", nxos: "show running-config aaa", lookFor: "Login method order; local fallback present." },
      { title: "802.1X sessions on a port", command: "show authentication sessions interface {if} details", lookFor: "Status Authorized, method dot1x or mab, the VLAN assigned." },
      { title: "dot1x statistics", command: "show dot1x interface {if} statistics", lookFor: "EAPOL frames both ways; none received means the supplicant is silent." },
      { title: "Test the server", command: "test aaa group tacacs+ {user} password new-code", lookFor: "User successfully authenticated, or the exact failure reason." },
      { title: "Recent auth messages", command: "show logging | include AUTHMGR|DOT1X|MAB|RADIUS|TACACS", lookFor: "Server timeouts, rejects and the reason text." },
    ],
  },
];

export const INPUT_LABEL = { if: "Interface", vlan: "VLAN", ip: "IP address", mac: "MAC address", user: "User name" } as const;

/** Normalize a MAC to Cisco's hhhh.hhhh.hhhh. */
export function cisco_mac(input: string): string {
  const hex = input.toLowerCase().replace(/[^0-9a-f]/g, "");
  if (hex.length !== 12) return input.trim();
  return `${hex.slice(0, 4)}.${hex.slice(4, 8)}.${hex.slice(8, 12)}`;
}

export function commandFor(step: Step, os: Os | null, values: Record<string, string>): string | null {
  let cmd = os === "nx-os" ? step.nxos ?? step.command : os === "asa" ? step.asa ?? step.command : step.command;
  if (os === "asa" && !step.asa && /^show (ip |mac|spanning|vlan|interfaces)/.test(step.command)) return null;
  for (const [k, v] of Object.entries(values)) {
    const val = k === "mac" ? cisco_mac(v) : v.trim();
    cmd = cmd.replaceAll(`{${k}}`, val);
  }
  return /\{\w+\}/.test(cmd) ? null : cmd;
}
