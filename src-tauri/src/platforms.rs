//! One profile per network OS: what to collect, how to troubleshoot, how a
//! configuration change is entered, committed, saved and undone. The
//! detection and facts live in `cisco::detect`; this is the command data
//! that differs per platform, kept apart from the check logic so a new OS
//! is mostly a matter of filling these tables.
//!
//! Command syntax follows the vendor references (AOS-S 16.x, AOS-CX 10.x,
//! Junos 21+, EOS 4.2x, Cisco S1 SONiC CLI, Cumulus 5.x NVUE with 4.x NCLU
//! and Linux/FRR fallbacks). Where a release differs, the collection lists
//! the alternatives and the first one the device accepts is used.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::cisco::detect::Os;
use crate::session::SessionManager;
use crate::tools::ToolState;

type CmdResult<T> = Result<T, String>;

/// A quick action: a named set of read-only commands.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub title: String,
    pub commands: Vec<String>,
    /// not available on this platform (shown greyed, with the reason)
    pub note: String,
}

/// Which configuration texts the platform has and how to read them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigKind {
    pub kind: String,
    pub label: String,
    pub command: String,
}

/// How a configuration change is made on this platform.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    /// candidate configuration with an explicit commit (Junos, EOS sessions, NVUE)
    pub candidate: bool,
    pub enter: Vec<String>,
    pub exit: Vec<String>,
    /// after the lines: makes the change live (empty when lines apply as typed)
    pub commit: Vec<String>,
    /// a commit that undoes itself unless confirmed, with the minutes placeholder {m}
    pub commit_confirmed: String,
    pub confirm: Vec<String>,
    pub save: Vec<String>,
    /// taken before the change, with the {name} placeholder
    pub checkpoint: String,
    pub rollback: String,
    /// the restorable backup is this command's output
    pub restorable: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub os: Os,
    pub label: String,
    pub vendor: String,
    pub shell: bool,
    pub no_paging: Option<String>,
    pub privileged_prompt: String,
    pub enable: Option<String>,
    pub configs: Vec<ConfigKind>,
    pub collections: Vec<Collection>,
    pub troubleshooting: Vec<Collection>,
    pub workflow: Workflow,
    /// commands validated against which document
    pub reference: String,
}

fn c(id: &str, title: &str, cmds: &[&str]) -> Collection {
    Collection { id: id.into(), title: title.into(), commands: cmds.iter().map(|s| s.to_string()).collect(), note: String::new() }
}

fn na(id: &str, title: &str, note: &str) -> Collection {
    Collection { id: id.into(), title: title.into(), commands: Vec::new(), note: note.into() }
}

fn k(kind: &str, label: &str, command: &str) -> ConfigKind {
    ConfigKind { kind: kind.into(), label: label.into(), command: command.into() }
}

