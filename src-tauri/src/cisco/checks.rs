//! Read-only health checks. Every check names the commands it needs, reads
//! their output and produces passed / warning / failed / unknown with the
//! lines that back it up and the commands to run next. Nothing here sends
//! anything but `show` commands.

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::detect::Os;
use super::generic;
use super::parse::{self, Table};

/// The spec's states. A failed collection or parse is never Healthy: it is
/// NotCollected, with the reason in the summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Healthy,
    Warning,
    Critical,
    /// the feature is not in use on this device (no stack, no PoE)
    NotApplicable,
    /// the platform has no way to check this
    Unsupported,
    /// the command failed or its output could not be read
    NotCollected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub id: String,
    pub title: String,
    pub category: String,
    pub status: Status,
    pub summary: String,
    /// output lines that back the verdict
    pub evidence: Vec<String>,
    pub follow_up: Vec<String>,
    pub commands: Vec<String>,
    /// structured view of the output, when there is one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<Table>,
    /// when the output was collected (unix seconds)
    #[serde(default)]
    pub collected_at: i64,
    /// the one thing to do next
    #[serde(default)]
    pub next_step: String,
}

/// command -> output (Err = the device rejected it or it timed out)
pub type Outputs = BTreeMap<String, Result<String, String>>;

pub struct CheckDef {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    /// part of the Quick Health Check (the Detailed one runs everything)
    pub quick: bool,
    pub commands: fn(Os) -> Vec<&'static str>,
    pub evaluate: fn(Os, &Outputs) -> Verdict,
}

/// Which checks a tier runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Quick,
    Detailed,
}

pub struct Verdict {
    pub status: Status,
    pub summary: String,
    pub evidence: Vec<String>,
    pub follow_up: Vec<&'static str>,
    pub table: Option<Table>,
}

impl Verdict {
    pub(super) fn new(status: Status, summary: impl Into<String>) -> Self {
        Verdict { status, summary: summary.into(), evidence: Vec::new(), follow_up: Vec::new(), table: None }
    }
    pub(super) fn evidence_first(mut self, line: String) -> Self {
        self.evidence.insert(0, line);
        self
    }

    pub(super) fn evidence(mut self, lines: Vec<String>) -> Self {
        self.evidence = lines.into_iter().take(40).collect();
        self
    }
    pub(super) fn follow_up(mut self, cmds: &[&'static str]) -> Self {
        self.follow_up = cmds.to_vec();
        self
    }
    pub(super) fn table(mut self, t: Table) -> Self {
        self.table = Some(t);
        self
    }
    /// The output could not be read: never a healthy result.
    pub(super) fn unknown(why: impl Into<String>) -> Self {
        Verdict::new(Status::NotCollected, why)
    }
}

pub(super) fn re(s: &str) -> Regex {
    Regex::new(s).expect("check regex")
}

/// Output of the first of `cmds` that worked.
pub(super) fn first<'a>(out: &'a Outputs, cmds: &[&str]) -> Option<&'a str> {
    cmds.iter().find_map(|c| out.get(*c).and_then(|r| r.as_ref().ok()).map(|s| s.as_str()))
}

pub(super) fn rejected(out: &Outputs, cmds: &[&str]) -> Option<String> {
    cmds.iter().find_map(|c| out.get(*c).and_then(|r| r.as_ref().err()).cloned())
}

pub(super) fn grep(text: &str, pattern: &str) -> Vec<String> {
    let r = re(pattern);
    text.lines().filter(|l| r.is_match(l)).map(|l| l.trim_end().to_string()).collect()
}

pub(super) fn num(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').replace(',', "").parse().ok()
}

