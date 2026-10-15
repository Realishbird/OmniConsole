//! Health checks for the non-Cisco platforms (and the checks that are the
//! same everywhere): per-OS command tables plus evaluators that read the
//! output with patterns rather than full parsers. Every evaluator returns
//! NotCollected when it cannot find what it is looking for, so an unreadable
//! output never counts as healthy.

use super::checks::{first, grep, n, num, re, Outputs, Status, Verdict};
use super::detect::Os;

/// Commands for a check on a platform. Empty = unsupported there.
pub fn commands(os: Os, id: &str) -> Vec<&'static str> {
    use Os::*;
    match (id, os) {
        // ----- the checks shared with Cisco
        ("time-sync", Ios | IosXe | NxOs | IosXr) => vec!["show ntp status"],
        ("time-sync", Asa) => vec!["show ntp status"],
        ("time-sync", SmallBusiness) => vec!["show sntp status"],
        ("time-sync", ArubaOsS) => vec!["show ntp status", "show sntp"],
        ("time-sync", ArubaCx) => vec!["show ntp status"],
        ("time-sync", Junos) => vec!["show ntp status"],
        ("time-sync", Eos) => vec!["show ntp status"],
        ("time-sync", Sonic) => vec!["show ntp"],
        ("time-sync", Cumulus) => vec!["chronyc tracking", "timedatectl"],
        ("storage", Ios | IosXe) => vec!["dir flash: | include bytes"],
        ("storage", NxOs) => vec!["dir bootflash: | include bytes"],
        ("storage", IosXr) => vec!["dir harddisk: | include bytes"],
        ("storage", Asa) => vec!["dir | include bytes"],
        ("storage", ArubaOsS) => vec!["show flash"],
        ("storage", ArubaCx) => vec!["show system resource-utilization"],
        ("storage", Junos) => vec!["show system storage"],
        ("storage", Eos) => vec!["show file systems"],
        ("storage", Sonic) => vec!["show platform storage", "df -h"],
        ("storage", Cumulus) => vec!["df -h"],
        ("optics", Ios | IosXe) => vec!["show interfaces transceiver"],
        ("optics", NxOs) => vec!["show interface transceiver details"],
        ("optics", ArubaOsS) => vec!["show interfaces transceiver detail"],
        ("optics", ArubaCx) => vec!["show interface transceiver"],
        ("optics", Junos) => vec!["show interfaces diagnostics optics"],
        ("optics", Eos) => vec!["show interfaces transceiver"],
        ("optics", Sonic) => vec!["show interface transceiver summary"],
        ("optics", Cumulus) => vec!["sudo l1-show all"],
        ("uplinks", Ios | IosXe) => vec!["show interfaces description"],
        ("uplinks", NxOs) => vec!["show interface description"],
        ("uplinks", ArubaOsS) => vec!["show interfaces brief"],
        ("uplinks", ArubaCx) => vec!["show interface brief"],
        ("uplinks", Junos) => vec!["show interfaces descriptions"],
        ("uplinks", Eos) => vec!["show interfaces description"],
        ("uplinks", Sonic) => vec!["show interface description"],
        ("uplinks", Cumulus) => vec!["nv show interface --view=description", "net show interface description"],
        ("overlay", IosXe | NxOs) => vec!["show nve peers", "show bgp l2vpn evpn summary"],
        ("overlay", ArubaCx) => vec!["show vxlan vni", "show bgp l2vpn evpn summary"],
        ("overlay", Junos) => vec!["show evpn instance", "show bgp summary"],
        ("overlay", Eos) => vec!["show vxlan vtep", "show bgp evpn summary"],
        ("overlay", Sonic) => vec!["show vxlan tunnel", "show bgp l2vpn evpn summary"],
        ("overlay", Cumulus) => vec!["nv show nve vxlan", "sudo vtysh -c \"show bgp l2vpn evpn summary\""],
        // ----- Aruba AOS-S
        ("system", ArubaOsS) => vec!["show version", "show system"],
        ("cpu", ArubaOsS) => vec!["show cpu"],
        ("memory", ArubaOsS) => vec!["show system"],
        ("temperature", ArubaOsS) => vec!["show system temperature"],
        ("power", ArubaOsS) => vec!["show system power-supply"],
        ("fans", ArubaOsS) => vec!["show system fans"],
        ("stack", ArubaOsS) => vec!["show stacking"],
        ("redundancy", ArubaOsS) => vec!["show vsf"],
        ("interface-errors", ArubaOsS) => vec!["show interfaces"],
        ("speed-duplex", ArubaOsS) => vec!["show interfaces brief"],
        ("poe", ArubaOsS) => vec!["show power-over-ethernet brief"],
        ("vlans", ArubaOsS) => vec!["show vlans", "show trunks"],
        ("port-channels", ArubaOsS) => vec!["show lacp", "show trunks"],
        ("spanning-tree", ArubaOsS) => vec!["show spanning-tree"],
        ("routing", ArubaOsS) => vec!["show ip ospf neighbor", "show ip bgp summary"],
        ("logs", ArubaOsS) => vec!["show logging -r"],
        ("neighbors", ArubaOsS) => vec!["show lldp info remote-device", "show cdp neighbors"],
        ("ip-interfaces", ArubaOsS) => vec!["show ip"],
        ("mac-table", ArubaOsS) => vec!["show mac-address"],
        ("arp", ArubaOsS) => vec!["show arp"],
        // ----- Aruba AOS-CX
        ("system", ArubaCx) => vec!["show version", "show system"],
        ("cpu", ArubaCx) => vec!["show system resource-utilization"],
        ("memory", ArubaCx) => vec!["show system resource-utilization"],
        ("temperature", ArubaCx) => vec!["show environment temperature"],
        ("power", ArubaCx) => vec!["show environment power-supply"],
        ("fans", ArubaCx) => vec!["show environment fan"],
        ("stack", ArubaCx) => vec!["show vsf"],
        ("redundancy", ArubaCx) => vec!["show vsx status"],
        ("interface-errors", ArubaCx) => vec!["show interface"],
        ("speed-duplex", ArubaCx) => vec!["show interface brief"],
        ("poe", ArubaCx) => vec!["show power-over-ethernet brief"],
        ("vlans", ArubaCx) => vec!["show vlan"],
        ("port-channels", ArubaCx) => vec!["show lacp interfaces"],
        ("spanning-tree", ArubaCx) => vec!["show spanning-tree"],
        ("routing", ArubaCx) => vec!["show ip ospf neighbors", "show bgp ipv4 unicast summary"],
        ("logs", ArubaCx) => vec!["show events -r -n 100"],
        ("neighbors", ArubaCx) => vec!["show lldp neighbor-info"],
        ("ip-interfaces", ArubaCx) => vec!["show ip interface brief"],
        ("mac-table", ArubaCx) => vec!["show mac-address-table"],
        ("arp", ArubaCx) => vec!["show arp"],
        // ----- Junos
        ("system", Junos) => vec!["show version", "show system uptime", "show chassis alarms", "show system alarms"],
        ("cpu", Junos) => vec!["show chassis routing-engine"],
        ("memory", Junos) => vec!["show chassis routing-engine"],
        ("temperature", Junos) => vec!["show chassis environment"],
        ("power", Junos) => vec!["show chassis environment"],
        ("fans", Junos) => vec!["show chassis environment"],
        ("stack", Junos) => vec!["show virtual-chassis"],
        ("redundancy", Junos) => vec!["show chassis routing-engine"],
        ("interface-errors", Junos) => vec!["show interfaces extensive | match \"Physical interface|errors|drops\""],
        ("speed-duplex", Junos) => vec!["show interfaces terse"],
        ("poe", Junos) => vec!["show poe interface"],
        ("vlans", Junos) => vec!["show vlans"],
        ("port-channels", Junos) => vec!["show lacp interfaces"],
        ("spanning-tree", Junos) => vec!["show spanning-tree bridge", "show spanning-tree interface"],
        ("routing", Junos) => vec!["show ospf neighbor", "show bgp summary"],
        ("logs", Junos) => vec!["show log messages | match \"error|fail|down|crit|alarm\" | last 100"],
        ("neighbors", Junos) => vec!["show lldp neighbors"],
        ("ip-interfaces", Junos) => vec!["show interfaces terse | match inet"],
        ("mac-table", Junos) => vec!["show ethernet-switching table"],
        ("arp", Junos) => vec!["show arp"],
        // ----- Arista EOS
        ("system", Eos) => vec!["show version"],
        ("cpu", Eos) => vec!["show processes top once"],
        ("memory", Eos) => vec!["show version"],
        ("temperature", Eos) => vec!["show environment temperature"],
        ("power", Eos) => vec!["show environment power"],
        ("fans", Eos) => vec!["show environment cooling"],
        ("stack", Eos) => vec![],
        ("redundancy", Eos) => vec!["show mlag"],
        ("interface-errors", Eos) => vec!["show interfaces counters errors"],
        ("speed-duplex", Eos) => vec!["show interfaces status"],
        ("poe", Eos) => vec!["show poe"],
        ("vlans", Eos) => vec!["show vlan", "show interfaces trunk"],
        ("port-channels", Eos) => vec!["show port-channel summary"],
        ("spanning-tree", Eos) => vec!["show spanning-tree"],
        ("routing", Eos) => vec!["show ip ospf neighbor", "show ip bgp summary"],
        ("logs", Eos) => vec!["show logging last 100"],
        ("neighbors", Eos) => vec!["show lldp neighbors"],
        ("ip-interfaces", Eos) => vec!["show ip interface brief"],
        ("mac-table", Eos) => vec!["show mac address-table"],
        ("arp", Eos) => vec!["show ip arp"],
        // ----- SONiC (Cisco S1 CLI)
        ("system", Sonic) => vec!["show version"],
        ("cpu", Sonic) => vec!["show system cpu", "top -bn1 | head -5"],
        ("memory", Sonic) => vec!["show system memory", "free -m"],
        ("temperature", Sonic) => vec!["show platform temperature"],
        ("power", Sonic) => vec!["show platform psustatus"],
        ("fans", Sonic) => vec!["show platform fanstatus"],
        ("stack", Sonic) => vec![],
        ("redundancy", Sonic) => vec!["show mclag brief"],
        ("interface-errors", Sonic) => vec!["show interface counters errors"],
        ("speed-duplex", Sonic) => vec!["show interface status"],
        ("poe", Sonic) => vec![],
        ("vlans", Sonic) => vec!["show vlan"],
        ("port-channels", Sonic) => vec!["show portchannel summary"],
        ("spanning-tree", Sonic) => vec!["show spanning-tree"],
        ("routing", Sonic) => vec!["show ip ospf neighbor", "show ip bgp summary"],
        ("logs", Sonic) => vec!["show logging | tail -n 100"],
        ("neighbors", Sonic) => vec!["show lldp table"],
        ("ip-interfaces", Sonic) => vec!["show ip interface"],
        ("mac-table", Sonic) => vec!["show mac address-table"],
        ("arp", Sonic) => vec!["show arp"],
        // ----- Cumulus: NVUE first, NCLU and Linux as fallbacks
        ("system", Cumulus) => vec!["nv show system", "net show system", "cat /etc/lsb-release", "uptime"],
        ("cpu", Cumulus) => vec!["top -bn1 | head -5"],
        ("memory", Cumulus) => vec!["free -m"],
        ("temperature", Cumulus) => vec!["sudo smonctl"],
        ("power", Cumulus) => vec!["sudo smonctl"],
        ("fans", Cumulus) => vec!["sudo smonctl"],
        ("stack", Cumulus) => vec![],
        ("redundancy", Cumulus) => vec!["nv show mlag", "net show clag", "sudo clagctl"],
        ("interface-errors", Cumulus) => vec!["ip -s -br link"],
        ("speed-duplex", Cumulus) => vec!["nv show interface", "net show interface all"],
        ("poe", Cumulus) => vec![],
        ("vlans", Cumulus) => vec!["nv show bridge domain br_default vlan", "net show bridge vlan"],
        ("port-channels", Cumulus) => vec!["nv show interface --view=bond", "net show interface bonds"],
        ("spanning-tree", Cumulus) => vec!["sudo mstpctl showbridge", "net show bridge spanning-tree"],
        ("routing", Cumulus) => vec!["sudo vtysh -c \"show ip ospf neighbor\"", "sudo vtysh -c \"show bgp summary\""],
        ("logs", Cumulus) => vec!["sudo journalctl -p err -n 100 --no-pager"],
        ("neighbors", Cumulus) => vec!["nv show interface --view=lldp", "net show lldp", "sudo lldpctl"],
        ("ip-interfaces", Cumulus) => vec!["ip -br addr"],
        ("mac-table", Cumulus) => vec!["sudo bridge fdb show | head -200"],
        ("arp", Cumulus) => vec!["ip neigh"],
        _ => vec![],
    }
}