fn v(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

/// The standard collection ids, in display order. Every profile answers each
/// one, with a note where the platform has nothing for it.
#[allow(dead_code)]
pub const COLLECTION_IDS: &[(&str, &str)] = &[
    ("system", "System and hardware inventory"),
    ("config", "Configuration"),
    ("interfaces", "Interfaces, counters, errors, optics, PoE"),
    ("layer2", "VLANs, trunks, spanning tree, LACP"),
    ("neighbors", "LLDP / CDP neighbors"),
    ("routing", "Routing, VRFs, OSPF, BGP"),
    ("redundancy", "Stack / VC / VSF / VSX / MLAG"),
    ("overlay", "VXLAN / EVPN"),
    ("health", "Logs, environment, CPU, memory, storage"),
];

pub fn profile(os: Os) -> Profile {
    let (vendor, privileged_prompt, enable) = match os {
        Os::Ios | Os::IosXe | Os::NxOs | Os::IosXr | Os::Asa | Os::SmallBusiness | Os::CatOs => ("Cisco", "#", Some("enable")),
        Os::ArubaOsS => ("Aruba (HPE)", "#", Some("enable")),
        Os::ArubaCx => ("Aruba (HPE)", "#", None),
        Os::Junos => ("Juniper", ">", None),
        Os::Eos => ("Arista", "#", Some("enable")),
        Os::Sonic => ("Cisco S1 / SONiC", "#", None),
        Os::Cumulus => ("NVIDIA", "$", None),
        Os::Ise => ("Cisco", "#", None),
        Os::Aireos => ("Cisco", ">", None),
        Os::Unknown => ("", "#", None),
    };
    let (configs, collections, troubleshooting, workflow, reference) = match os {
        Os::ArubaOsS => (
            vec![k("running", "Running configuration", "show running-config"), k("startup", "Startup configuration", "show config")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show system", "show modules", "show system information", "show flash"]),
                c("config", "Configuration", &["show running-config", "show config"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interfaces brief", "show interfaces", "show interfaces transceiver detail", "show power-over-ethernet brief"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlans", "show trunks", "show lacp", "show spanning-tree", "show spanning-tree detail"]),
                c("neighbors", "LLDP / CDP neighbors", &["show lldp info remote-device", "show lldp info remote-device detail", "show cdp neighbors detail"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route", "show ip", "show vrf", "show ip ospf neighbor", "show ip bgp summary"]),
                c("redundancy", "Stack / VSF", &["show stacking", "show vsf", "show vsf detail", "show vsf link"]),
                na("overlay", "VXLAN / EVPN", "Not available on AOS-S"),
                c("health", "Logs, environment, CPU, memory, storage", &["show logging -r", "show system temperature", "show system power-supply", "show system fans", "show cpu", "show system", "show flash", "show ntp status", "show sntp"]),
            ],
            vec![
                c("port", "Port problems", &["show interfaces brief", "show interfaces display", "show lldp info remote-device", "show mac-address", "show power-over-ethernet", "show port-security"]),
                c("stp", "Spanning tree events", &["show spanning-tree", "show spanning-tree root-history", "show logging -r | include STP|spanning"]),
                c("auth", "802.1X / MAC auth", &["show port-access clients", "show port-access authenticator", "show radius"]),
                c("uplinks", "Uplinks and trunks", &["show trunks", "show lacp", "show interfaces brief | include Up"]),
                c("crash", "Crashes and reboots", &["show system | include Up|Reboot", "show boot-history", "show logging -r | include crash|reboot|reload"]),
            ],
            Workflow {
                candidate: false, enter: v(&["configure"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["write memory"]), checkpoint: String::new(), rollback: String::new(), restorable: "show running-config".into(),
                notes: v(&["AOS-S has no checkpoints: the restorable backup is the running config text, restored with `copy tftp startup-config` or by pasting."]),
            },
            "Aruba AOS-S 16.11 command reference",
        ),
        Os::ArubaCx => (
            vec![k("running", "Running configuration", "show running-config"), k("startup", "Startup configuration", "show startup-config"), k("checkpoints", "Checkpoints", "show checkpoint list")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show system", "show module", "show interface transceiver brief", "show images"]),
                c("config", "Configuration", &["show running-config", "show startup-config", "show checkpoint list"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interface brief", "show interface", "show interface transceiver", "show power-over-ethernet brief"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlan", "show interface vlan", "show spanning-tree", "show spanning-tree summary", "show lacp interfaces", "show lacp aggregates"]),
                c("neighbors", "LLDP / CDP neighbors", &["show lldp neighbor-info", "show lldp neighbor-info detail", "show cdp neighbor-info"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route", "show vrf", "show ip ospf neighbors", "show bgp ipv4 unicast summary", "show bgp all summary"]),
                c("redundancy", "VSF / VSX", &["show vsf", "show vsf detail", "show vsx status", "show vsx brief", "show vsx config-consistency"]),
                c("overlay", "VXLAN / EVPN", &["show interface vxlan", "show vxlan vni", "show evpn evi", "show bgp l2vpn evpn summary"]),
                c("health", "Logs, environment, CPU, memory, storage", &["show events -r", "show environment temperature", "show environment power-supply", "show environment fan", "show system resource-utilization", "show ntp status", "show images"]),
            ],
            vec![
                c("port", "Port problems", &["show interface brief", "show interface", "show lldp neighbor-info", "show mac-address-table", "show power-over-ethernet", "show port-access clients"]),
                c("stp", "Spanning tree events", &["show spanning-tree", "show spanning-tree detail", "show events -r | include STP|spanning"]),
                c("auth", "802.1X / MAC auth", &["show port-access clients", "show aaa authentication port-access interface all client-status", "show radius-server"]),
                c("vsx", "VSX health", &["show vsx status", "show vsx config-consistency", "show lacp interfaces multi-chassis", "show vsx brief"]),
                c("crash", "Crashes and reboots", &["show system | include Up|Boot", "show boot-history", "show events -r | include crash|reboot|reload"]),
            ],
            Workflow {
                candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["write memory"]), checkpoint: "copy running-config checkpoint {name}".into(), rollback: "checkpoint rollback {name}".into(), restorable: "show running-config".into(),
                notes: v(&["AOS-CX also auto-saves a checkpoint before every change; `checkpoint auto` arms a timed rollback."]),
            },
            "Aruba AOS-CX 10.x command reference (release-specific)",
        ),
        Os::Junos => (
            vec![k("committed", "Committed configuration", "show configuration | display set"), k("candidate-diff", "Candidate vs committed", "show configuration | compare rollback 0"), k("rollbacks", "Rollback history", "show system commit")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show chassis hardware", "show system uptime", "show system storage", "show chassis alarms", "show system alarms"]),
                c("config", "Configuration", &["show configuration | display set", "show system commit"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interfaces terse", "show interfaces extensive | match \"interface|errors|drops|Input bytes|Output bytes\"", "show interfaces diagnostics optics", "show poe interface"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlans", "show ethernet-switching interface", "show spanning-tree bridge", "show spanning-tree interface", "show lacp interfaces"]),
                c("neighbors", "LLDP / CDP neighbors", &["show lldp neighbors", "show lldp neighbors detail"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show route summary", "show route instance", "show ospf neighbor", "show bgp summary"]),
                c("redundancy", "Virtual Chassis", &["show virtual-chassis", "show virtual-chassis vc-port", "show chassis routing-engine"]),
                c("overlay", "VXLAN / EVPN", &["show evpn instance", "show evpn database", "show vxlan tunnel-end-point remote"]),
                c("health", "Logs, environment, CPU, memory, storage", &["show log messages | last 100", "show chassis environment", "show chassis routing-engine", "show system storage", "show ntp status", "show system processes extensive | match \"CPU|Mem\""]),
            ],
            vec![
                c("port", "Port problems", &["show interfaces terse", "show interfaces extensive | match \"interface|errors|drops\"", "show lldp neighbors", "show ethernet-switching table", "show poe interface"]),
                c("stp", "Spanning tree events", &["show spanning-tree bridge", "show spanning-tree interface", "show log messages | match \"STP|RSTP|topology\" | last 50"]),
                c("auth", "802.1X / MAC auth", &["show dot1x interface", "show network-access requests", "show dot1x authentication-failed-users"]),
                c("vc", "Virtual Chassis health", &["show virtual-chassis status", "show virtual-chassis vc-port", "show virtual-chassis protocol adjacency"]),
                c("crash", "Crashes and reboots", &["show system uptime", "show system core-dumps", "show log messages | match \"reboot|panic|crash\" | last 50"]),
            ],
            Workflow {
                candidate: true, enter: v(&["configure"]), exit: v(&["exit"]), commit: v(&["commit"]), commit_confirmed: "commit confirmed {m}".into(), confirm: v(&["commit"]),
                save: v(&[]), checkpoint: String::new(), rollback: "rollback 1".into(), restorable: "show configuration".into(),
                notes: v(&["Lines are entered in the candidate configuration and nothing is live until `commit`.", "`commit confirmed N` reverts on its own unless confirmed; `rollback 1` + `commit` undoes the last commit."]),
            },
            "Junos CLI user guide; command names are consistent across EX/QFX/MX",
        ),
        Os::Eos => (
            vec![k("running", "Running configuration", "show running-config"), k("startup", "Startup configuration", "show startup-config"), k("checkpoints", "Checkpoints", "show configuration checkpoints")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show inventory", "show boot-config", "show extensions"]),
                c("config", "Configuration", &["show running-config", "show startup-config", "show configuration checkpoints"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interfaces status", "show interfaces counters errors", "show interfaces counters discards", "show interfaces transceiver", "show poe"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlan", "show interfaces trunk", "show spanning-tree", "show port-channel summary", "show lacp neighbor"]),
                c("neighbors", "LLDP / CDP neighbors", &["show lldp neighbors", "show lldp neighbors detail"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route summary", "show vrf", "show ip ospf neighbor", "show ip bgp summary", "show bgp summary"]),
                c("redundancy", "MLAG", &["show mlag", "show mlag detail", "show mlag interfaces", "show mlag config-sanity"]),
                c("overlay", "VXLAN / EVPN", &["show vxlan vtep", "show vxlan vni", "show bgp evpn summary", "show vxlan address-table"]),
                c("health", "Logs, environment, CPU, memory, storage", &["show logging last 100", "show environment all", "show processes top once", "show version | include memory", "show file systems", "show ntp status"]),
            ],
            vec![
                c("port", "Port problems", &["show interfaces status", "show interfaces counters errors", "show lldp neighbors", "show mac address-table", "show interfaces transceiver"]),
                c("stp", "Spanning tree events", &["show spanning-tree", "show spanning-tree topology status", "show logging last 50 | include STP|SPANTREE"]),
                c("mlag", "MLAG health", &["show mlag", "show mlag config-sanity", "show mlag interfaces detail"]),
                c("bgp", "BGP neighbors", &["show ip bgp summary", "show bgp evpn summary", "show ip bgp neighbors | include BGP neighbor|state"]),
                c("crash", "Crashes and reboots", &["show reload cause", "show agent logs crash", "show logging last 50 | include reboot|crash"]),
            ],
            Workflow {
                candidate: true, enter: v(&["configure session omni-{name}"]), exit: v(&["end"]), commit: v(&["configure session omni-{name} commit"]), commit_confirmed: "configure session omni-{name} commit timer 00:{m}:00".into(), confirm: v(&["configure session omni-{name} commit"]),
                save: v(&["write memory"]), checkpoint: "configure checkpoint save {name}".into(), rollback: "configure replace checkpoint:{name}".into(), restorable: "show running-config".into(),
                notes: v(&["Changes go into a configuration session and apply on commit; `commit timer` reverts them unless confirmed."]),
            },
            "Arista EOS CLI reference (4.2x)",
        ),
        Os::Sonic => (
            vec![k("running", "Running configuration", "show running-configuration"), k("startup", "Saved configuration (config_db)", "show startup-configuration")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show platform summary", "show platform inventory", "show platform syseeprom", "show boot"]),
                c("config", "Configuration", &["show running-configuration", "show startup-configuration"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interface status", "show interface counters errors", "show interface counters", "show interface transceiver summary"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlan", "show spanning-tree", "show portchannel summary", "show lacp"]),
                c("neighbors", "LLDP / CDP neighbors", &["show lldp table", "show lldp neighbors"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route summary", "show vrf", "show ip ospf neighbor", "show ip bgp summary", "show bgp summary"]),
                c("redundancy", "MC-LAG", &["show mclag brief", "show mclag interface"]),
                c("overlay", "VXLAN / EVPN", &["show vxlan tunnel", "show vxlan vlanvnimap", "show bgp l2vpn evpn summary"]),
                c("health", "Logs, environment, CPU, memory, storage", &["show logging | tail -n 100", "show platform environment", "show platform psustatus", "show platform fanstatus", "show platform temperature", "show system memory", "show system cpu", "show ntp", "show platform storage"]),
            ],
            vec![
                c("port", "Port problems", &["show interface status", "show interface counters errors", "show lldp table", "show mac address-table", "show interface transceiver summary"]),
                c("bgp", "BGP neighbors", &["show ip bgp summary", "show bgp l2vpn evpn summary", "show ip bgp neighbors"]),
                c("services", "Containers and services", &["show services", "show system status", "show feature status"]),
                c("crash", "Crashes and reboots", &["show reboot-cause", "show reboot-cause history", "show logging | grep -i \"crash\\|reboot\" | tail -n 50"]),
            ],
            Workflow {
                candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["write memory"]), checkpoint: String::new(), rollback: String::new(), restorable: "show running-configuration".into(),
                notes: v(&["The Cisco S1 SONiC CLI applies lines as typed; `write memory` persists config_db. There is no checkpoint; restore from the saved config_db."]),
            },
            "Cisco S1 SONiC CLI reference; community SONiC uses `show runningconfiguration all` and `config save -y` instead",
        ),
        Os::Cumulus => (
            vec![k("applied", "Applied configuration (NVUE)", "nv config show"), k("startup", "Saved startup (NVUE)", "cat /etc/nvue.d/startup.yaml"), k("nclu", "NCLU commands (4.x)", "net show configuration commands"), k("frr", "FRRouting", "sudo vtysh -c \"show running-config\"")],
            vec![
                c("system", "System and hardware inventory", &["nv show system", "nv show platform", "nv show platform hardware", "net show system", "cat /etc/lsb-release", "uptime", "sudo decode-syseeprom"]),
                c("config", "Configuration", &["nv config show", "net show configuration commands", "sudo vtysh -c \"show running-config\"", "cat /etc/network/interfaces"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["nv show interface", "nv show interface --view=counters", "net show interface all", "ip -s -br link", "sudo l1-show all", "sudo ethtool -m swp1"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["nv show bridge domain br_default vlan", "nv show interface --view=lldp", "nv show interface bond0", "net show bridge vlan", "net show bridge spanning-tree", "sudo mstpctl showbridge", "cat /proc/net/bonding/bond0"]),
                c("neighbors", "LLDP / CDP neighbors", &["nv show interface --view=lldp", "net show lldp", "sudo lldpctl"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["nv show vrf", "nv show vrf default router bgp neighbor", "net show bgp summary", "net show ospf neighbor", "sudo vtysh -c \"show bgp summary\"", "sudo vtysh -c \"show ip ospf neighbor\"", "sudo vtysh -c \"show ip route summary\""]),
                c("redundancy", "MLAG", &["nv show mlag", "nv show mlag backup", "net show clag", "sudo clagctl"]),
                c("overlay", "VXLAN / EVPN", &["nv show nve vxlan", "nv show evpn", "net show evpn vni", "sudo vtysh -c \"show bgp l2vpn evpn summary\"", "sudo vtysh -c \"show evpn vni\""]),
                c("health", "Logs, environment, CPU, memory, storage", &["sudo journalctl -p err -n 100 --no-pager", "nv show platform environment", "sudo smonctl", "nv show system cpu", "nv show system memory", "top -bn1 | head -5", "free -m", "df -h", "chronyc tracking", "timedatectl"]),
            ],
            vec![
                c("port", "Port problems", &["nv show interface swp1", "net show interface swp1", "ip -s link show swp1", "sudo ethtool swp1", "sudo ethtool -S swp1 | grep -i err", "sudo ethtool -m swp1"]),
                c("mlag", "MLAG health", &["nv show mlag", "net show clag", "sudo clagctl", "sudo clagctl verifyvlans"]),
                c("bgp", "BGP neighbors", &["nv show vrf default router bgp neighbor", "net show bgp summary", "sudo vtysh -c \"show bgp summary\"", "sudo vtysh -c \"show bgp neighbors\" | grep -i \"neighbor\\|state\""]),
                c("services", "Daemons and services", &["sudo systemctl status switchd frr nvued --no-pager", "sudo systemctl --failed --no-pager", "nv show system"]),
                c("crash", "Crashes and reboots", &["uptime", "last reboot | head", "sudo journalctl -p crit -n 50 --no-pager", "ls -la /var/support"]),
            ],
            Workflow {
                candidate: true, enter: v(&[]), exit: v(&[]), commit: v(&["nv config apply --assume-yes"]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["nv config save"]), checkpoint: String::new(), rollback: "nv config apply startup --assume-yes".into(), restorable: "nv config show".into(),
                notes: v(&["NVUE (5.x): lines are `nv set ...`, nothing is live until `nv config apply`; `nv config save` persists it. Rolling back re-applies the saved startup, so save only after verifying.", "NCLU (3.x/4.x): use `net add ...`, `net commit`, `net rollback last`. Linux files and FRR `vtysh` for anything NVUE does not cover."]),
            },
            "NVIDIA Cumulus Linux command reference (NVUE 5.x; NCLU for 4.x)",
        ),
        Os::Ise => (
            vec![k("running", "ADE-OS running configuration", "show running-config"), k("startup", "ADE-OS startup configuration", "show startup-config")],
            vec![
                c("system", "System and hardware inventory", &["show version", "show uptime", "show inventory", "show udi", "show application version ise"]),
                c("config", "Configuration", &["show running-config", "show repository"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show interface", "show ports"]),
                na("layer2", "VLANs, trunks, spanning tree, LACP", "An appliance has no switching features"),
                na("neighbors", "LLDP / CDP neighbors", "Not available on ADE-OS"),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route", "show interface | include inet"]),
                c("redundancy", "Deployment and personas", &["show application status ise", "show application version ise"]),
                na("overlay", "VXLAN / EVPN", "Not applicable to ISE"),
                c("health", "Logs, environment, CPU, memory, storage", &["show logging system | last 100", "show cpu", "show memory", "show disks", "show ntp", "show backup history", "show application status ise"]),
            ],
            vec![
                c("services", "Services not running", &["show application status ise", "show logging application ise-psc.log tail", "show logging application ise-psc.log | include ERROR"]),
                c("auth", "Authentication failures", &["show logging application prrt-server.log tail", "show logging application ise-psc.log | include RADIUS|TACACS"]),
                c("sync", "Deployment sync and replication", &["show application status ise", "show logging application ise-psc.log | include replication|sync"]),
                c("disk", "Disk and log growth", &["show disks", "show logging system | include disk|space", "show repository"]),
                c("crash", "Crashes and reboots", &["show uptime", "show logging system | include reboot|crash|panic|oom"]),
            ],
            Workflow {
                candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["write memory"]), checkpoint: String::new(), rollback: String::new(), restorable: "show running-config".into(),
                notes: v(&["ADE-OS config only covers the appliance (interfaces, NTP, DNS, repositories, logging). Policy lives in the ISE admin UI or ERS/OpenAPI, never in this CLI.", "Services restart with `application stop ise` / `application start ise`; `application reset-config ise` wipes the node. Both wait for review and approval."]),
            },
            "Cisco ISE 3.x CLI reference guide (ADE-OS)",
        ),
        Os::Aireos => (
            vec![k("running", "Running configuration", "show run-config commands"), k("startup", "Saved configuration (full)", "show run-config")],
            vec![
                c("system", "System and hardware inventory", &["show sysinfo", "show inventory", "show boot", "show redundancy summary"]),
                c("config", "Configuration", &["show run-config commands", "show run-config"]),
                c("interfaces", "Interfaces, counters, errors, optics, PoE", &["show port summary", "show port detailed-info all", "show interface summary", "show lag summary"]),
                c("layer2", "VLANs, trunks, spanning tree, LACP", &["show interface summary", "show lag summary", "show spanningtree switch"]),
                c("neighbors", "LLDP / CDP neighbors", &["show cdp neighbors detail"]),
                c("routing", "Routing, VRFs, OSPF, BGP", &["show route summary", "show interface summary", "show arp switch"]),
                c("redundancy", "HA / SSO", &["show redundancy summary", "show redundancy detail", "show redundancy peer-system statistics"]),
                c("overlay", "Wireless: APs, WLANs, clients, RF", &["show ap summary", "show ap join stats summary all", "show ap image all", "show wlan summary", "show client summary", "show advanced 802.11a summary", "show advanced 802.11b summary", "show rf-profile summary"]),
                c("health", "Logs, environment, CPU, memory, storage", &["show msglog", "show traplog", "show cpu", "show memory statistics", "show sysinfo", "show time"]),
            ],
            vec![
                c("ap-join", "APs not joining", &["show ap join stats summary all", "show ap join stats detailed <AP-MAC>", "show ap summary", "show certificate summary", "show time", "show msglog"]),
                c("clients", "Client connectivity", &["show client summary", "show client detail <MAC>", "show exclusionlist", "show radius summary", "show aaa auth"]),
                c("rf", "RF problems", &["show advanced 802.11a summary", "show advanced 802.11a channel", "show advanced 802.11a txpower", "show ap auto-rf 802.11a <AP>", "show 802.11a cleanair air-quality summary"]),
                c("ha", "HA / SSO state", &["show redundancy summary", "show redundancy detail", "show redundancy statistics"]),
                c("crash", "Crashes and reboots", &["show sysinfo", "show msglog", "show traplog", "show ap crash-file"]),
            ],
            Workflow {
                candidate: false, enter: v(&[]), exit: v(&[]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
                save: v(&["save config"]), checkpoint: String::new(), rollback: String::new(), restorable: "show run-config commands".into(),
                notes: v(&["AireOS has no config mode: `config ...` lines apply immediately and `save config` (answer y) persists them.", "Restore by uploading a saved config over TFTP/FTP (`transfer upload`) and resetting, after approval."]),
            },
            "Cisco Wireless Controller (AireOS 8.10) command reference",
        ),
        // Cisco platforms keep their command sets in cisco::checks; the
        // collections below are the same commands grouped for quick actions
        _ => cisco_profile(os),
    };
    Profile {
        os,
        label: os.label().into(),
        vendor: vendor.into(),
        shell: os.is_shell(),
        no_paging: os.no_paging().map(|s| s.to_string()),
        privileged_prompt: privileged_prompt.into(),
        enable: enable.map(|s| s.to_string()),
        configs,
        collections,
        troubleshooting,
        workflow,
        reference: reference.into(),
    }
}

fn cisco_profile(os: Os) -> (Vec<ConfigKind>, Vec<Collection>, Vec<Collection>, Workflow, &'static str) {
    let nx = os == Os::NxOs;
    let ifs = if nx { "show interface" } else { "show interfaces" };
    let configs = match os {
        Os::CatOs => vec![k("running", "Configuration", "show config")],
        Os::NxOs => vec![k("running", "Running configuration", "show running-config"), k("startup", "Startup configuration", "show startup-config"), k("checkpoints", "Checkpoints", "show checkpoint summary")],
        _ => vec![k("running", "Running configuration", "show running-config"), k("startup", "Startup configuration", "show startup-config")],
    };
    let collections = vec![
        c("system", "System and hardware inventory", &["show version", "show inventory", "show module", if nx { "show boot" } else { "show boot" }]),
        c("config", "Configuration", &["show running-config", "show startup-config"]),
        c("interfaces", "Interfaces, counters, errors, optics, PoE", &[if nx { "show interface status" } else { "show interfaces status" }, ifs, if nx { "show interface transceiver details" } else { "show interfaces transceiver detail" }, "show power inline"]),
        c("layer2", "VLANs, trunks, spanning tree, LACP", &["show vlan brief", if nx { "show interface trunk" } else { "show interfaces trunk" }, "show spanning-tree", if nx { "show port-channel summary" } else { "show etherchannel summary" }, "show lacp neighbor"]),
        c("neighbors", "LLDP / CDP neighbors", &["show cdp neighbors detail", "show lldp neighbors detail"]),
        c("routing", "Routing, VRFs, OSPF, BGP", &["show ip route summary", "show vrf", if nx { "show ip ospf neighbors" } else { "show ip ospf neighbor" }, "show ip bgp summary", "show ip eigrp neighbors"]),
        c("redundancy", "Stack / StackWise Virtual / vPC", &["show switch", "show switch stack-ports", "show stackwise-virtual", "show redundancy", "show vpc"]),
        c("overlay", "VXLAN / EVPN", &["show nve peers", "show nve vni", "show bgp l2vpn evpn summary"]),
        c("health", "Logs, environment, CPU, memory, storage", &[if nx { "show logging last 100" } else { "show logging | include -[0-3]-" }, "show environment all", "show processes cpu | include CPU", "show processes memory | include Processor", "dir flash: | include bytes", "show ntp status"]),
    ];
    let troubleshooting = vec![
        c("port", "Port problems", &[if nx { "show interface status" } else { "show interfaces status" }, ifs, "show cdp neighbors detail", "show mac address-table", "show power inline", "show interfaces status err-disabled"]),
        c("stp", "Spanning tree events", &["show spanning-tree", "show spanning-tree inconsistentports", "show spanning-tree detail | include changes|occurred|from", "show logging | include STP|SPANTREE"]),
        c("auth", "802.1X / MAC auth", &["show authentication sessions", "show dot1x all summary", "show aaa servers"]),
        c("crash", "Crashes and reboots", &["show version | include uptime|reason", "show logging | include RELOAD|crash|restart", "dir crashinfo:"]),
    ];
    let workflow = match os {
        Os::NxOs => Workflow {
            candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
            save: v(&["copy running-config startup-config"]), checkpoint: "checkpoint {name}".into(), rollback: "rollback running-config checkpoint {name}".into(), restorable: "show running-config".into(), notes: v(&[]),
        },
        Os::Ios | Os::IosXe => Workflow {
            candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
            save: v(&["write memory"]), checkpoint: "copy running-config flash:{name}.cfg".into(), rollback: "configure replace flash:{name}.cfg force".into(), restorable: "show running-config".into(), notes: v(&["`reload in N` can be armed as a safety net."]),
        },
        Os::IosXr => Workflow {
            candidate: true, enter: v(&["configure"]), exit: v(&["end"]), commit: v(&["commit"]), commit_confirmed: "commit confirmed {m}".into(), confirm: v(&["commit"]),
            save: v(&[]), checkpoint: String::new(), rollback: "rollback configuration last 1".into(), restorable: "show running-config".into(), notes: v(&[]),
        },
        _ => Workflow {
            candidate: false, enter: v(&["configure terminal"]), exit: v(&["end"]), commit: v(&[]), commit_confirmed: String::new(), confirm: v(&[]),
            save: v(&["write memory"]), checkpoint: String::new(), rollback: String::new(), restorable: "show running-config".into(), notes: v(&[]),
        },
    };
    (configs, collections, troubleshooting, workflow, "Cisco IOS / IOS-XE / NX-OS command references")
}

/// Substitute the placeholders a workflow uses.
pub fn fill(template: &str, name: &str, minutes: u32) -> String {
    template.replace("{name}", name).replace("{m}", &minutes.to_string())
}

/* ------------------------------------------------------------- commands */

#[tauri::command]
pub fn platform_profile(os: Option<Os>) -> Profile {
    profile(os.unwrap_or(Os::Unknown))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: Os,
    pub label: String,
}

/// Every platform, for the manual override list.
#[tauri::command]
pub fn platforms_list() -> Vec<PlatformInfo> {
    Os::all().iter().map(|o| PlatformInfo { os: *o, label: o.label().into() }).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CollectEvent {
    Command { command: String, index: usize, total: usize },
    Output { command: String, ok: bool, output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collected {
    pub id: String,
    pub title: String,
    pub started: i64,
    pub finished: i64,
    pub cancelled: bool,
    pub outputs: Vec<(String, Result<String, String>)>,
}

/// Run one collection (or troubleshooting set) and return every output.
#[tauri::command]
pub async fn collect_run(
    sessions: State<'_, SessionManager>,
    tools: State<'_, ToolState>,
    id: String,
    os: Os,
    collection: String,
    events: Channel<CollectEvent>,
) -> CmdResult<Collected> {
    let p = profile(os);
    let set = p.collections.iter().chain(p.troubleshooting.iter()).find(|c| c.id == collection).ok_or_else(|| format!("no collection {collection}"))?.clone();
    if set.commands.is_empty() {
        return Err(if set.note.is_empty() { "nothing to collect".into() } else { set.note });
    }
    let cancel = tools.start(&id);
    let started = chrono::Utc::now().timestamp();
    if let Some(cmd) = os.no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }
    let total = set.commands.len();
    let mut outputs: BTreeMap<usize, (String, Result<String, String>)> = BTreeMap::new();
    let mut cancelled = false;
    for (index, command) in set.commands.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let _ = events.send(CollectEvent::Command { command: command.clone(), index, total });
        let result = sessions.run_command(&id, command, Duration::from_secs(180)).await.map_err(|e| format!("{e:#}"));
        if let Err(e) = &result {
            if e.contains("session is closed") {
                tools.finish(&id);
                return Err(e.clone());
            }
        }
        let _ = events.send(CollectEvent::Output { command: command.clone(), ok: result.is_ok(), output: result.clone().unwrap_or_else(|e| e) });
        outputs.insert(index, (command.clone(), result));
    }
    tools.finish(&id);
    Ok(Collected { id: set.id, title: set.title, started, finished: chrono::Utc::now().timestamp(), cancelled, outputs: outputs.into_values().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_answers_every_collection() {
        for os in Os::all() {
            let p = profile(*os);
            for (id, _) in COLLECTION_IDS {
                let c = p.collections.iter().find(|c| c.id == *id).unwrap_or_else(|| panic!("{:?} lacks {id}", os));
                assert!(!c.commands.is_empty() || !c.note.is_empty(), "{:?} {id} has neither commands nor a note", os);
            }
            assert!(!p.configs.is_empty(), "{:?} has no config kinds", os);
            assert!(!p.workflow.restorable.is_empty());
        }
    }

    #[test]
    fn workflow_placeholders_fill() {
        let w = profile(Os::Junos).workflow;
        assert_eq!(fill(&w.commit_confirmed, "x", 5), "commit confirmed 5");
        let w = profile(Os::Eos).workflow;
        assert_eq!(fill(&w.enter[0], "20260101", 5), "configure session omni-20260101");
        assert_eq!(fill(&w.commit_confirmed, "a", 5), "configure session omni-a commit timer 00:5:00");
    }
}