/// The count, with the right word.
pub(super) fn n(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/* ---------------------------------------------------------------- checks */

fn system(os: Os, out: &Outputs) -> Verdict {
    let Some(text) = first(out, &["show version"]) else {
        return Verdict::unknown("show version did not run");
    };
    let f = super::detect::parse_facts(text);
    let mut lines = Vec::new();
    if !f.uptime.is_empty() {
        lines.push(format!("Uptime: {}", f.uptime));
    }
    if !f.reload_reason.is_empty() {
        lines.push(format!("Last reload reason: {}", f.reload_reason));
    }
    let recent = re(r"^\s*(\d+)\s+(minute|hour)s?").is_match(&f.uptime) && !f.uptime.contains("day") && !f.uptime.contains("week") && !f.uptime.contains("year");
    let summary = format!(
        "{} {} on {}{}",
        f.os.unwrap_or(os).label(),
        f.version,
        if f.model.is_empty() { "unknown model".to_string() } else { f.model.clone() },
        if f.serial.is_empty() { String::new() } else { format!(", serial {}", f.serial) }
    );
    if recent {
        Verdict::new(Status::Warning, format!("Reloaded recently (up {}). {summary}", f.uptime)).evidence(lines).follow_up(&["show logging | include RELOAD|restart|crash", "show version | include reason"])
    } else {
        Verdict::new(Status::Healthy, summary).evidence(lines)
    }
}

fn cpu(os: Os, out: &Outputs) -> Verdict {
    match os {
        Os::NxOs => {
            let Some(text) = first(out, &["show system resources"]) else {
                return Verdict::unknown("show system resources did not run");
            };
            // CPU states  :  3.57% user,   2.34% kernel,   94.08% idle
            let idle = re(r"([\d.]+)% idle").captures(text).and_then(|c| num(&c[1]));
            match idle {
                Some(i) => rate_cpu(100.0 - i, "now", grep(text, "CPU|Load"), &["show processes cpu sort | head lines 15"]),
                None => Verdict::unknown("could not read the CPU line").evidence(grep(text, "CPU")),
            }
        }
        Os::Asa => {
            let Some(text) = first(out, &["show cpu usage"]) else {
                return Verdict::unknown("show cpu usage did not run");
            };
            // CPU utilization for 5 seconds = 2%; 1 minute: 2%; 5 minutes: 2%
            let five = re(r"5 minutes?[:=]\s*(\d+)%").captures(text).and_then(|c| num(&c[1]));
            match five {
                Some(v) => rate_cpu(v, "5 minute average", grep(text, "CPU"), &["show processes cpu-usage sorted non-zero"]),
                None => Verdict::unknown("could not read the CPU line").evidence(grep(text, "CPU")),
            }
        }
        _ => {
            let Some(text) = first(out, &["show processes cpu | include CPU", "show processes cpu"]) else {
                return Verdict::unknown("show processes cpu did not run");
            };
            // CPU utilization for five seconds: 7%/0%; one minute: 6%; five minutes: 6%
            let five = re(r"five minutes:\s*(\d+)%").captures(text).and_then(|c| num(&c[1]));
            let one = re(r"one minute:\s*(\d+)%").captures(text).and_then(|c| num(&c[1]));
            match (five, one) {
                (Some(f), Some(o)) => rate_cpu(f.max(o), "5 minute average", grep(text, "CPU utilization"), &["show processes cpu sorted | exclude 0.00%  0.00%  0.00%", "show processes cpu history"]),
                _ => Verdict::unknown("could not read the CPU line").evidence(grep(text, "CPU")),
            }
        }
    }
}

fn rate_cpu(value: f64, what: &str, evidence: Vec<String>, follow: &[&'static str]) -> Verdict {
    let v = Verdict::new(
        if value >= 85.0 { Status::Critical } else if value >= 70.0 { Status::Warning } else { Status::Healthy },
        format!("CPU {value:.0}% ({what})"),
    )
    .evidence(evidence);
    if value >= 70.0 { v.follow_up(follow) } else { v }
}

fn memory(os: Os, out: &Outputs) -> Verdict {
    let (total, free, evidence) = match os {
        Os::NxOs => {
            let Some(text) = first(out, &["show system resources"]) else {
                return Verdict::unknown("show system resources did not run");
            };
            // Memory usage:   16400784K total,   5812148K used,   10588636K free
            let c = re(r"(\d+)K total,\s+(\d+)K used,\s+(\d+)K free").captures(text);
            match c {
                Some(c) => (num(&c[1]), num(&c[3]), grep(text, "Memory")),
                None => return Verdict::unknown("could not read the memory line").evidence(grep(text, "Memory")),
            }
        }
        Os::Asa => {
            let Some(text) = first(out, &["show memory"]) else {
                return Verdict::unknown("show memory did not run");
            };
            let free = re(r"Free memory:\s+(\d+) bytes").captures(text).and_then(|c| num(&c[1]));
            let total = re(r"Total memory:\s+(\d+) bytes").captures(text).and_then(|c| num(&c[1]));
            (total, free, grep(text, "memory"))
        }
        _ => {
            let Some(text) = first(out, &["show processes memory | include Processor", "show processes memory sorted | include Processor", "show memory statistics"]) else {
                return Verdict::unknown("show processes memory did not run");
            };
            // Processor Pool Total:  383454200 Used:  120153948 Free:  263300252
            if let Some(c) = re(r"Processor Pool Total:\s+(\d+)\s+Used:\s+(\d+)\s+Free:\s+(\d+)").captures(text) {
                (num(&c[1]), num(&c[3]), grep(text, "Processor"))
            } else if let Some(c) = re(r"(?m)^Processor\s+\S+\s+(\d+)\s+(\d+)\s+(\d+)").captures(text) {
                // show memory statistics: Head Total(b) Used(b) Free(b)
                (num(&c[1]), num(&c[3]), grep(text, "Processor"))
            } else {
                return Verdict::unknown("could not read the processor memory line").evidence(grep(text, "Processor"));
            }
        }
    };
    match (total, free) {
        (Some(t), Some(f)) if t > 0.0 => {
            let pct_free = f / t * 100.0;
            let status = if pct_free < 10.0 { Status::Critical } else if pct_free < 20.0 { Status::Warning } else { Status::Healthy };
            let v = Verdict::new(status, format!("Memory {pct_free:.0}% free", )).evidence(evidence);
            if status != Status::Healthy { v.follow_up(&["show processes memory sorted | exclude 0 0", "show memory debug leaks"]) } else { v }
        }
        _ => Verdict::unknown("could not read memory totals").evidence(evidence),
    }
}

fn environment(kind: &str, os: Os, out: &Outputs) -> Verdict {
    let cmds: &[&str] = match os {
        Os::NxOs => &["show environment"],
        Os::Asa => &["show environment"],
        Os::SmallBusiness => &["show system"],
        _ => &["show environment all", "show env all", "show environment"],
    };
    let Some(text) = first(out, cmds) else {
        return match rejected(out, cmds) {
            Some(_) => Verdict::new(Status::Unsupported, format!("{kind} sensors: the device has no environment command")),
            None => Verdict::unknown(format!("{kind}: environment command did not run")),
        };
    };
    let section = match kind {
        "Temperature" => r"(?i)temp|thermal|inlet|outlet|hotspot",
        "Power supplies" => r"(?i)power|PS\d|psu|PWR|^\s*\d[AB]\s|built-in|^\s*\d+\s+\S+.*\b(Good|Bad|OK|Faulty|Not Present)\b",
        _ => r"(?i)fan",
    };
    // Catalyst `show env all` lists the RPS after its own "SW Status RPS Name" header;
    // an RPS that is not present is the normal case, not a missing supply
    let mut in_rps = false;
    let text: String = text.lines().filter(|l| {
        if re(r"(?i)^\s*SW\s+Status\s+RPS|^\s*RPS\b").is_match(l) { in_rps = true; return kind != "Power supplies"; }
        if re(r"(?i)^\s*SW\s+PID|^\s*(FAN|TEMPERATURE|SYSTEM)").is_match(l) { in_rps = false; }
        !(in_rps && kind == "Power supplies")
    }).collect::<Vec<_>>().join("\n");
    let text = text.as_str();
    let lines: Vec<String> = grep(text, section).into_iter().filter(|l| !l.trim().is_empty() && !re(r"(?i)^\s*SW\s+PID|^\s*-+").is_match(l)).collect();
    if lines.is_empty() {
        return Verdict::new(Status::NotCollected, format!("{kind}: nothing reported")).evidence(grep(text, ".").into_iter().take(10).collect());
    }
    let bad = re(r"(?i)\b(fault|faulty|fail|failed|critical|shutdown|not ok|red|alarm|over|absent|not present|powered-dn|off-denied|off-env-other)\b");
    let good = re(r"(?i)\b(ok|green|normal|good|present|on|powered-up|powered-on)\b");
    let bad_lines: Vec<String> = lines.iter().filter(|l| bad.is_match(l) && !l.to_lowercase().contains("threshold")).cloned().collect();
    // "not present" power supply slots are a warning, real faults fail
    let hard = bad_lines.iter().filter(|l| !re(r"(?i)not present|absent|no input power").is_match(l)).count();
    if hard > 0 {
        Verdict::new(Status::Critical, format!("{kind}: {} reporting a fault", n(hard, "sensor"))).evidence(bad_lines).follow_up(&["show environment all", "show inventory", "show logging | include ENVM|PLATFORM|THERMAL|POWER|FAN"])
    } else if !bad_lines.is_empty() {
        Verdict::new(Status::Warning, format!("{kind}: {} missing or without input", n(bad_lines.len(), "unit"))).evidence(bad_lines)
    } else if lines.iter().any(|l| good.is_match(l)) || lines.iter().any(|l| re(r"\d+\s*(C|Celsius)").is_match(l)) {
        Verdict::new(Status::Healthy, format!("{kind}: all OK")).evidence(lines.into_iter().take(12).collect())
    } else {
        Verdict::new(Status::NotCollected, format!("{kind}: could not read the status")).evidence(lines.into_iter().take(12).collect())
    }
}

fn interface_errors(_os: Os, out: &Outputs) -> Verdict {
    let Some(text) = first(out, &["show interfaces", "show interface"]) else {
        return Verdict::unknown("show interfaces did not run");
    };
    let ifaces = parse::interfaces(text);
    if ifaces.is_empty() {
        return Verdict::unknown("could not parse show interfaces");
    }
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    let mut warn = Vec::new();
    for i in &ifaces {
        if i.input_errors + i.crc + i.output_drops + i.collisions + i.input_drops + i.runts + i.giants > 0 {
            rows.push(vec![
                i.name.clone(), i.description.clone(), i.status.clone(), i.input_errors.to_string(), i.crc.to_string(), i.input_drops.to_string(),
                i.output_drops.to_string(), i.collisions.to_string(), (i.runts + i.giants).to_string(),
            ]);
            if i.crc > 0 || i.collisions > 0 || i.input_errors > 1000 {
                bad.push(format!("{}: {} input errors, {} CRC, {} collisions, {} output drops", i.name, i.input_errors, i.crc, i.collisions, i.output_drops));
            } else {
                warn.push(format!("{}: {} input errors, {} input drops, {} output drops", i.name, i.input_errors, i.input_drops, i.output_drops));
            }
        }
    }
    let table = Table {
        title: "Interfaces with errors".into(),
        columns: ["Interface", "Description", "Status", "Input errors", "CRC", "Input drops", "Output drops", "Collisions", "Runts/giants"].iter().map(|s| s.to_string()).collect(),
        rows,
    };
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{} with CRC errors, collisions or heavy input errors (bad cable, duplex mismatch or faulty SFP)", n(bad.len(), "interface")))
            .evidence(bad.into_iter().chain(warn).collect())
            .follow_up(&["show interfaces counters errors", "show interfaces status", "show controllers ethernet-controller", "clear counters  (after noting the values, to see if they grow)"])
            .table(table)
    } else if !warn.is_empty() {
        Verdict::new(Status::Warning, format!("{} with drops or input errors (check if the counters grow)", n(warn.len(), "interface")))
            .evidence(warn)
            .follow_up(&["show interfaces counters errors", "show platform hardware fed switch active qos queue stats interface <if>"])
            .table(table)
    } else {
        Verdict::new(Status::Healthy, format!("No errors or drops on {}", n(ifaces.len(), "interface")))
    }
}

fn speed_duplex(os: Os, out: &Outputs) -> Verdict {
    let cmds: &[&str] = if os == Os::NxOs { &["show interface status"] } else { &["show interfaces status"] };
    let Some(text) = first(out, cmds) else {
        return Verdict::unknown("show interfaces status did not run");
    };
    let table = parse::interface_status(text);
    let half: Vec<String> = table.rows.iter().filter(|r| r.get(3).is_some_and(|d| d.contains("half"))).map(|r| format!("{} is {} {}", r[0], r[3], r[4])).collect();
    let errdis: Vec<String> = table.rows.iter().filter(|r| r.get(2).is_some_and(|s| s.contains("err-disabled") || s.contains("errDisable"))).map(|r| format!("{} is err-disabled", r[0])).collect();
    let forced: Vec<String> = table.rows.iter().filter(|r| r.get(2).is_some_and(|s| s == "connected") && r.get(3).is_some_and(|d| d == "full" || d == "half") && r.get(4).is_some_and(|s| !s.starts_with("a-") && !s.starts_with("auto"))).map(|r| format!("{} forced to {} {}", r[0], r[3], r[4])).collect();
    let connected = table.rows.iter().filter(|r| r.get(2).is_some_and(|s| s == "connected")).count();
    if table.rows.is_empty() {
        return Verdict::unknown("could not parse show interfaces status");
    }
    let v = if !errdis.is_empty() {
        Verdict::new(Status::Critical, format!("{} err-disabled{}", n(errdis.len(), "port"), if half.is_empty() { String::new() } else { format!(", {} at half duplex", half.len()) }))
            .evidence(errdis.into_iter().chain(half).chain(forced).collect())
            .follow_up(&["show interfaces status err-disabled", "show errdisable recovery", "show logging | include err-disable|ERR_DISABLE"])
    } else if !half.is_empty() {
        Verdict::new(Status::Warning, format!("{} at half duplex (duplex mismatch likely)", n(half.len(), "port"))).evidence(half.into_iter().chain(forced).collect()).follow_up(&["show interfaces <if>", "show interfaces <if> counters errors"])
    } else if !forced.is_empty() {
        Verdict::new(Status::Healthy, format!("{connected} connected, no duplex problems; {} with speed or duplex forced", n(forced.len(), "port"))).evidence(forced)
    } else {
        Verdict::new(Status::Healthy, format!("{connected} connected, all auto-negotiated full duplex"))
    };
    v.table(table)
}

fn poe(_os: Os, out: &Outputs) -> Verdict {
    let Some(text) = first(out, &["show power inline"]) else {
        return match rejected(out, &["show power inline"]) {
            Some(_) => Verdict::new(Status::Unsupported, "No PoE on this device"),
            None => Verdict::unknown("show power inline did not run"),
        };
    };
    // Module   Available     Used     Remaining
    //   1       740.0       312.4      427.6
    let totals: Vec<(f64, f64)> = re(r"(?m)^\s*\d+\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*$").captures_iter(text).filter_map(|c| Some((num(&c[1])?, num(&c[2])?))).collect();
    let faulty = grep(text, r"(?i)\bfaulty\b|\bfault\b|power-deny|\bdenied\b");
    let table = parse::power_inline(text);
    if totals.is_empty() && table.rows.is_empty() {
        return Verdict::unknown("could not parse show power inline").evidence(grep(text, ".").into_iter().take(8).collect());
    }
    let (avail, used) = totals.iter().fold((0.0, 0.0), |a, t| (a.0 + t.0, a.1 + t.1));
    let pct = if avail > 0.0 { used / avail * 100.0 } else { 0.0 };
    let summary = format!("PoE {used:.0} W of {avail:.0} W used ({pct:.0}%)");
    let v = if !faulty.is_empty() {
        Verdict::new(Status::Critical, format!("{} PoE {} faulty or denied. {summary}", faulty.len(), if faulty.len() == 1 { "port" } else { "ports" })).evidence(faulty).follow_up(&["show power inline <if> detail", "show logging | include ILPOWER|POE"])
    } else if pct >= 90.0 {
        Verdict::new(Status::Warning, format!("{summary}, close to the budget")).follow_up(&["show power inline | include on", "show power inline priority"])
    } else {
        Verdict::new(Status::Healthy, summary)
    };
    v.table(table)
}

fn vlans_trunks(os: Os, out: &Outputs) -> Verdict {
    let trunk_cmd = if os == Os::NxOs { "show interface trunk" } else { "show interfaces trunk" };
    let trunk = first(out, &[trunk_cmd]);
    let vlan = first(out, &["show vlan brief"]);
    if trunk.is_none() && vlan.is_none() {
        return match rejected(out, &[trunk_cmd, "show vlan brief"]) {
            Some(_) => Verdict::new(Status::Unsupported, "No VLAN or trunk commands on this device"),
            None => Verdict::unknown("VLAN and trunk commands did not run"),
        };
    }
    let vlan_count = vlan.map(|t| re(r"(?m)^(\d+)\s+\S+\s+(active|act/unsup|suspended)").captures_iter(t).count()).unwrap_or(0);
    let trunks = trunk.map(parse::trunks).unwrap_or_default();
    let mut notes = Vec::new();
    let mut warn = Vec::new();
    for t in &trunks {
        notes.push(format!("{}: mode {}, native VLAN {}, allowed {}", t.port, t.mode, t.native, t.allowed));
        if t.native == "1" {
            warn.push(format!("{} uses native VLAN 1 (set a dedicated native VLAN)", t.port));
        }
        if t.allowed == "1-4094" || t.allowed == "ALL" || t.allowed.to_lowercase() == "all" {
            warn.push(format!("{} allows every VLAN (prune to the ones needed)", t.port));
        }
    }
    let table = Table {
        title: "Trunks".into(),
        columns: ["Port", "Mode", "Encapsulation", "Native VLAN", "Allowed VLANs"].iter().map(|s| s.to_string()).collect(),
        rows: trunks.iter().map(|t| vec![t.port.clone(), t.mode.clone(), t.encap.clone(), t.native.clone(), t.allowed.clone()]).collect(),
    };
    let summary = format!("{} active, {}", n(vlan_count, "VLAN"), n(trunks.len(), "trunk"));
    if !warn.is_empty() {
        Verdict::new(Status::Warning, format!("{summary}; {} worth reviewing", n(warn.len(), "trunk setting"))).evidence(warn.into_iter().chain(notes).collect()).follow_up(&["show interfaces trunk", "show vlan brief", "show spanning-tree interface <if>"]).table(table)
    } else {
        Verdict::new(Status::Healthy, summary).evidence(notes).table(table)
    }
}

fn port_channels(os: Os, out: &Outputs) -> Verdict {
    let cmd = if os == Os::NxOs { "show port-channel summary" } else { "show etherchannel summary" };
    let Some(text) = first(out, &[cmd]) else {
        return match rejected(out, &[cmd]) {
            Some(_) => Verdict::new(Status::Unsupported, "No port channels on this device"),
            None => Verdict::unknown(format!("{cmd} did not run")),
        };
    };
    // 1      Po1(SU)         LACP      Gi1/0/49(P)    Gi1/0/50(D)
    let group = re(r"(?m)^\s*\d+\s+(Po\d+)\(([A-Za-z]+)\)[ \t]*(\S*)[ \t]*(.*)$");
    let member = re(r"(\S+?)\(([A-Za-z]+)\)");
    let mut rows = Vec::new();
    let mut bad = Vec::new();
    let mut warn = Vec::new();
    for c in group.captures_iter(text) {
        let (po, flags, proto, members) = (&c[1], &c[2], &c[3], &c[4]);
        let mut member_list = Vec::new();
        for m in member.captures_iter(members) {
            member_list.push(format!("{}({})", &m[1], &m[2]));
            match &m[2] {
                "P" => {}
                "D" => bad.push(format!("{po}: member {} is down", &m[1])),
                "s" | "I" | "H" | "w" => warn.push(format!("{po}: member {} is {}", &m[1], match &m[2] { "s" => "suspended", "I" => "stand-alone (individual)", "H" => "hot-standby", _ => "waiting" })),
                _ => {}
            }
        }
        if flags.contains('D') && !flags.contains('U') {
            bad.push(format!("{po} is down ({flags})"));
        } else if flags.contains('M') {
            warn.push(format!("{po} has not bundled all members ({flags})"));
        }
        rows.push(vec![po.to_string(), flags.to_string(), proto.to_string(), member_list.join(" ")]);
    }
    if rows.is_empty() {
        if re(r"(?i)Number of channel-groups in use:\s*0|Number of aggregators:\s*0").is_match(text) || text.trim().is_empty() {
            return Verdict::new(Status::NotApplicable, "No port channels configured");
        }
        return Verdict::unknown("could not read the port channel rows").evidence(grep(text, r"Po\d+").into_iter().take(8).collect());
    }
    let table = Table { title: "Port channels".into(), columns: ["Channel", "Flags", "Protocol", "Members"].iter().map(|s| s.to_string()).collect(), rows };
    let summary = format!("{}", n(table.rows.len(), "port channel"));
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{summary}; {} down", n(bad.len(), "member or channel"))).evidence(bad.into_iter().chain(warn).collect()).follow_up(&["show etherchannel <n> detail", "show lacp neighbor", "show interfaces <if> status"]).table(table)
    } else if !warn.is_empty() {
        Verdict::new(Status::Warning, format!("{summary}; {} not bundled", n(warn.len(), "member"))).evidence(warn).follow_up(&["show etherchannel <n> detail", "show lacp counters"]).table(table)
    } else {
        Verdict::new(Status::Healthy, format!("{summary}, all members bundled")).table(table)
    }
}