/* ------------------------------------------------------------ helpers */

fn text_of<'a>(out: &'a Outputs, os: Os, id: &str) -> Option<&'a str> {
    let cmds = commands(os, id);
    first(out, &cmds)
}

/// Lines whose status words say something is wrong (and are not "no alarms" lines).
fn bad_lines(text: &str) -> Vec<String> {
    let bad = re(r"(?i)\b(fail(ed|ure)?|faulty|fault|critical|absent|not present|missing|shutdown|alarm|err(or)?|degraded|overheat|over temperature|under voltage|not operational|removed)\b");
    let fine = re(r"(?i)\b(no alarms?|none|0 (errors|alarms)|not? (present|applicable)|ok|normal|cleared)\b|alarms?\s*[:=]\s*(0|none)|^\s*-+\s*$|total|failures?\s*[:=]\s*0");
    text.lines().map(str::trim_end).filter(|l| bad.is_match(l) && !fine.is_match(l) && !l.trim_start().starts_with('%')).map(|l| l.to_string()).collect()
}

fn good_lines(text: &str) -> Vec<String> {
    let good = re(r"(?i)\b(ok|normal|good|present|operational|active|up|healthy|ready|nominal|running)\b");
    text.lines().map(str::trim_end).filter(|l| good.is_match(l)).map(|l| l.to_string()).collect()
}

