//! What a proposed command would do: read something, change the
//! configuration, or disrupt the device. The UI gates each kind
//! differently: read-only runs on approval, config needs a second
//! confirmation, disruptive needs the production lock opened too.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    ReadOnly,
    Config,
    Disruptive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classified {
    pub command: String,
    pub kind: Kind,
    pub reason: String,
}

const READ_ONLY: &[&str] = &[
    "show", "display", "dir", "more", "ping", "traceroute", "trace", "terminal", "where", "who", "whoami", "pwd", "cd", "verify",
    "test cable-diagnostics", "test aaa", "monitor", "enable", "disable", "exit", "end", "quit", "logout", "?", "help", "get",
    "sh ", "sho ", "disp ", "cat ", "ls", "run show", "do show", "echo",
];

const DISRUPTIVE: &[(&str, &str)] = &[
    ("reload", "reloads the device"),
    ("reboot", "reboots the device"),
    ("write erase", "erases the startup configuration"),
    ("erase ", "erases flash or the startup configuration"),
    ("format ", "formats a file system"),
    ("delete ", "deletes files"),
    ("rmdir", "deletes a folder"),
    ("squeeze", "rewrites flash"),
    ("boot ", "changes the boot image"),
    ("install ", "installs or activates software"),
    ("request ", "installs software or reloads"),
    ("archive download", "downloads and installs software"),
    ("software install", "installs software"),
    ("upgrade", "upgrades software"),
    ("copy ", "copies files, possibly over the startup config or the image"),
    ("clear ip", "clears routing or ARP state"),
    ("clear line", "disconnects a line"),
    ("clear mac", "flushes the MAC table"),
    ("clear arp", "flushes the ARP table"),
    ("clear spanning", "restarts spanning tree"),
    ("clear interface", "resets interfaces"),
    ("clear port", "resets ports"),
    ("clear configuration", "clears the configuration"),
    ("debug ", "debug output can overload the CPU"),
    ("shutdown", "shuts something down"),
    ("no shutdown", "brings an interface up"),
    ("test crash", "crashes the device"),
    ("power ", "controls power"),
    ("hw-module", "resets hardware"),
    ("redundancy force", "forces a supervisor switchover"),
    ("switch ", "changes the stack"),
    ("factory-reset", "factory resets the device"),
    ("config-register", "changes the config register"),
];

fn starts(cmd: &str, word: &str) -> bool {
    cmd == word || cmd.starts_with(&format!("{word} ")) || (word.ends_with(' ') && cmd.starts_with(word))
}

/// Classify commands as a sequence: once `configure terminal` (or an
/// interface or other mode line) is seen, following lines are config
/// until `end`.
pub fn classify(commands: &[String]) -> Vec<Classified> {
    let mut in_config = false;
    let mut out = Vec::new();
    for raw in commands {
        let command = raw.trim().to_string();
        if command.is_empty() || command.starts_with('!') || command.starts_with('#') {
            continue;
        }
        let lower = command.to_lowercase();
        let lower = lower.trim_start_matches("do ").to_string();
        let (kind, reason) = if let Some((_, why)) = DISRUPTIVE.iter().find(|(w, _)| starts(&lower, w) || (w.ends_with(' ') && lower.contains(w) && !lower.starts_with("show"))) {
            // "shutdown" inside an interface is config, but still disruptive to that port
            (Kind::Disruptive, why.to_string())
        } else if lower == "end" || lower == "exit" && in_config {
            if lower == "end" {
                in_config = false;
            }
            (Kind::Config, "leaves configuration mode".into())
        } else if lower.starts_with("conf") && (lower.contains(" t") || lower == "configure" || lower == "conf") {
            in_config = true;
            (Kind::Config, "enters configuration mode".into())
        } else if lower.starts_with("write") || lower.starts_with("wr") && lower.len() <= 3 {
            (Kind::Config, "saves the configuration".into())
        } else if lower.starts_with("clear counters") || lower.starts_with("clear logging") {
            (Kind::ReadOnly, "clears counters only".into())
        } else if lower.starts_with("clear ") {
            (Kind::Disruptive, "clears state".into())
        } else if in_config {
            (Kind::Config, "configuration line".into())
        } else if READ_ONLY.iter().any(|w| starts(&lower, w.trim_end()) || (w.ends_with(' ') && lower.starts_with(w))) {
            (Kind::ReadOnly, "shows information".into())
        } else if lower.starts_with("ssh ") || lower.starts_with("telnet ") {
            (Kind::ReadOnly, "opens a session from the device".into())
        } else {
            // interface Gi1/0/1, vlan 10, router ospf 1, ip route ... without "configure terminal" first
            in_config = true;
            (Kind::Config, "configuration line (configuration mode assumed)".into())
        };
        out.push(Classified { command, kind, reason });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(cmds: &[&str]) -> Vec<Kind> {
        classify(&cmds.iter().map(|s| s.to_string()).collect::<Vec<_>>()).iter().map(|c| c.kind).collect()
    }

    #[test]
    fn classifies() {
        assert_eq!(kinds(&["show version", "sh ip int br", "ping 10.0.0.1", "terminal length 0"]), vec![Kind::ReadOnly; 4]);
        assert_eq!(kinds(&["configure terminal", "interface Gi1/0/1", " description uplink", "end", "write memory"]), vec![Kind::Config; 5]);
        assert_eq!(kinds(&["reload", "write erase", "copy run start", "clear ip ospf process", "debug ip packet"]), vec![Kind::Disruptive; 5]);
        assert_eq!(kinds(&["conf t", "interface Gi1/0/1", "shutdown"]), vec![Kind::Config, Kind::Config, Kind::Disruptive]);
        assert_eq!(kinds(&["interface Vlan10", "ip address 10.0.0.1 255.255.255.0"]), vec![Kind::Config, Kind::Config]);
        assert_eq!(kinds(&["clear counters"]), vec![Kind::ReadOnly]);
        assert_eq!(kinds(&["! comment", "", "show run"]), vec![Kind::ReadOnly]);
    }
}