const DETAIL_CMD: &str = "show spanning-tree detail | include changes|occurred|from|^ Port |Designated port id|Designated bridge has";

/// (designated port, backup port) pairs: ports of this switch cabled to each other.
fn self_loops(summary: &str, detail: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // Port 13 (FastEthernet0/13) of VLAN0001 is backup blocking
    //    Designated port id is 128.1, designated path cost 19
    let mut names: Vec<(u32, String)> = Vec::new();
    for c in re(r"(?m)^\s*Port (\d+) \((\S+)\) of \S+ is").captures_iter(detail) {
        names.push((c[1].parse().unwrap_or(0), c[2].to_string()));
    }
    let mut current: Option<(String, bool)> = None;
    for line in detail.lines() {
        if let Some(c) = re(r"^\s*Port \d+ \((\S+)\) of \S+ is (\S+)").captures(line) {
            current = Some((c[1].to_string(), &c[2] == "backup"));
        } else if let Some(c) = re(r"Designated port id is \d+\.(\d+)").captures(line) {
            if let Some((port, true)) = &current {
                let id: u32 = c[1].parse().unwrap_or(0);
                if let Some((_, partner)) = names.iter().find(|(num, _)| *num == id) {
                    out.push((short_if(partner), short_if(port)));
                }
            }
        }
    }
    // without the detail: any Backup-role port is still a self-loop
    for c in re(r"(?m)^\s*(\S+)\s+Back\s+(BLK|BKN|LRN|LIS)").captures_iter(summary) {
        let port = short_if(&c[1]);
        if !out.iter().any(|(_, b)| *b == port) {
            out.push(("another local port".into(), port));
        }
    }
    out
}