fn pct(text: &str, patterns: &[&str]) -> Option<f64> {
    patterns.iter().find_map(|p| re(p).captures(text).and_then(|c| num(&c[1])))
}

fn not_configured(text: &str) -> bool {
    re(r"(?i)(not (enabled|configured|running|supported|available)|disabled|no (such|.* configured|entries|neighbors)|is not (active|enabled)|not found|unknown command|invalid)").is_match(text) && text.lines().filter(|l| !l.trim().is_empty()).count() <= 6
}

fn status_from_lines(title: &str, text: &str, follow: &[&'static str]) -> Verdict {
    let bad = bad_lines(text);
    let good = good_lines(text);
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{title}: {}", n(bad.len(), "problem line"))).evidence(bad).follow_up(follow)
    } else if !good.is_empty() {
        Verdict::new(Status::Healthy, format!("{title}: {} reported normal", n(good.len(), "item"))).evidence(good.into_iter().take(12).collect())
    } else if text.trim().is_empty() {
        Verdict::unknown(format!("{title}: the device returned no output"))
    } else if not_configured(text) {
        Verdict::new(Status::NotApplicable, format!("{title}: not present on this device")).evidence(text.lines().take(3).map(|l| l.to_string()).collect())
    } else {
        Verdict::unknown(format!("{title}: could not read the status words in the output")).evidence(text.lines().take(6).map(|l| l.to_string()).collect())
    }
}

