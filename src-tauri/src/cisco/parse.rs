//! Parsers that turn common `show` output into tables. Each one keeps the
//! raw text untouched; a table is a view beside it, never a replacement.

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn re(s: &str) -> Regex {
    Regex::new(s).expect("parse regex")
}

fn cols(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn pick(text: &str, pattern: &str) -> u64 {
    re(pattern).captures(text).and_then(|c| c[1].replace(',', "").parse().ok()).unwrap_or(0)
}

/* ------------------------------------------------------- show interfaces */

#[derive(Debug, Default, Clone)]
pub struct Interface {
    pub name: String,
    pub status: String,
    pub description: String,
    pub input_errors: u64,
    pub crc: u64,
    pub input_drops: u64,
    pub output_drops: u64,
    pub collisions: u64,
    pub runts: u64,
    pub giants: u64,
}

/// Blocks of `show interfaces` (IOS, IOS-XE and NX-OS share the shape).
pub fn interfaces(text: &str) -> Vec<Interface> {
    let head = re(r"(?m)^(\S+) is ([\w\- ]+?)(?:, line protocol is (\S+))?(?: \(.*\))?\s*$");
    let mut out = Vec::new();
    let mut starts: Vec<(usize, String, String)> = Vec::new();
    for c in head.captures_iter(text) {
        let status = match c.get(3) {
            Some(lp) => format!("{}/{}", c[2].trim(), lp.as_str()),
            None => c[2].trim().to_string(),
        };
        starts.push((c.get(0).unwrap().start(), c[1].to_string(), status));
    }
    for (i, (start, name, status)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map(|s| s.0).unwrap_or(text.len());
        let block = &text[*start..end];
        out.push(Interface {
            name: name.clone(),
            status: status.clone(),
            description: re(r"(?m)^\s*Description: (.+)$").captures(block).map(|c| c[1].trim().to_string()).unwrap_or_default(),
            input_errors: pick(block, r"(\d+) input errors?"),
            crc: pick(block, r"(\d+) CRC"),
            input_drops: pick(block, r"[Ii]nput queue: \d+/\d+/(\d+)/\d+") + pick(block, r"(\d+) input discard"),
            output_drops: pick(block, r"Total output drops: (\d+)") + pick(block, r"(\d+) output discard"),
            collisions: pick(block, r"(\d+) collisions") + pick(block, r"(\d+) late collision"),
            runts: pick(block, r"(\d+) runts"),
            giants: pick(block, r"(\d+) giants"),
        });
    }
    out
}

/* ------------------------------------------------ show interfaces status */

/// Port  Name  Status  Vlan  Duplex  Speed  Type
pub fn interface_status(text: &str) -> Table {
    let mut table = Table { title: "Interface status".into(), columns: cols(&["Port", "Name", "Status", "Duplex", "Speed", "VLAN", "Type"]), rows: Vec::new() };
    // the Name column may be empty or contain spaces, so anchor on the status word
    let row = re(r"(?m)^(\S+)\s+(.*?)\s*(connected|notconnect|notconnec|disabled|err-disabled|errDisable|sfpAbsent|xcvrAbsen|noOperMem|inactive|monitoring|linkFlapE|down|up|suspended|channelDo)\s+(\S+)\s+(\S+)\s+(\S+)\s*(.*)$");
    for c in row.captures_iter(text) {
        if &c[1] == "Port" {
            continue;
        }
        table.rows.push(vec![c[1].into(), c[2].trim().into(), c[3].into(), c[5].into(), c[6].into(), c[4].into(), c[7].trim().into()]);
    }
    table
}

/* ------------------------------------------------------ show power inline */

pub fn power_inline(text: &str) -> Table {
    let mut table = Table { title: "PoE ports".into(), columns: cols(&["Port", "Admin", "Status", "Power (W)", "Device", "Class", "Max (W)"]), rows: Vec::new() };
    let row = re(r"(?m)^(\S+/\d+)\s+(auto|static|never)\s+(\S+)\s+([\d.]+)\s+(.*?)\s+(\S+)\s+([\d.]+)\s*$");
    for c in row.captures_iter(text) {
        table.rows.push(vec![c[1].into(), c[2].into(), c[3].into(), c[4].into(), c[5].trim().into(), c[6].into(), c[7].into()]);
    }
    table
}

/* -------------------------------------------------- show interfaces trunk */

#[derive(Debug, Default, Clone)]
pub struct Trunk {
    pub port: String,
    pub mode: String,
    pub encap: String,
    pub native: String,
    pub allowed: String,
}

pub fn trunks(text: &str) -> Vec<Trunk> {
    let mut out: Vec<Trunk> = Vec::new();
    let mut section = 0;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("Port") && l.contains("Mode") {
            section = 1;
            continue;
        }
        if l.starts_with("Port") && l.contains("allowed on trunk") {
            section = 2;
            continue;
        }
        if l.starts_with("Port") {
            section = 3;
            continue;
        }
        if l.is_empty() {
            continue;
        }
        let f: Vec<&str> = l.split_whitespace().collect();
        match section {
            1 if f.len() >= 5 => out.push(Trunk { port: f[0].into(), mode: f[1].into(), encap: f[2].into(), native: f[4].into(), allowed: String::new() }),
            2 if f.len() >= 2 => {
                if let Some(t) = out.iter_mut().find(|t| t.port == f[0]) {
                    t.allowed = f[1..].join(" ");
                }
            }
            _ => {}
        }
    }
    out
}