fn short_if(name: &str) -> String {
    name.replace("FastEthernet", "Fa").replace("GigabitEthernet", "Gi").replace("TenGigabitEthernet", "Te").replace("TwentyFiveGigE", "Twe").replace("FortyGigabitEthernet", "Fo").replace("HundredGigE", "Hu").replace("Port-channel", "Po")
}

fn spanning_tree(_os: Os, out: &Outputs) -> Verdict {
    let Some(text) = first(out, &["show spanning-tree"]) else {
        return match rejected(out, &["show spanning-tree"]) {
            Some(_) => Verdict::new(Status::Unsupported, "No spanning tree on this device"),
            None => Verdict::unknown("show spanning-tree did not run"),
        };
    };
    let vlans = re(r"(?m)^(VLAN\d+|MST\d+)").captures_iter(text).count();
    let root_here = re(r"(?m)This bridge is the root").find_iter(text).count();
    let blocking = grep(text, r"(?m)^\s*\S+\s+(Altn|Back|Desg|Root)\s+(BLK|BKN)");
    let broken = grep(text, r"BKN|\*(ROOT|LOOP|TYPE|PVID|BA)_Inc");
    // A port in the Backup role hears this switch's own BPDUs: two of its
    // ports are on the same segment, which is a cabling loop that STP is
    // holding back. The designated port id in the detail says which port.
    let detail_text = first(out, &[DETAIL_CMD]).unwrap_or("");
    let loops = self_loops(text, detail_text);
    let inconsistent = first(out, &["show spanning-tree inconsistentports"]).map(|t| grep(t, r"(?m)^(VLAN|MST)\d+\s+\S+\s+")).unwrap_or_default();
    let detail = first(out, &[DETAIL_CMD]);
    let mut recent = Vec::new();
    let mut churn = Vec::new();
    if let Some(d) = detail {
        // Number of topology changes 14 last change occurred 00:02:11 ago
        for c in re(r"(?m)Number of topology changes (\d+) last change occurred ([\d:dwy]+) ago").captures_iter(d) {
            let (count, age) = (num(&c[1]).unwrap_or(0.0), &c[2]);
            let is_recent = re(r"^00:(0\d|1\d):\d\d$").is_match(age);
            if is_recent {
                recent.push(format!("topology change {} ago", age));
            }
            if count >= 10.0 && is_recent {
                churn.push(format!("{} topology changes, the last {} ago", count as u64, age));
            }
        }
    }
    let from = detail.map(|d| grep(d, r"from \S+")).unwrap_or_default();
    let summary = format!("{} with spanning tree, root for {root_here}, {} blocking", n(vlans, "instance"), n(blocking.len(), "port"));
    if !loops.is_empty() {
        let mut ev = loops.iter().map(|(a, b)| format!("Loop: {a} and {b} are on the same segment (Backup role = this switch hears its own BPDUs on {b})")).collect::<Vec<_>>();
        ev.extend(blocking.iter().cloned());
        ev.extend(churn.iter().cloned());
        ev.extend(from.iter().cloned());
        return Verdict::new(Status::Critical, format!("Cabling loop between {}: spanning tree is holding it back", loops.iter().map(|(a, b)| format!("{a} and {b}")).collect::<Vec<_>>().join(", ")))
            .evidence(ev)
            .follow_up(&["show spanning-tree interface <if> detail", "show cdp neighbors <if>", "show interfaces <if> status", "show logging | include LINK|SPANTREE"]);
    }
    if !broken.is_empty() || !inconsistent.is_empty() {
        Verdict::new(Status::Critical, format!("{summary}; {} inconsistent or broken", n(broken.len() + inconsistent.len(), "port"))).evidence(broken.into_iter().chain(inconsistent).chain(from).collect()).follow_up(&["show spanning-tree inconsistentports", "show spanning-tree interface <if> detail", "show logging | include SPANTREE"])
    } else if !churn.is_empty() {
        Verdict::new(Status::Warning, format!("{summary}; many topology changes, the last one recent")).evidence(churn.into_iter().chain(from).chain(blocking).collect()).follow_up(&["show spanning-tree detail | include changes|occurred|from", "show spanning-tree interface <if> detail", "show logging | include LINK|SPANTREE"])
    } else if !recent.is_empty() {
        Verdict::new(Status::Warning, format!("{summary}; topology changed in the last 20 minutes")).evidence(recent.into_iter().chain(from).collect()).follow_up(&["show spanning-tree detail | include changes|occurred|from|^ Port |Designated port id|Designated bridge has", "show spanning-tree interface <if> detail"])
    } else if vlans == 0 {
        Verdict::new(Status::NotCollected, "could not parse show spanning-tree").evidence(grep(text, ".").into_iter().take(8).collect())
    } else {
        Verdict::new(Status::Healthy, summary).evidence(blocking)
    }
}