fn rate(value: f64, what: &str, warn: f64, crit: f64, evidence: Vec<String>, follow: &[&'static str]) -> Verdict {
    let v = Verdict::new(if value >= crit { Status::Critical } else if value >= warn { Status::Warning } else { Status::Healthy }, format!("{what} {value:.0}%")).evidence(evidence);
    if value >= warn { v.follow_up(follow) } else { v }
}

/* ---------------------------------------------------------- evaluators */

pub fn evaluate(os: Os, id: &str, out: &Outputs) -> Verdict {
    let Some(text) = text_of(out, os, id) else {
        return Verdict::unknown("the command did not run");
    };
    match id {
        "system" => system(os, text),
        "cpu" => cpu(text),
        "memory" => memory(text),
        "temperature" => status_from_lines("Temperature", &section(os, id, text, "temp"), &[]),
        "power" => status_from_lines("Power supplies", &section(os, id, text, "power|psu|supply"), &[]),
        "fans" => status_from_lines("Fans", &section(os, id, text, "fan|cooling"), &[]),
        "stack" | "redundancy" => redundancy(id, text),
        "interface-errors" => interface_errors(text),
        "speed-duplex" => speed_duplex(text),
        "poe" => poe(text),
        "vlans" => count_rows("VLANs", text, r"(?m)^\s*\d{1,4}\s+\S", "VLAN"),
        "port-channels" => lag(text),
        "spanning-tree" => stp(text),
        "routing" => routing(os, out),
        "logs" => logs(text),
        "neighbors" => neighbors(text),
        "ip-interfaces" => count_rows("IP interfaces", text, r"(?m)\b\d{1,3}(\.\d{1,3}){3}\b", "address"),
        "mac-table" => count_rows("MAC address table", text, r"(?mi)\b[0-9a-f]{2}[:.-][0-9a-f]{2}[:.-]|\b[0-9a-f]{4}\.[0-9a-f]{4}\.[0-9a-f]{4}\b", "entry"),
        "arp" => count_rows("ARP table", text, r"(?m)\b\d{1,3}(\.\d{1,3}){3}\b", "entry"),
        "time-sync" => time_sync(text),
        "storage" => storage(text),
        "optics" => optics(text),
        "uplinks" => uplinks(text),
        "overlay" => overlay(text),
        _ => Verdict::unknown(format!("no evaluator for {id}")),
    }
}

fn system(os: Os, text: &str) -> Verdict {
    let f = super::detect::parse_facts(text);
    let mut lines = Vec::new();
    if !f.uptime.is_empty() {
        lines.push(format!("Uptime: {}", f.uptime));
    }
    let alarms = bad_lines(text);
    lines.extend(alarms.iter().cloned());
    let recent = re(r"(?i)^\s*(\d+)\s+(minute|hour)s?").is_match(&f.uptime) && !re(r"(?i)day|week|year").is_match(&f.uptime);
    let summary = format!("{} {} on {}{}", os.label(), f.version, if f.model.is_empty() { "unknown model".to_string() } else { f.model.clone() }, if f.serial.is_empty() { String::new() } else { format!(", serial {}", f.serial) });
    if f.version.is_empty() && f.model.is_empty() {
        return Verdict::unknown("could not read the version or model from show version").evidence(text.lines().take(5).map(|l| l.to_string()).collect());
    }
    if !alarms.is_empty() {
        Verdict::new(Status::Warning, format!("Active alarms. {summary}")).evidence(lines)
    } else if recent {
        Verdict::new(Status::Warning, format!("Rebooted recently (up {}). {summary}", f.uptime)).evidence(lines)
    } else {
        Verdict::new(Status::Healthy, summary).evidence(lines)
    }
}

fn cpu(text: &str) -> Verdict {
    let busy = pct(text, &[r"(\d+)\s*percent busy", r"CPU usage \(%\)\s*:\s*(\d+)", r"CPU usage\s*:\s*(\d+)", r"CPU utilization\s*:\s*(\d+)%", r"cpu[_ ]usage\s*[:=]?\s*([\d.]+)"]);
    let idle = pct(text, &[r"Idle\s+(\d+) percent", r"([\d.]+)\s*%?\s*id\b", r"([\d.]+)% idle", r"idle\s*[:=]\s*([\d.]+)"]);
    let value = busy.or_else(|| idle.map(|i| 100.0 - i));
    match value {
        Some(v) => rate(v, "CPU", 70.0, 85.0, grep(text, "(?i)cpu|busy|idle|load"), &[]),
        None => Verdict::unknown("could not find a CPU percentage").evidence(text.lines().take(5).map(|l| l.to_string()).collect()),
    }
}

fn memory(text: &str) -> Verdict {
    if let Some(v) = pct(text, &[r"Memory usage \(%\)\s*:\s*(\d+)", r"Memory utilization\s+(\d+) percent", r"memory[_ ]usage\s*[:=]?\s*([\d.]+)%?"]) {
        return rate(v, "Memory", 80.0, 90.0, grep(text, "(?i)memory"), &[]);
    }
    // total/free pairs
    let pairs: &[(&str, bool)] = &[
        (r"Total memory:\s*(\d+) kB\s+Free memory:\s*(\d+) kB", true),
        (r"Memory\s*-\s*Total\s*:\s*([\d,]+)\s+Free\s*:\s*([\d,]+)", true),
        (r"(?m)^Mem:\s+(\d+)\s+(\d+)\s+(\d+)", false),
    ];
    for (p, total_free) in pairs {
        if let Some(c) = re(p).captures(text) {
            let total = num(&c[1]).unwrap_or(0.0);
            let used = if *total_free { total - num(&c[2]).unwrap_or(0.0) } else { num(&c[2]).unwrap_or(0.0) };
            if total > 0.0 {
                return rate(used / total * 100.0, "Memory", 80.0, 90.0, grep(text, "(?i)mem"), &[]);
            }
        }
    }
    Verdict::unknown("could not find memory figures").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
}

/// Lines of an environment dump that belong to one kind of sensor, when
/// the platform prints temperature, power and fans in one command.
fn section(os: Os, id: &str, text: &str, word: &str) -> String {
    let shared = ["temperature", "power", "fans"].iter().filter(|o| commands(os, o) == commands(os, id)).count() > 1;
    if !shared {
        return text.to_string();
    }
    let r = re(&format!("(?i){word}"));
    let picked: Vec<&str> = text.lines().filter(|l| r.is_match(l)).collect();
    if picked.is_empty() { text.to_string() } else { picked.join("\n") }
}

fn redundancy(id: &str, text: &str) -> Verdict {
    let title = if id == "stack" { "Stack / Virtual Chassis" } else { "Redundancy (VSX / MLAG / VSF)" };
    if not_configured(text) || text.trim().is_empty() {
        return Verdict::new(Status::NotApplicable, format!("{title}: not configured")).evidence(text.lines().take(3).map(|l| l.to_string()).collect());
    }
    let bad = re(r"(?i)\b(inactive|not consistent|inconsistent|down|failed|split|secondary-standalone|not synced|out-of-sync|conflict|peer.*(down|unreachable|not found))\b");
    let fine = re(r"(?i)(none|no |0 )");
    let problems: Vec<String> = text.lines().filter(|l| bad.is_match(l) && !fine.is_match(l)).map(|l| l.trim_end().to_string()).collect();
    let good = re(r"(?i)\b(active|established|in-sync|consistent|present|up|connected|prsnt|ok|primary|secondary|master|backup|linecard)\b");
    let goods: Vec<String> = text.lines().filter(|l| good.is_match(l)).map(|l| l.trim_end().to_string()).collect();
    if !problems.is_empty() {
        Verdict::new(Status::Critical, format!("{title}: {}", n(problems.len(), "problem"))).evidence(problems)
    } else if !goods.is_empty() {
        Verdict::new(Status::Healthy, format!("{title}: {} members / peers reported fine", goods.len())).evidence(goods.into_iter().take(12).collect())
    } else {
        Verdict::unknown(format!("{title}: could not read the status")).evidence(text.lines().take(5).map(|l| l.to_string()).collect())
    }
}

fn interface_errors(text: &str) -> Verdict {
    // "Physical interface: ge-0/0/1" / "GigabitEthernet1/1 ..." style headers, then counter lines
    let header = re(r"(?i)^(Physical interface:\s*(\S+)|(\S+)\s+is\s+(up|down|administratively)|\s*(swp\d+\S*|eth\d+\S*|Ethernet\d\S*|Et\d\S*|ge-\S+|xe-\S+|et-\S+|\d+/\d+/\d+|\d{1,2})\b)");
    let counter = re(r"(?i)\b([1-9]\d*)\s+(input errors|output errors|CRC|crc|frame|runts|giants|discards|drops?|errors)\b|\b(input errors|output errors|CRC|FCS|discards?|drops?|errors?)\s*[:=]?\s*([1-9]\d*)\b");
    let mut current = String::new();
    let mut hits: Vec<String> = Vec::new();
    let mut parsed = 0;
    for line in text.lines() {
        if let Some(c) = header.captures(line) {
            current = c.get(2).or(c.get(3)).or(c.get(5)).map(|m| m.as_str().to_string()).unwrap_or_default();
            parsed += 1;
            continue;
        }
        if counter.is_match(line) && !line.contains(" 0 ") {
            hits.push(format!("{}: {}", current, line.trim()));
        }
    }
    // tabular outputs (EOS/SONiC counters errors): a row with any non-zero error column
    let row = re(r"(?m)^\s*(Et\S+|Ethernet\S+|swp\S+|Po\S+|\d+/\d+/\d+)\s+(.*)$");
    for c in row.captures_iter(text) {
        parsed += 1;
        let cols: Vec<&str> = c[2].split_whitespace().collect();
        if cols.iter().any(|v| v.parse::<u64>().map(|x| x > 0).unwrap_or(false)) {
            hits.push(format!("{}: {}", &c[1], c[2].trim()));
        }
    }
    hits.dedup();
    if parsed == 0 && hits.is_empty() {
        return Verdict::unknown("could not find interface counters in the output").evidence(text.lines().take(5).map(|l| l.to_string()).collect());
    }
    if hits.is_empty() {
        Verdict::new(Status::Healthy, format!("No errors or discards on {}", n(parsed, "interface"))).evidence(vec![format!("{parsed} interfaces read, every error counter 0")])
    } else {
        Verdict::new(Status::Warning, format!("Errors or discards on {}", n(hits.len(), "counter line"))).evidence(hits)
    }
}

fn speed_duplex(text: &str) -> Verdict {
    let half = grep(text, r"(?i)\b(half|10M|10 ?Mbps|err-?disabled|errdisable|suspended|lacp-blocked)\b");
    let half: Vec<String> = half.into_iter().filter(|l| !re(r"(?i)^\s*(port|interface|name|-)").is_match(l)).collect();
    let rows = text.lines().filter(|l| re(r"(?i)\b(up|down|connected|notconnect|disabled|linkup|linkdown)\b").is_match(l)).count();
    if rows == 0 {
        return Verdict::unknown("could not find interface status rows").evidence(text.lines().take(5).map(|l| l.to_string()).collect());
    }
    if half.is_empty() {
        Verdict::new(Status::Healthy, format!("{} at expected speed/duplex, none err-disabled", n(rows, "interface"))).evidence(vec![format!("{rows} status rows read")])
    } else {
        Verdict::new(Status::Warning, format!("{} with half duplex, 10 Mbps or err-disabled", n(half.len(), "interface"))).evidence(half)
    }
}

fn poe(text: &str) -> Verdict {
    if not_configured(text) || re(r"(?i)no poe|not a poe|not supported").is_match(text) {
        return Verdict::new(Status::NotApplicable, "No PoE on this device");
    }
    let total = pct(text, &[r"(?i)(?:total|available|budget)[^\d\n]{0,30}?([\d.]+)\s*W", r"(?i)Available Power\s*[:=]\s*([\d.]+)"]);
    let used = pct(text, &[r"(?i)(?:used|consumed|allocated|in use)[^\d\n]{0,30}?([\d.]+)\s*W", r"(?i)Power Used\s*[:=]\s*([\d.]+)"]);
    let faults = grep(text, r"(?i)\b(fault|denied|overload|over-?current|short)\b");
    match (total, used) {
        (Some(t), Some(u)) if t > 0.0 => {
            let p = u / t * 100.0;
            let mut v = rate(p, "PoE budget used", 80.0, 95.0, grep(text, "(?i)power|watt|W\\b"), &[]);
            if !faults.is_empty() {
                v.status = Status::Warning;
                v.summary = format!("{} with PoE faults; {}", n(faults.len(), "port"), v.summary);
                v.evidence = faults;
            }
            v
        }
        _ if !faults.is_empty() => Verdict::new(Status::Warning, format!("{} with PoE faults", n(faults.len(), "port"))).evidence(faults),
        _ => Verdict::unknown("could not find the PoE budget figures").evidence(text.lines().take(5).map(|l| l.to_string()).collect()),
    }
}

fn count_rows(title: &str, text: &str, pattern: &str, word: &str) -> Verdict {
    let count = re(pattern).find_iter(text).count();
    if count == 0 {
        if not_configured(text) || text.trim().is_empty() {
            return Verdict::new(Status::NotApplicable, format!("{title}: none"));
        }
        return Verdict::unknown(format!("{title}: could not read the rows")).evidence(text.lines().take(5).map(|l| l.to_string()).collect());
    }
    Verdict::new(Status::Healthy, format!("{title}: {}", n(count, word))).evidence(text.lines().filter(|l| !l.trim().is_empty()).take(12).map(|l| l.trim_end().to_string()).collect())
}

fn lag(text: &str) -> Verdict {
    if not_configured(text) || text.trim().is_empty() {
        return Verdict::new(Status::NotApplicable, "No LAGs configured");
    }
    let bad = grep(text, r"(?i)\b(down|standby|individual|suspended|detached|not bundled|lacp.?blocked|inactive|D\)|s\)|I\))");
    let bad: Vec<String> = bad.into_iter().filter(|l| !re(r"(?i)^\s*(flags|legend|-|port|name)").is_match(l)).collect();
    let rows = text.lines().filter(|l| re(r"(?i)\b(Po\d+|port-channel\d+|bond\d+|ae\d+|Trk\d+|lag\d+|PortChannel\d+)\b").is_match(l)).count();
    if !bad.is_empty() {
        Verdict::new(Status::Warning, format!("{} with a member down or not bundled", n(bad.len(), "LAG line"))).evidence(bad)
    } else if rows > 0 {
        Verdict::new(Status::Healthy, format!("{} up with all members bundled", n(rows, "LAG"))).evidence(text.lines().filter(|l| !l.trim().is_empty()).take(12).map(|l| l.trim_end().to_string()).collect())
    } else {
        Verdict::unknown("could not read the LAG rows").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
    }
}

fn stp(text: &str) -> Verdict {
    if re(r"(?i)(not enabled|disabled|No spanning tree|not running)").is_match(text) && text.lines().count() < 8 {
        return Verdict::new(Status::Warning, "Spanning tree is not enabled").evidence(text.lines().take(3).map(|l| l.to_string()).collect());
    }
    let bad = grep(text, r"(?i)\b(inconsistent|blocked by|root guard|bpdu guard|loop guard|err-?disabled|dispute)\b");
    let blocking = text.lines().filter(|l| re(r"(?i)\b(BLK|blocking|discarding|DIS\b|ALT|Altn)\b").is_match(l)).count();
    let root = grep(text, r"(?i)root (bridge|id|port)|this bridge is the root|Root ID");
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("Spanning tree: {}", n(bad.len(), "guard or inconsistency line"))).evidence(bad)
    } else if !root.is_empty() || blocking > 0 || re(r"(?i)spanning|stp|rstp|mstp").is_match(text) {
        let mut ev = root.into_iter().take(6).collect::<Vec<_>>();
        ev.push(format!("{blocking} blocking/discarding port lines"));
        Verdict::new(Status::Healthy, format!("Spanning tree running, {} blocking/discarding", n(blocking, "port"))).evidence(ev)
    } else {
        Verdict::unknown("could not read the spanning tree output").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
    }
}