/* --------------------------------------------------- show cdp neighbors */

#[derive(Debug, Default, Clone)]
pub struct Neighbor {
    pub local: String,
    pub device: String,
    pub remote: String,
    pub platform: String,
    pub ip: String,
}

pub fn cdp_neighbors(text: &str) -> Vec<Neighbor> {
    let mut out = Vec::new();
    for block in text.split("-------------------------") {
        let device = re(r"Device ID: ?(\S+)").captures(block).map(|c| c[1].to_string());
        let Some(device) = device else { continue };
        out.push(Neighbor {
            local: re(r"Interface: ([^,\s]+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
            device,
            remote: re(r"Port ID \(outgoing port\): (\S+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
            platform: re(r"Platform: ([^,]+),").captures(block).map(|c| c[1].trim().to_string()).unwrap_or_default(),
            ip: re(r"IP(?:v4)? [Aa]ddress: (\S+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
        });
    }
    out
}

pub fn lldp_neighbors(text: &str) -> Vec<Neighbor> {
    let mut out = Vec::new();
    for block in text.split("------------------------------------------------") {
        let device = re(r"System Name: ?(\S+)").captures(block).map(|c| c[1].to_string());
        let Some(device) = device else { continue };
        out.push(Neighbor {
            local: re(r"Local Intf: (\S+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
            device,
            remote: re(r"Port id: (\S+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
            platform: re(r"(?m)^System Description: ?\n?\s*(.+)$").captures(block).map(|c| c[1].trim().chars().take(60).collect()).unwrap_or_default(),
            ip: re(r"IP: (\S+)").captures(block).map(|c| c[1].to_string()).unwrap_or_default(),
        });
    }
    out
}

/* ---------------------------------------------- show ip interface brief */

pub fn ip_interface_brief(text: &str) -> Table {
    let mut table = Table { title: "IP interfaces".into(), columns: cols(&["Interface", "IP address", "Status", "Protocol"]), rows: Vec::new() };
    let row = re(r"(?m)^(\S+)\s+(\S+)\s+(?:YES|NO)\s+\S+\s+(administratively down|up|down|deleted)\s+(\S+)\s*$");
    for c in row.captures_iter(text) {
        table.rows.push(vec![c[1].into(), c[2].into(), c[3].into(), c[4].into()]);
    }
    if table.rows.is_empty() {
        // NX-OS: Interface  IP Address  Interface Status
        let nx = re(r"(?m)^(\S+)\s+(\d+\.\d+\.\d+\.\d+)\s+protocol-(\S+)/link-(\S+)");
        for c in nx.captures_iter(text) {
            table.rows.push(vec![c[1].into(), c[2].into(), c[4].into(), c[3].into()]);
        }
    }
    table
}

/* ------------------------------------------------ show mac address-table */

pub fn mac_table(text: &str) -> Table {
    let mut table = Table { title: "MAC address table".into(), columns: cols(&["VLAN", "MAC address", "Type", "Port"]), rows: Vec::new() };
    let row = re(r"(?m)^\s*\*?\s*(\d+|All)\s+([0-9a-fA-F]{4}\.[0-9a-fA-F]{4}\.[0-9a-fA-F]{4})\s+(\S+)\s+(?:\S+\s+)*?(\S+)\s*$");
    for c in row.captures_iter(text) {
        table.rows.push(vec![c[1].into(), c[2].to_lowercase(), c[3].to_lowercase(), c[4].into()]);
    }
    table
}

/* --------------------------------------------------------- show ip arp */

pub fn arp_table(text: &str) -> Table {
    let mut table = Table { title: "ARP table".into(), columns: cols(&["IP address", "Age (min)", "MAC address", "Interface"]), rows: Vec::new() };
    let row = re(r"(?m)^Internet\s+(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+([0-9a-fA-F]{4}\.[0-9a-fA-F]{4}\.[0-9a-fA-F]{4}|Incomplete)\s+\S+\s*(\S*)\s*$");
    for c in row.captures_iter(text) {
        table.rows.push(vec![c[1].into(), c[2].into(), c[3].to_lowercase(), c[4].into()]);
    }
    if table.rows.is_empty() {
        // NX-OS: Address  Age  MAC Address  Interface
        let nx = re(r"(?m)^(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+([0-9a-fA-F]{4}\.[0-9a-fA-F]{4}\.[0-9a-fA-F]{4})\s+(\S+)");
        for c in nx.captures_iter(text) {
            table.rows.push(vec![c[1].into(), c[2].into(), c[3].to_lowercase(), c[4].into()]);
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_interfaces_blocks() {
        let text = "GigabitEthernet1/0/1 is up, line protocol is up (connected)\n  Hardware is Gigabit Ethernet, address is 0000.0000.0001\n  Description: uplink to core\n  Input queue: 0/2000/12/0 (size/max/drops/flushes); Total output drops: 345\n     1200 input errors, 1100 CRC, 0 frame, 0 overrun, 0 ignored\n     0 output errors, 7 collisions, 2 interface resets\nGigabitEthernet1/0/2 is down, line protocol is down (notconnect)\n  Input queue: 0/2000/0/0 (size/max/drops/flushes); Total output drops: 0\n     0 input errors, 0 CRC, 0 frame, 0 overrun, 0 ignored\n     0 output errors, 0 collisions, 1 interface resets\n";
        let i = interfaces(text);
        assert_eq!(i.len(), 2);
        assert_eq!(i[0].name, "GigabitEthernet1/0/1");
        assert_eq!(i[0].status, "up/up");
        assert_eq!(i[0].description, "uplink to core");
        assert_eq!(i[0].crc, 1100);
        assert_eq!(i[0].input_drops, 12);
        assert_eq!(i[0].output_drops, 345);
        assert_eq!(i[0].collisions, 7);
        assert_eq!(i[1].input_errors, 0);
    }

    #[test]
    fn status_table() {
        let text = "Port      Name               Status       Vlan       Duplex  Speed Type\nGi1/0/1   AP floor 2         connected    10         a-full  a-1000 10/100/1000BaseTX\nGi1/0/2                      notconnect   1            auto   auto 10/100/1000BaseTX\nGi1/0/3   old printer        connected    20         a-half   a-100 10/100/1000BaseTX\nGi1/0/4   cam 4              err-disabled 30           auto   auto 10/100/1000BaseTX\nTe1/1/1   uplink             connected    trunk        full    10G SFP-10GBase-SR\n";
        let t = interface_status(text);
        assert_eq!(t.rows.len(), 5);
        assert_eq!(t.rows[0], vec!["Gi1/0/1", "AP floor 2", "connected", "a-full", "a-1000", "10", "10/100/1000BaseTX"]);
        assert_eq!(t.rows[1][1], "");
        assert_eq!(t.rows[2][3], "a-half");
        assert_eq!(t.rows[3][2], "err-disabled");
        assert_eq!(t.rows[4][4], "10G");
    }

    #[test]
    fn trunk_sections() {
        let text = "Port        Mode             Encapsulation  Status        Native vlan\nGi1/0/49    on               802.1q         trunking      1\nTe1/1/1     desirable        n-802.1q       trunking      999\n\nPort        Vlans allowed on trunk\nGi1/0/49    1-4094\nTe1/1/1     10,20,30-35\n\nPort        Vlans allowed and active in management domain\nGi1/0/49    1,10,20\n";
        let t = trunks(text);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].native, "1");
        assert_eq!(t[0].allowed, "1-4094");
        assert_eq!(t[1].allowed, "10,20,30-35");
    }

    #[test]
    fn neighbors_and_tables() {
        let cdp = "-------------------------\nDevice ID: core-sw1.example.net\nEntry address(es):\n  IP address: 10.0.0.1\nPlatform: cisco C9300-48P,  Capabilities: Router Switch IGMP\nInterface: GigabitEthernet1/0/49,  Port ID (outgoing port): GigabitEthernet1/0/1\n";
        let n = cdp_neighbors(cdp);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].device, "core-sw1.example.net");
        assert_eq!(n[0].local, "GigabitEthernet1/0/49");
        assert_eq!(n[0].platform, "cisco C9300-48P");
        let mac = mac_table("          Mac Address Table\nVlan    Mac Address       Type        Ports\n----    -----------       --------    -----\n All    0100.0ccc.cccc    STATIC      CPU\n  10    aabb.ccdd.eeff    DYNAMIC     Gi1/0/5\n");
        assert_eq!(mac.rows.len(), 2);
        assert_eq!(mac.rows[1], vec!["10", "aabb.ccdd.eeff", "dynamic", "Gi1/0/5"]);
        let arp = arp_table("Protocol  Address          Age (min)  Hardware Addr   Type   Interface\nInternet  10.0.0.1                -   aabb.ccdd.0001  ARPA   Vlan1\nInternet  10.0.0.50             12   aabb.ccdd.0050  ARPA   Vlan1\n");
        assert_eq!(arp.rows.len(), 2);
        assert_eq!(arp.rows[1][1], "12");
        let ip = ip_interface_brief("Interface              IP-Address      OK? Method Status                Protocol\nVlan1                  10.0.0.2        YES NVRAM  up                    up\nGigabitEthernet0/0     unassigned      YES unset  administratively down down\n");
        assert_eq!(ip.rows.len(), 2);
        assert_eq!(ip.rows[1][2], "administratively down");
    }
}