fn routing(os: Os, out: &Outputs) -> Verdict {
    let (ospf_cmd, bgp_cmd, eigrp_cmd) = if os == Os::NxOs {
        ("show ip ospf neighbors", "show ip bgp summary", "show ip eigrp neighbors")
    } else {
        ("show ip ospf neighbor", "show ip bgp summary", "show ip eigrp neighbors")
    };
    let mut lines = Vec::new();
    let mut bad = Vec::new();
    let mut warn = Vec::new();
    let mut protocols = 0;
    if let Some(t) = first(out, &[ospf_cmd]) {
        let nbrs: Vec<_> = re(r"(?m)^(\d+\.\d+\.\d+\.\d+)\s+(\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)").captures_iter(t).collect();
        if !nbrs.is_empty() {
            protocols += 1;
            for c in &nbrs {
                let state = c[3].split('/').next().unwrap_or("");
                lines.push(format!("OSPF {} state {} on {}", &c[1], &c[3], &c[6]));
                if !matches!(state, "FULL" | "2WAY") {
                    warn.push(format!("OSPF neighbor {} is {} on {}", &c[1], &c[3], &c[6]));
                }
            }
        }
    }
    if let Some(t) = first(out, &[bgp_cmd]) {
        // Neighbor  V  AS  MsgRcvd MsgSent TblVer InQ OutQ Up/Down  State/PfxRcd
        let peers: Vec<_> = re(r"(?m)^(\*?\d+\.\d+\.\d+\.\d+|[0-9a-fA-F:]+)\s+4\s+(\d+)\s+\d+\s+\d+\s+\d+\s+\d+\s+\d+\s+(\S+)\s+(\S+)").captures_iter(t).collect();
        if !peers.is_empty() {
            protocols += 1;
            for c in &peers {
                lines.push(format!("BGP {} AS {} {} {}", &c[1], &c[2], &c[3], &c[4]));
                if num(&c[4]).is_none() {
                    bad.push(format!("BGP peer {} (AS {}) is {}", &c[1], &c[2], &c[4]));
                }
            }
        }
    }
    if let Some(t) = first(out, &[eigrp_cmd]) {
        let nbrs = re(r"(?m)^\s*\d+\s+(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+(\d+)\s+(\S+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\d+)").captures_iter(t).count();
        if nbrs > 0 {
            protocols += 1;
            lines.push(format!("EIGRP: {}", n(nbrs, "neighbor")));
            for c in re(r"(?m)^\s*\d+\s+(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+\d+\s+\S+\s+\d+\s+\d+\s+\d+\s+(\d+)").captures_iter(t) {
                if num(&c[3]).is_some_and(|q| q > 0.0) {
                    warn.push(format!("EIGRP neighbor {} on {} has {} packets queued", &c[1], &c[2], &c[3]));
                }
            }
        }
    }
    if protocols == 0 {
        return Verdict::new(Status::Unsupported, "No OSPF, BGP or EIGRP neighbors (a layer 2 switch, or static routing)");
    }
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{} not established", n(bad.len(), "routing neighbor"))).evidence(bad.into_iter().chain(warn).chain(lines).collect()).follow_up(&["show ip bgp neighbors <peer>", "show ip ospf interface brief", "show logging | include BGP|OSPF|EIGRP|ADJCHG"])
    } else if !warn.is_empty() {
        Verdict::new(Status::Warning, format!("{} in a transitional state", n(warn.len(), "routing neighbor"))).evidence(warn.into_iter().chain(lines).collect()).follow_up(&["show ip ospf neighbor detail", "show ip ospf interface <if>"])
    } else {
        Verdict::new(Status::Healthy, format!("{} up", n(lines.len(), "routing neighbor"))).evidence(lines)
    }
}

fn stack(os: Os, out: &Outputs) -> Verdict {
    if os == Os::NxOs || os == Os::Asa {
        return Verdict::new(Status::Unsupported, "Not a stackable platform");
    }
    let Some(text) = first(out, &["show switch"]) else {
        return match rejected(out, &["show switch"]) {
            Some(_) => Verdict::new(Status::Unsupported, "Not a stackable platform"),
            None => Verdict::unknown("show switch did not run"),
        };
    };
    // *1       Active   bcd1.2345.6789     15     V01     Ready
    let members: Vec<_> = re(r"(?m)^\s*\*?(\d+)\s+(Active|Standby|Member|Master|Slave)\s+(\S+)\s+(\d+)\s+(\S+)\s+(\S.*?)\s*$").captures_iter(text).collect();
    if members.is_empty() {
        return Verdict::new(Status::Unsupported, "Not a stack (single switch)");
    }
    let mut bad = Vec::new();
    let mut lines = Vec::new();
    for c in &members {
        lines.push(format!("Switch {} {} priority {} {} {}", &c[1], &c[2], &c[4], &c[5], &c[6]));
        if !c[6].starts_with("Ready") {
            bad.push(format!("Switch {} is {}", &c[1], &c[6]));
        }
    }
    let ports = first(out, &["show switch stack-ports"]).map(|t| grep(t, r"(?m)^\s*\d+\s+(\S+)\s+(\S+)")).unwrap_or_default();
    let down_ports: Vec<String> = ports.iter().filter(|l| l.contains("DOWN") || l.contains("Down")).cloned().collect();
    let has_standby = members.iter().any(|c| &c[2] == "Standby");
    let summary = format!("{} in the stack{}", n(members.len(), "switch"), if members.len() > 1 && !has_standby { ", no standby" } else { "" });
    if !bad.is_empty() {
        Verdict::new(Status::Critical, format!("{summary}; {} not ready", n(bad.len(), "member"))).evidence(bad.into_iter().chain(down_ports).chain(lines).collect()).follow_up(&["show switch detail", "show switch stack-ports summary", "show logging | include STACKMGR"])
    } else if !down_ports.is_empty() || (members.len() > 1 && !has_standby) {
        Verdict::new(Status::Warning, format!("{summary}; {} down", n(down_ports.len(), "stack port"))).evidence(down_ports.into_iter().chain(lines).collect()).follow_up(&["show switch stack-ports summary", "show switch detail"])
    } else {
        Verdict::new(Status::Healthy, format!("{summary}, all ready")).evidence(lines)
    }
}

fn redundancy(os: Os, out: &Outputs) -> Verdict {
    if os == Os::Asa {
        let Some(text) = first(out, &["show failover"]) else {
            return Verdict::new(Status::Unsupported, "Failover not configured");
        };
        if text.contains("Failover Off") {
            return Verdict::new(Status::Unsupported, "Failover is off");
        }
        let bad = grep(text, r"(?i)Failed|Not Monitored|Unknown|Cold Standby|Sync Config");
        return if bad.is_empty() {
            Verdict::new(Status::Healthy, "Failover pair healthy").evidence(grep(text, r"This host|Other host|Active|Standby"))
        } else {
            Verdict::new(Status::Critical, "Failover reports a problem").evidence(bad).follow_up(&["show failover state", "show failover history"])
        };
    }
    let Some(text) = first(out, &["show redundancy"]) else {
        return Verdict::new(Status::Unsupported, "No redundant supervisors");
    };
    if text.contains("Simplex") || text.contains("Non-redundant") || text.contains("Not Redundant") {
        return Verdict::new(Status::Unsupported, "Single supervisor");
    }
    let peer = re(r"(?m)Peer Processor Information[\s\S]*?Current Software state = (.+)$").captures(text).map(|c| c[1].trim().to_string());
    match peer.as_deref() {
        Some("STANDBY HOT") => Verdict::new(Status::Healthy, "Redundancy: standby is HOT").evidence(grep(text, "state|Mode")),
        Some(s) => Verdict::new(Status::Warning, format!("Redundancy: peer is {s}")).evidence(grep(text, "state|Mode|Peer")).follow_up(&["show redundancy states", "show platform", "show logging | include REDUNDANCY|RF"]),
        None => Verdict::new(Status::NotCollected, "could not read the peer state").evidence(grep(text, "state")),
    }
}

fn logs(os: Os, out: &Outputs) -> Verdict {
    let cmd = match os {
        Os::NxOs => "show logging last 100",
        Os::Asa => "show logging | include %ASA-[0-3]-",
        _ => "show logging | include -[0-3]-",
    };
    let Some(text) = first(out, &[cmd]) else {
        return Verdict::unknown("show logging did not run");
    };
    let severe = grep(text, r"%[A-Z0-9_]+(?:-[A-Z0-9_]+)*-[0-2]-");
    let errors = grep(text, r"%[A-Z0-9_]+(?:-[A-Z0-9_]+)*-3-");
    let recent_errors: Vec<String> = errors.iter().rev().take(10).cloned().collect();
    let paired = flapping_together(text);
    if !paired.is_empty() {
        return Verdict::new(Status::Critical, format!("{} change state at the same moments: likely cabled to each other (loop) or sharing a failing device", paired.iter().map(|(a, b, _)| format!("{a} and {b}")).collect::<Vec<_>>().join(", ")))
            .evidence(paired.iter().map(|(a, b, times)| format!("{a} and {b} went down/up together {} ({})", n(*times, "time"), "same timestamp")).chain(errors.iter().rev().take(10).cloned()).collect())
            .follow_up(&["show spanning-tree", "show spanning-tree detail | include changes|occurred|from", "show cdp neighbors", "show interfaces status"]);
    }
    if !severe.is_empty() {
        Verdict::new(Status::Critical, format!("{} of severity critical or worse in the log", n(severe.len(), "message"))).evidence(severe.into_iter().rev().take(15).collect()).follow_up(&["show logging | include -[0-2]-", "show logging | include Traceback|crash", "show tech-support (if escalating to Cisco TAC)"])
    } else if !errors.is_empty() {
        Verdict::new(Status::Warning, format!("{} of severity error in the log", n(errors.len(), "message"))).evidence(recent_errors).follow_up(&["show logging | include -3-", "show logging | include <mnemonic>"])
    } else {
        Verdict::new(Status::Healthy, "No critical or error messages in the log")
    }
}