fn routing(os: Os, out: &Outputs) -> Verdict {
    let cmds = commands(os, "routing");
    let mut evidence = Vec::new();
    let mut problems = Vec::new();
    let mut seen = 0;
    for c in &cmds {
        let Some(text) = out.get(*c).and_then(|r| r.as_ref().ok()) else { continue };
        if not_configured(text) || text.trim().is_empty() {
            continue;
        }
        let is_bgp = c.contains("bgp");
        for line in text.lines() {
            let l = line.trim_end();
            if is_bgp {
                // a neighbour row: address, AS, ..., state/prefixes
                if re(r"^\s*\*?\s*\d{1,3}(\.\d{1,3}){3}\s").is_match(l) || re(r"^\s*[0-9a-f:]+:[0-9a-f:]+\s+\d").is_match(l) {
                    seen += 1;
                    if re(r"(?i)\b(Idle|Active|Connect|OpenSent|OpenConfirm|never)\b").is_match(l) {
                        problems.push(format!("BGP: {l}"));
                    } else {
                        evidence.push(format!("BGP: {l}"));
                    }
                }
            } else if re(r"^\s*\d{1,3}(\.\d{1,3}){3}\s").is_match(l) || re(r"(?i)^\s*(\S+)\s+\d{1,3}(\.\d{1,3}){3}\s").is_match(l) {
                seen += 1;
                if re(r"(?i)\b(Full|2-?Way)\b").is_match(l) {
                    evidence.push(format!("OSPF: {l}"));
                } else if re(r"(?i)\b(Init|ExStart|Exchange|Loading|Down|Attempt)\b").is_match(l) {
                    problems.push(format!("OSPF: {l}"));
                } else {
                    evidence.push(format!("OSPF: {l}"));
                }
            }
        }
    }
    if seen == 0 {
        return Verdict::new(Status::NotApplicable, "No OSPF or BGP neighbors configured");
    }
    if !problems.is_empty() {
        Verdict::new(Status::Critical, format!("{} not up", n(problems.len(), "routing neighbor"))).evidence(problems)
    } else {
        Verdict::new(Status::Healthy, format!("{} up", n(seen, "routing neighbor"))).evidence(evidence.into_iter().take(20).collect())
    }
}