/// Pairs of local interfaces whose LINK up/down messages share a timestamp
/// at least twice: (a, b, times).
fn flapping_together(log: &str) -> Vec<(String, String, usize)> {
    // Oct 14 10:00:01.123: %LINK-3-UPDOWN: Interface FastEthernet0/1, changed state to down
    let line = re(r"(?m)^\S*\s*(\w{3}\s+\d+\s+\d{2}:\d{2}:\d{2}|\d{4}\s+\w{3}\s+\d+\s+\d{2}:\d{2}:\d{2}|\d+:\d{2}:\d{2}).*%LINK-3-UPDOWN: Interface (\S+?), changed state to (\w+)");
    let mut by_time: std::collections::BTreeMap<(String, String), Vec<String>> = std::collections::BTreeMap::new();
    for c in line.captures_iter(log) {
        let ifname = short_if(&c[2]);
        let entry = by_time.entry((c[1].to_string(), c[3].to_string())).or_default();
        if !entry.contains(&ifname) {
            entry.push(ifname);
        }
    }
    let mut pairs: std::collections::BTreeMap<(String, String), usize> = std::collections::BTreeMap::new();
    for (_, ifs) in by_time {
        if ifs.len() == 2 {
            let mut k = ifs.clone();
            k.sort();
            *pairs.entry((k[0].clone(), k[1].clone())).or_default() += 1;
        }
    }
    pairs.into_iter().filter(|(_, t)| *t >= 2).map(|((a, b), t)| (a, b, t)).collect()
}

fn neighbors(os: Os, out: &Outputs) -> Verdict {
    let cdp = first(out, &["show cdp neighbors detail"]);
    let lldp = first(out, &["show lldp neighbors detail"]);
    let mut table = Table { title: "Neighbors (CDP/LLDP)".into(), columns: ["Local port", "Neighbor", "Remote port", "Platform", "IP address", "Via"].iter().map(|s| s.to_string()).collect(), rows: Vec::new() };
    if let Some(t) = cdp {
        table.rows.extend(parse::cdp_neighbors(t).into_iter().map(|n| vec![n.local, n.device, n.remote, n.platform, n.ip, "CDP".into()]));
    }
    if let Some(t) = lldp {
        for nb in parse::lldp_neighbors(t) {
            if !table.rows.iter().any(|r| r[0] == nb.local && r[1] == nb.device) {
                table.rows.push(vec![nb.local, nb.device, nb.remote, nb.platform, nb.ip, "LLDP".into()]);
            }
        }
    }
    let _ = os;
    if cdp.is_none() && lldp.is_none() {
        return Verdict::new(Status::Unsupported, "CDP and LLDP are not available");
    }
    let count = table.rows.len();
    Verdict::new(Status::Healthy, format!("{} seen", n(count, "neighbor"))).table(table)
}

fn table_check(title: &str, cmds: &[&str], out: &Outputs, build: fn(&str) -> Table) -> Verdict {
    let Some(text) = first(out, cmds) else {
        return match rejected(out, cmds) {
            Some(_) => Verdict::new(Status::Unsupported, format!("{title}: not available on this device")),
            None => Verdict::unknown(format!("{title}: command did not run")),
        };
    };
    let table = build(text);
    let count = table.rows.len();
    Verdict::new(Status::Healthy, format!("{title}: {}", n(count, "entry"))).table(table)
}

/* -------------------------------------------------------------- catalog */

pub fn catalog() -> Vec<CheckDef> {
    vec![
        CheckDef { id: "system", quick: true, title: "System and uptime", category: "System", commands: |_| vec!["show version"], evaluate: system },
        CheckDef {
            id: "cpu", quick: true, title: "CPU", category: "System",
            commands: |os| match os { Os::NxOs => vec!["show system resources"], Os::Asa => vec!["show cpu usage"], Os::SmallBusiness => vec!["show cpu utilization"], _ => vec!["show processes cpu | include CPU"] },
            evaluate: cpu,
        },
        CheckDef {
            id: "memory", quick: true, title: "Memory", category: "System",
            commands: |os| match os { Os::NxOs => vec!["show system resources"], Os::Asa => vec!["show memory"], Os::SmallBusiness => vec![], _ => vec!["show processes memory | include Processor"] },
            evaluate: memory,
        },
        CheckDef { id: "temperature", quick: true, title: "Temperature", category: "Hardware", commands: env_cmds, evaluate: |os, out| environment("Temperature", os, out) },
        CheckDef { id: "power", quick: true, title: "Power supplies", category: "Hardware", commands: env_cmds, evaluate: |os, out| environment("Power supplies", os, out) },
        CheckDef { id: "fans", quick: true, title: "Fans", category: "Hardware", commands: env_cmds, evaluate: |os, out| environment("Fans", os, out) },
        CheckDef { id: "stack", quick: true, title: "Stack", category: "Hardware", commands: |os| match os { Os::Ios | Os::IosXe => vec!["show switch", "show switch stack-ports"], _ => vec![] }, evaluate: stack },
        CheckDef { id: "redundancy", quick: true, title: "Redundancy", category: "Hardware", commands: |os| match os { Os::Asa => vec!["show failover"], Os::IosXe | Os::Ios | Os::NxOs | Os::IosXr => vec!["show redundancy"], _ => vec![] }, evaluate: redundancy },
        CheckDef { id: "interface-errors", quick: true, title: "Interface errors and discards", category: "Interfaces", commands: |os| if os == Os::NxOs { vec!["show interface"] } else { vec!["show interfaces"] }, evaluate: interface_errors },
        CheckDef { id: "speed-duplex", quick: false, title: "Speed, duplex and err-disabled", category: "Interfaces", commands: |os| if os == Os::NxOs { vec!["show interface status"] } else { vec!["show interfaces status"] }, evaluate: speed_duplex },
        CheckDef { id: "poe", quick: false, title: "Power over Ethernet", category: "Interfaces", commands: |os| match os { Os::NxOs | Os::Asa | Os::IosXr => vec![], _ => vec!["show power inline"] }, evaluate: poe },
        CheckDef { id: "vlans", quick: false, title: "VLANs and trunks", category: "Layer 2", commands: |os| match os { Os::NxOs => vec!["show vlan brief", "show interface trunk"], Os::Asa | Os::IosXr => vec![], _ => vec!["show vlan brief", "show interfaces trunk"] }, evaluate: vlans_trunks },
        CheckDef { id: "port-channels", quick: false, title: "Port channels", category: "Layer 2", commands: |os| match os { Os::NxOs => vec!["show port-channel summary"], Os::Asa | Os::IosXr => vec![], _ => vec!["show etherchannel summary"] }, evaluate: port_channels },
        CheckDef { id: "spanning-tree", quick: false, title: "Spanning tree", category: "Layer 2", commands: |os| match os { Os::Asa | Os::IosXr => vec![], _ => vec!["show spanning-tree", "show spanning-tree inconsistentports", "show spanning-tree detail | include changes|occurred|from|^ Port |Designated port id|Designated bridge has"] }, evaluate: spanning_tree },
        CheckDef { id: "routing", quick: false, title: "Routing neighbors", category: "Layer 3", commands: |os| match os { Os::NxOs => vec!["show ip ospf neighbors", "show ip bgp summary", "show ip eigrp neighbors"], Os::SmallBusiness => vec![], _ => vec!["show ip ospf neighbor", "show ip bgp summary", "show ip eigrp neighbors"] }, evaluate: routing },
        CheckDef { id: "logs", quick: true, title: "Recent log messages", category: "System", commands: |os| match os { Os::NxOs => vec!["show logging last 100"], Os::Asa => vec!["show logging | include %ASA-[0-3]-"], _ => vec!["show logging | include -[0-3]-"] }, evaluate: logs },
        CheckDef { id: "neighbors", quick: false, title: "CDP and LLDP neighbors", category: "Tables", commands: |os| match os { Os::Asa => vec![], _ => vec!["show cdp neighbors detail", "show lldp neighbors detail"] }, evaluate: neighbors },
        CheckDef { id: "ip-interfaces", quick: false, title: "IP interfaces", category: "Tables", commands: |os| match os { Os::Asa => vec!["show interface ip brief"], _ => vec!["show ip interface brief"] }, evaluate: |_, out| table_check("IP interfaces", &["show ip interface brief", "show interface ip brief"], out, parse::ip_interface_brief) },
        CheckDef { id: "mac-table", quick: false, title: "MAC address table", category: "Tables", commands: |os| match os { Os::Asa | Os::IosXr => vec![], _ => vec!["show mac address-table"] }, evaluate: |_, out| table_check("MAC address table", &["show mac address-table"], out, parse::mac_table) },
        CheckDef { id: "arp", quick: false, title: "ARP table", category: "Tables", commands: |os| match os { Os::Asa => vec!["show arp"], _ => vec!["show ip arp"] }, evaluate: |_, out| table_check("ARP table", &["show ip arp", "show arp"], out, parse::arp_table) },
        // checks evaluated the same way on every platform
        CheckDef { id: "time-sync", quick: true, title: "Time synchronization", category: "System", commands: |os| generic::commands(os, "time-sync"), evaluate: |os, out| generic::evaluate(os, "time-sync", out) },
        CheckDef { id: "storage", quick: true, title: "Storage", category: "System", commands: |os| generic::commands(os, "storage"), evaluate: |os, out| generic::evaluate(os, "storage", out) },
        CheckDef { id: "optics", quick: false, title: "Optics", category: "Interfaces", commands: |os| generic::commands(os, "optics"), evaluate: |os, out| generic::evaluate(os, "optics", out) },
        CheckDef { id: "uplinks", quick: true, title: "Uplinks", category: "Interfaces", commands: |os| generic::commands(os, "uplinks"), evaluate: |os, out| generic::evaluate(os, "uplinks", out) },
        CheckDef { id: "overlay", quick: false, title: "VXLAN / EVPN", category: "Layer 3", commands: |os| generic::commands(os, "overlay"), evaluate: |os, out| generic::evaluate(os, "overlay", out) },
        // ISE appliances
        CheckDef { id: "services", quick: true, title: "Application services", category: "Appliance", commands: |os| generic::commands(os, "services"), evaluate: |os, out| generic::evaluate(os, "services", out) },
        CheckDef { id: "backups", quick: true, title: "Backup history", category: "Appliance", commands: |os| generic::commands(os, "backups"), evaluate: |os, out| generic::evaluate(os, "backups", out) },
        // wireless controllers (Catalyst 9800 and AireOS)
        CheckDef { id: "ap-inventory", quick: true, title: "Access points", category: "Wireless", commands: |os| generic::commands(os, "ap-inventory"), evaluate: |os, out| generic::evaluate(os, "ap-inventory", out) },
        CheckDef { id: "ap-join", quick: true, title: "AP join failures", category: "Wireless", commands: |os| generic::commands(os, "ap-join"), evaluate: |os, out| generic::evaluate(os, "ap-join", out) },
        CheckDef { id: "ap-images", quick: false, title: "AP images", category: "Wireless", commands: |os| generic::commands(os, "ap-images"), evaluate: |os, out| generic::evaluate(os, "ap-images", out) },
        CheckDef { id: "wlans", quick: false, title: "WLANs and tags", category: "Wireless", commands: |os| generic::commands(os, "wlans"), evaluate: |os, out| generic::evaluate(os, "wlans", out) },
        CheckDef { id: "clients", quick: true, title: "Clients and authentication", category: "Wireless", commands: |os| generic::commands(os, "clients"), evaluate: |os, out| generic::evaluate(os, "clients", out) },
        CheckDef { id: "rf", quick: false, title: "RF health", category: "Wireless", commands: |os| generic::commands(os, "rf"), evaluate: |os, out| generic::evaluate(os, "rf", out) },
    ]
}