fn logs(text: &str) -> Verdict {
    let hits = grep(text, r"(?i)(\b(crit|critical|emerg|alert|error|err|fail(ed|ure|ing)?|panic)\b|%\w+-[0-3]-|\bI\s+\d+/\d+/\d+\s+\d+:\d+:\d+\s+\S+\s+(crit|err))");
    let hits: Vec<String> = hits.into_iter().filter(|l| !re(r"(?i)(0 errors|no errors|errors: 0|no such file|^\s*--)").is_match(l)).collect();
    if !hits.is_empty() {
        Verdict::new(Status::Warning, format!("{} with errors or worse in the recent log", n(hits.len(), "message"))).evidence(hits.into_iter().rev().take(30).collect())
    } else if text.trim().is_empty() {
        Verdict::new(Status::Healthy, "No error-level log messages matched").evidence(vec!["the log filter returned nothing".into()])
    } else {
        Verdict::new(Status::Healthy, format!("No error-level messages in the last {}", n(text.lines().count(), "line"))).evidence(text.lines().rev().take(5).map(|l| l.trim_end().to_string()).collect())
    }
}

fn neighbors(text: &str) -> Verdict {
    let rows: Vec<String> = text
        .lines()
        .filter(|l| re(r"(?i)\b(swp\d+|eth\d+|Et(hernet)?\d+\S*|ge-\S+|xe-\S+|et-\S+|\d+/\d+(/\d+)?|Gi\S*\d|Te\S*\d|\b\d{1,2}\b)\s").is_match(l) && !re(r"(?i)^\s*(local|port|interface|-|chassis|neighbor|name)").is_match(l) && re(r"[A-Za-z0-9][\w.-]+").is_match(l))
        .map(|l| l.trim_end().to_string())
        .collect();
    let rows: Vec<String> = rows.into_iter().filter(|l| l.split_whitespace().count() >= 2).collect();
    if rows.is_empty() {
        if text.trim().is_empty() || not_configured(text) {
            return Verdict::new(Status::Warning, "No LLDP/CDP neighbors seen").evidence(text.lines().take(3).map(|l| l.to_string()).collect());
        }
        return Verdict::unknown("could not read the neighbor rows").evidence(text.lines().take(5).map(|l| l.to_string()).collect());
    }
    Verdict::new(Status::Healthy, format!("{}", n(rows.len(), "neighbor"))).evidence(rows.into_iter().take(30).collect())
}

fn time_sync(text: &str) -> Verdict {
    if re(r"(?i)(unsynchroni[sz]ed|not synchroni|no (ntp )?server|stratum\s*:?\s*16|Leap status\s*:\s*Not synchroni|System clock synchronized:\s*no|clock is not|NTP is disabled|not enabled)").is_match(text) {
        return Verdict::new(Status::Warning, "Clock is not synchronized").evidence(grep(text, r"(?i)sync|stratum|server|leap|ntp"));
    }
    if re(r"(?i)(\bsynchroni[sz]ed\b|System clock synchronized:\s*yes|Leap status\s*:\s*Normal|stratum\s*:?\s*[1-9]\b|stratum\s*:?\s*1[0-5]\b|^\s*\*)").is_match(text) {
        return Verdict::new(Status::Healthy, "Clock synchronized").evidence(grep(text, r"(?i)sync|stratum|server|reference|leap"));
    }
    Verdict::unknown("could not read the NTP status").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
}