fn env_cmds(os: Os) -> Vec<&'static str> {
    match os {
        Os::NxOs | Os::Asa => vec!["show environment"],
        Os::SmallBusiness => vec!["show system"],
        Os::IosXe => vec!["show environment all"],
        Os::Ios => vec!["show env all"],
        _ => vec!["show environment"],
    }
}

/// The checks a run covers: the explicit ids, else the tier.
pub fn select(ids: &[String], tier: Option<Tier>) -> Vec<CheckDef> {
    catalog()
        .into_iter()
        .filter(|d| if !ids.is_empty() { ids.iter().any(|i| i == d.id) } else { tier != Some(Tier::Quick) || d.quick })
        .collect()
}

pub fn commands_of(d: &CheckDef, os: Os) -> Vec<&'static str> {
    if os.is_cisco() && !generic::OWN_IDS.contains(&d.id) { (d.commands)(os) } else { generic::commands(os, d.id) }
}

/// Every command the selected checks need, in order, without duplicates.
pub fn commands_for(os: Os, ids: &[String], tier: Option<Tier>) -> Vec<String> {
    let mut cmds: Vec<String> = Vec::new();
    for def in select(ids, tier) {
        for c in commands_of(&def, os) {
            if !cmds.iter().any(|x| x == c) {
                cmds.push(c.to_string());
            }
        }
    }
    cmds
}