fn storage(text: &str) -> Verdict {
    // df style "95%" or Cisco "X bytes total (Y bytes free)"
    let mut worst: Option<(f64, String)> = None;
    for c in re(r"(?m)^(\S.*?)\s+(\d{1,3})%\s").captures_iter(text) {
        if let Some(p) = num(&c[2]) {
            if worst.as_ref().map(|w| p > w.0).unwrap_or(true) {
                worst = Some((p, c[0].trim().to_string()));
            }
        }
    }
    if worst.is_none() {
        if let Some(c) = re(r"(\d+)\s+bytes (?:total|available)\s*\((\d+)\s+bytes free\)").captures(text) {
            let (t, f) = (num(&c[1]).unwrap_or(0.0), num(&c[2]).unwrap_or(0.0));
            if t > 0.0 {
                worst = Some(((t - f) / t * 100.0, c[0].to_string()));
            }
        }
    }
    if worst.is_none() {
        if let Some(c) = re(r"(?i)(?:storage|disk|flash)[^\n%]*?([\d.]+)\s*%").captures(text) {
            worst = num(&c[1]).map(|p| (p, c[0].trim().to_string()));
        }
    }
    match worst {
        Some((p, line)) => rate(p, "Storage used", 85.0, 95.0, vec![line], &[]),
        None => Verdict::unknown("could not read storage usage").evidence(text.lines().take(5).map(|l| l.to_string()).collect()),
    }
}

fn optics(text: &str) -> Verdict {
    if text.trim().is_empty() || not_configured(text) {
        return Verdict::new(Status::NotApplicable, "No optical transceivers reported");
    }
    let bad = grep(text, r"(?i)(\b(low|high)[- ](alarm|warn(ing)?)\b|\+\+|--|\bN/A\b.*dBm|-inf|\balarm\b)");
    let bad: Vec<String> = bad.into_iter().filter(|l| !re(r"(?i)^\s*(port|interface|name|-|threshold|legend)").is_match(l) && !l.contains("++ : high alarm")).collect();
    let rows = text.lines().filter(|l| re(r"-?\d+\.\d+\s*(dBm|C\b|mA|V\b)|\bdBm\b").is_match(l)).count();
    if !bad.is_empty() {
        Verdict::new(Status::Warning, format!("{} with alarm or warning thresholds crossed", n(bad.len(), "transceiver line"))).evidence(bad)
    } else if rows > 0 {
        Verdict::new(Status::Healthy, format!("{} within thresholds", n(rows, "transceiver reading"))).evidence(text.lines().filter(|l| re(r"dBm").is_match(l)).take(12).map(|l| l.trim_end().to_string()).collect())
    } else {
        Verdict::unknown("could not read optical levels").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
    }
}

fn uplinks(text: &str) -> Verdict {
    let named: Vec<&str> = text.lines().filter(|l| re(r"(?i)\b(uplink|core|dist|agg|trunk to|to-|->|wan|isp|mlag|peer|vsx|isl)\b").is_match(l)).collect();
    if named.is_empty() {
        return Verdict::new(Status::NotApplicable, "No interfaces described as uplinks (name them 'uplink' or 'core' in the description)");
    }
    let down: Vec<String> = named.iter().filter(|l| re(r"(?i)\b(down|notconnect|admin|disabled|notconn)\b").is_match(l)).map(|l| l.trim_end().to_string()).collect();
    if !down.is_empty() {
        Verdict::new(Status::Critical, format!("{} down", n(down.len(), "uplink"))).evidence(down)
    } else {
        Verdict::new(Status::Healthy, format!("{} up", n(named.len(), "uplink"))).evidence(named.iter().map(|l| l.trim_end().to_string()).collect())
    }
}