pub fn evaluate(os: Os, ids: &[String], tier: Option<Tier>, out: &Outputs) -> Vec<CheckResult> {
    let now = chrono::Utc::now().timestamp();
    select(ids, tier)
        .iter()
        .map(|d| {
            let commands: Vec<String> = commands_of(d, os).iter().map(|s| s.to_string()).collect();
            let mut v = if commands.is_empty() {
                Verdict::new(Status::Unsupported, format!("{} cannot be checked on {}", d.title, os.label()))
            } else if commands.iter().all(|c| matches!(out.get(c), Some(Err(_)) | None)) {
                let errors: Vec<String> = commands.iter().filter_map(|c| out.get(c).and_then(|r| r.as_ref().err()).cloned()).collect();
                let all_rejected = !errors.is_empty() && errors.iter().all(|e| e.contains("rejected"));
                // a feature the device does not have (no stack, no PoE, no optics) is
                // not a collection failure; anything else that failed is not collected
                let optional = matches!(d.id, "stack" | "redundancy" | "poe" | "optics" | "port-channels" | "spanning-tree" | "routing" | "overlay" | "neighbors" | "uplinks" | "vlans" | "mac-table" | "ap-inventory" | "ap-join" | "ap-images" | "wlans" | "clients" | "rf" | "services" | "backups");
                if all_rejected && optional {
                    Verdict::new(Status::NotApplicable, format!("{}: the device has no such command", d.title)).evidence(errors)
                } else {
                    let why = errors.first().cloned().unwrap_or_else(|| "the commands did not run".into());
                    Verdict::new(Status::NotCollected, format!("Not collected: {why}"))
                }
            } else if os.is_cisco() && !generic::OWN_IDS.contains(&d.id) {
                (d.evaluate)(os, out)
            } else {
                generic::evaluate(os, d.id, out)
            };
            // a verdict without evidence from a command that printed nothing is not a healthy result
            if v.status == Status::Healthy && v.evidence.is_empty() && v.table.is_none() && commands.iter().all(|c| out.get(c).and_then(|r| r.as_ref().ok()).map(|o| o.trim().is_empty()).unwrap_or(true)) {
                v = Verdict::unknown("the device returned no output");
            }
            let next_step = match v.status {
                Status::Healthy => String::new(),
                Status::NotApplicable | Status::Unsupported => String::new(),
                Status::NotCollected => v.follow_up.first().map(|c| format!("Run `{c}` by hand and read the output")).unwrap_or_else(|| "Run the command by hand; check the platform selection if it was rejected".into()),
                _ => v.follow_up.first().map(|c| format!("Run `{c}`")).unwrap_or_else(|| "Look at the evidence lines on the device".into()),
            };
            CheckResult {
                id: d.id.into(),
                title: d.title.into(),
                category: d.category.into(),
                status: v.status,
                summary: v.summary,
                evidence: v.evidence,
                follow_up: v.follow_up.iter().map(|s| s.to_string()).collect(),
                commands,
                table: v.table,
                collected_at: now,
                next_step,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outputs(pairs: &[(&str, &str)]) -> Outputs {
        pairs.iter().map(|(c, o)| (c.to_string(), Ok(o.to_string()))).collect()
    }

    #[test]
    fn cpu_and_memory_ios() {
        let out = outputs(&[
            ("show processes cpu | include CPU", "CPU utilization for five seconds: 91%/2%; one minute: 88%; five minutes: 86%"),
            ("show processes memory | include Processor", "Processor Pool Total:  383454200 Used:  360153948 Free:   23300252"),
        ]);
        assert_eq!(cpu(Os::Ios, &out).status, Status::Critical);
        let m = memory(Os::Ios, &out);
        assert_eq!(m.status, Status::Critical, "{}", m.summary);
        let out = outputs(&[("show system resources", "Load average:   1 minute: 0.30\nCPU states  :  3.57% user,   2.34% kernel,   94.08% idle\nMemory usage:   16400784K total,   5812148K used,   10588636K free")]);
        assert_eq!(cpu(Os::NxOs, &out).status, Status::Healthy);
        assert_eq!(memory(Os::NxOs, &out).status, Status::Healthy);
    }

    #[test]
    fn environment_faults() {
        let out = outputs(&[("show env all", "FAN 1 is OK\nFAN 2 is FAULTY\nTEMPERATURE is OK\nTemperature Value: 38 Degree Celsius\nSW  PID                 Serial#     Status           Sys Pwr  PoE Pwr  Watts\n1A  PWR-C1-715WAC       ART1        OK               Good     Good     715\n1B  Not Present\n")]);
        assert_eq!(environment("Fans", Os::Ios, &out).status, Status::Critical);
        assert_eq!(environment("Temperature", Os::Ios, &out).status, Status::Healthy);
        assert_eq!(environment("Power supplies", Os::Ios, &out).status, Status::Warning);
    }

    #[test]
    fn port_channel_member_down() {
        let out = outputs(&[("show etherchannel summary", "Group  Port-channel  Protocol    Ports\n------+-------------+-----------+-----------\n1      Po1(SU)         LACP      Gi1/0/49(P)    Gi1/0/50(D)\n2      Po2(SU)         LACP      Gi1/0/51(P)    Gi1/0/52(P)\n")]);
        let v = port_channels(Os::Ios, &out);
        assert_eq!(v.status, Status::Critical);
        assert_eq!(v.evidence[0], "Po1: member Gi1/0/50 is down");
        assert_eq!(v.table.unwrap().rows.len(), 2);
    }

    #[test]
    fn routing_states() {
        let out = outputs(&[
            ("show ip ospf neighbor", "Neighbor ID     Pri   State           Dead Time   Address         Interface\n10.0.0.2          1   FULL/DR         00:00:35    10.1.1.2        Vlan100\n10.0.0.3          1   EXSTART/DROTHER 00:00:31    10.1.1.3        Vlan100\n"),
            ("show ip bgp summary", "Neighbor        V           AS MsgRcvd MsgSent   TblVer  InQ OutQ Up/Down  State/PfxRcd\n192.0.2.1       4        65001  100000  100001  1234567    0    0 10w2d          120\n192.0.2.2       4        65002       0       0        1    0    0 never    Active\n"),
        ]);
        let v = routing(Os::Ios, &out);
        assert_eq!(v.status, Status::Critical);
        assert!(v.evidence.iter().any(|l| l.contains("192.0.2.2") && l.contains("Active")));
        assert!(v.evidence.iter().any(|l| l.contains("10.0.0.3") && l.contains("EXSTART")));
    }

    #[test]
    fn stack_states() {
        let out = outputs(&[("show switch", "Switch/Stack Mac Address : 0000.0000.0001\n                                             H/W   Current\nSwitch#  Role   Mac Address     Priority Version  State\n----------------------------------------------------------\n*1       Active   0000.0000.0001     15     V01     Ready\n 2       Standby  0000.0000.0002     14     V01     Ready\n 3       Member   0000.0000.0003     1      V01     Version Mismatch\n")]);
        let v = stack(Os::IosXe, &out);
        assert_eq!(v.status, Status::Critical);
        assert_eq!(v.evidence[0], "Switch 3 is Version Mismatch");
    }

    #[test]
    fn loop_between_two_local_ports() {
        let mut out = Outputs::new();
        out.insert("show spanning-tree".into(), Ok("VLAN0001\n  Spanning tree enabled protocol ieee\n  Root ID    Priority    32769\n             This bridge is the root\nInterface           Role Sts Cost      Prio.Nbr Type\nFa0/1               Desg FWD 19        128.1    P2p\nFa0/13              Back BLK 19        128.13   P2p\nGi0/1               Desg FWD 4         128.25   P2p\n".into()));
        out.insert(DETAIL_CMD.into(), Ok("  Number of topology changes 15 last change occurred 00:02:11 ago\n          from FastEthernet0/1\n Port 1 (FastEthernet0/1) of VLAN0001 is designated forwarding\n   Designated bridge has priority 32769, address 0011.2233.4455\n   Designated port id is 128.1, designated path cost 0\n Port 13 (FastEthernet0/13) of VLAN0001 is backup blocking\n   Designated bridge has priority 32769, address 0011.2233.4455\n   Designated port id is 128.1, designated path cost 0\n".into()));
        let v = spanning_tree(Os::Ios, &out);
        assert_eq!(v.status, Status::Critical);
        assert!(v.summary.contains("Fa0/1 and Fa0/13"), "{}", v.summary);
        let log = "Oct 14 10:00:01.100: %LINK-3-UPDOWN: Interface FastEthernet0/1, changed state to down\nOct 14 10:00:01.100: %LINK-3-UPDOWN: Interface FastEthernet0/13, changed state to down\nOct 14 10:00:05.200: %LINK-3-UPDOWN: Interface FastEthernet0/1, changed state to up\nOct 14 10:00:05.200: %LINK-3-UPDOWN: Interface FastEthernet0/13, changed state to up\n";
        let pairs = flapping_together(log);
        assert_eq!(pairs, vec![("Fa0/1".to_string(), "Fa0/13".to_string(), 2)]);
        let mut o = Outputs::new();
        o.insert("show etherchannel summary".into(), Ok("Number of channel-groups in use: 1\nGroup  Port-channel  Protocol    Ports\n------+-------------+-----------+-----\n1      Po1(SD)          -        \n".into()));
        let v = port_channels(Os::Ios, &o);
        assert_eq!(v.status, Status::Critical, "{}", v.summary);
    }

    #[test]
    fn catalyst_env_all_power() {
        let mut out = Outputs::new();
        out.insert("show env all".into(), Ok("FAN is OK\nTEMPERATURE is OK\nSW  PID                 Serial#     Status           Sys Pwr  PoE Pwr  Watts\n--  ------------------  ----------  ---------------  -------  -------  -----\n 1  Built-in                        Good             Good     Good     370\nSW  Status          RPS Name          RPS Serial#  RPS Port#\n--  -------------   ----------------  -----------  ---------\n 1  Not Present     <>\n".into()));
        let v = environment("Power supplies", Os::Ios, &out);
        assert_eq!(v.status, Status::Healthy, "{} {:?}", v.summary, v.evidence);
        let v = environment("Fans", Os::Ios, &out);
        assert_eq!(v.status, Status::Healthy);
        // a feature the device does not have is not a collection failure
        let mut o = Outputs::new();
        o.insert("show switch".into(), Err("the device rejected `show switch`: % Invalid input".into()));
        o.insert("show switch stack-ports".into(), Err("the device rejected `show switch stack-ports`: % Invalid input".into()));
        let r = evaluate(Os::Ios, &["stack".into()], None, &o);
        assert_eq!(r[0].status, Status::NotApplicable);
    }

    #[test]
    fn logs_and_skips() {
        let out = outputs(&[("show logging | include -[0-3]-", "Jan  1 00:00:01: %LINK-3-UPDOWN: Interface Gi1/0/1, changed state to down\nJan  1 00:00:02: %SYS-2-MALLOCFAIL: Memory allocation failed")]);
        assert_eq!(logs(Os::Ios, &out).status, Status::Critical);
        let mut out = Outputs::new();
        out.insert("show power inline".into(), Err("the device rejected `show power inline`".into()));
        assert_eq!(poe(Os::Ios, &out).status, Status::Unsupported);
        let results = evaluate(Os::Asa, &[], None, &Outputs::new());
        assert!(results.iter().any(|r| r.id == "poe" && r.status == Status::Unsupported));
    }

    #[test]
    fn command_list_dedupes() {
        let cmds = commands_for(Os::IosXe, &[], None);
        assert_eq!(cmds.iter().filter(|c| *c == "show environment all").count(), 1);
        assert!(cmds.contains(&"show version".to_string()));
    }
}