fn overlay(text: &str) -> Verdict {
    if text.trim().is_empty() || not_configured(text) {
        return Verdict::new(Status::NotApplicable, "No VXLAN / EVPN configured");
    }
    let bad = grep(text, r"(?i)\b(down|Idle|Active|Connect|failed|not up|inactive)\b");
    let bad: Vec<String> = bad.into_iter().filter(|l| !re(r"(?i)^\s*(flags|legend|-|peer|vni|interface)").is_match(l)).collect();
    let rows = text.lines().filter(|l| re(r"(?i)\b(up|Established|\d+\s+\d+\s+\d+\s*$|L2VNI|L3VNI|vtep|peer)\b").is_match(l)).count();
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{} down or not established", n(bad.len(), "overlay peer/VNI line"))).evidence(bad)
    } else if rows > 0 {
        Verdict::new(Status::Healthy, format!("{} up", n(rows, "overlay line"))).evidence(text.lines().filter(|l| !l.trim().is_empty()).take(15).map(|l| l.trim_end().to_string()).collect())
    } else {
        Verdict::unknown("could not read the overlay status").evidence(text.lines().take(5).map(|l| l.to_string()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(cmd: &str, text: &str) -> Outputs {
        [(cmd.to_string(), Ok(text.to_string()))].into_iter().collect()
    }

    #[test]
    fn every_platform_has_commands_for_the_quick_checks() {
        for os in [Os::ArubaOsS, Os::ArubaCx, Os::Junos, Os::Eos, Os::Sonic, Os::Cumulus] {
            for id in ["system", "cpu", "memory", "temperature", "power", "fans", "interface-errors", "logs", "time-sync", "storage", "neighbors", "routing"] {
                assert!(!commands(os, id).is_empty(), "{os:?} has no commands for {id}");
            }
        }
    }

    #[test]
    fn aruba_cpu_and_memory() {
        let v = evaluate(Os::ArubaOsS, "cpu", &out("show cpu", "  7 percent busy, from 300 sec ago\n  1 sec ave:  5 percent busy\n"));
        assert_eq!(v.status, Status::Healthy);
        assert!(v.summary.contains("CPU 7%"), "{}", v.summary);
        let v = evaluate(Os::ArubaOsS, "memory", &out("show system", "  Memory   - Total : 1,073,741,824 Free : 100,000,000\n"));
        assert_eq!(v.status, Status::Critical);
        let v = evaluate(Os::ArubaCx, "cpu", &out("show system resource-utilization", "CPU usage (%)     : 92\nMemory usage (%)  : 40\n"));
        assert_eq!(v.status, Status::Critical);
        let v = evaluate(Os::ArubaCx, "memory", &out("show system resource-utilization", "CPU usage (%)     : 12\nMemory usage (%)  : 40\n"));
        assert_eq!(v.status, Status::Healthy);
    }

    #[test]
    fn junos_and_eos_resources() {
        let v = evaluate(Os::Junos, "cpu", &out("show chassis routing-engine", "    CPU utilization:\n      User                       3 percent\n      Idle                      95 percent\n    Memory utilization          41 percent\n"));
        assert_eq!(v.status, Status::Healthy);
        assert!(v.summary.contains("CPU 5%"), "{}", v.summary);
        let v = evaluate(Os::Junos, "memory", &out("show chassis routing-engine", "    Memory utilization          91 percent\n"));
        assert_eq!(v.status, Status::Critical);
        let v = evaluate(Os::Eos, "memory", &out("show version", "Total memory: 8098976 kB\nFree memory: 5560316 kB\n"));
        assert_eq!(v.status, Status::Healthy);
        let v = evaluate(Os::Eos, "cpu", &out("show processes top once", "top - 10:00:01 up 10 days\n%Cpu(s):  4.0 us,  2.0 sy,  0.0 ni, 93.0 id,  1.0 wa\n"));
        assert_eq!(v.status, Status::Healthy);
        let v = evaluate(Os::Cumulus, "memory", &out("free -m", "              total        used        free\nMem:           7976        7300         300\n"));
        assert_eq!(v.status, Status::Critical);
    }

    #[test]
    fn unreadable_output_is_not_healthy() {
        let v = evaluate(Os::Junos, "cpu", &out("show chassis routing-engine", "garbage"));
        assert_eq!(v.status, Status::NotCollected);
        let v = evaluate(Os::Eos, "power", &out("show environment power", ""));
        assert_eq!(v.status, Status::NotCollected);
        let v = evaluate(Os::Eos, "poe", &out("show poe", "% PoE is not supported on this platform"));
        assert_eq!(v.status, Status::NotApplicable);
    }

    #[test]
    fn environment_and_redundancy_words() {
        let v = evaluate(Os::ArubaCx, "power", &out("show environment power-supply", "Mbr/PSU  Product   Serial    PSU     Wattage  Status\n1/1      JL086A    CN1       AC      680      OK\n1/2      JL086A    CN2       AC      680      Absent\n"));
        assert_eq!(v.status, Status::Critical);
        assert!(v.evidence[0].contains("Absent"));
        let v = evaluate(Os::ArubaCx, "fans", &out("show environment fan", "Mbr/Fan  Status  Speed\n1/1      ok      normal\n1/2      ok      normal\n"));
        assert_eq!(v.status, Status::Healthy);
        let v = evaluate(Os::ArubaCx, "redundancy", &out("show vsx status", "VSX Operational State\n  ISL channel          : In-Sync\n  ISL mgmt channel     : operational\n  Config Sync Status   : In-Sync\n  NAE                  : peer_reachable\n  HTTPS Server         : peer_reachable\n"));
        assert_eq!(v.status, Status::Healthy);
        let v = evaluate(Os::Eos, "redundancy", &out("show mlag", "MLAG Configuration:\ndomain-id : dc1\nlocal-interface : Vlan4094\npeer-link : Port-Channel10\nMLAG Status:\nstate : Inactive\nnegotiation status : Connecting\n"));
        assert_eq!(v.status, Status::Critical);
        let v = evaluate(Os::Eos, "redundancy", &out("show mlag", "MLAG is not configured\n"));
        assert_eq!(v.status, Status::NotApplicable);
    }

    #[test]
    fn routing_logs_time() {
        let mut o = out("show ip bgp summary", "BGP summary information for VRF default\nNeighbor   V  AS    MsgRcvd MsgSent InQ OutQ Up/Down State/PfxRcd\n10.0.0.1   4  65001 100     100     0   0    1d02h    12\n10.0.0.2   4  65001 0       0       0   0    never    Active\n");
        o.insert("show ip ospf neighbor".into(), Ok("Neighbor ID  Pri State  Dead Time Address   Interface\n10.0.0.9     1   FULL/DR 00:00:35  10.1.1.9  Ethernet1\n".into()));
        let v = evaluate(Os::Eos, "routing", &o);
        assert_eq!(v.status, Status::Critical);
        assert_eq!(v.evidence.len(), 1);
        let v = evaluate(Os::Junos, "logs", &out("show log messages | match \"error|fail|down|crit|alarm\" | last 100", "Jan 1 10:00:00 edge1 chassisd[1]: CHASSISD_FAN_FAILURE: fan 1 failed\n"));
        assert_eq!(v.status, Status::Warning);
        let v = evaluate(Os::ArubaCx, "time-sync", &out("show ntp status", "NTP Status Information\nNTP is enabled\nNTP Authentication is disabled\nSynchronized to NTP server 10.0.0.5 at stratum 3\n"));
        assert_eq!(v.status, Status::Healthy);
        let v = evaluate(Os::Cumulus, "time-sync", &out("chronyc tracking", "Reference ID    : 00000000 ()\nStratum         : 0\nLeap status     : Not synchronised\n"));
        assert_eq!(v.status, Status::Warning);
        let v = evaluate(Os::Cumulus, "storage", &out("df -h", "Filesystem Size Used Avail Use% Mounted on\n/dev/sda1   7G  6.7G  200M  96% /\ntmpfs       1G    0    1G   0% /run\n"));
        assert_eq!(v.status, Status::Critical);
    }

    #[test]
    fn interface_errors_tabular_and_junos() {
        let v = evaluate(Os::Eos, "interface-errors", &out("show interfaces counters errors", "Port      FCS    Align  Symbol  Rx    Runts  Giants  Tx\nEt1       0      0      0       0     0      0       0\nEt2       12     0      0       0     0      0       0\n"));
        assert_eq!(v.status, Status::Warning);
        assert!(v.evidence[0].starts_with("Et2"));
        let v = evaluate(Os::Junos, "interface-errors", &out("show interfaces extensive | match \"Physical interface|errors|drops\"", "Physical interface: ge-0/0/0, Enabled, Physical link is Up\n    Errors: 0, Drops: 0, Framing errors: 0, Runts: 0\n  Output errors:\n    Carrier transitions: 1, Errors: 0, Drops: 0\n"));
        assert_eq!(v.status, Status::Healthy);
    }
}
