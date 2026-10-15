//! Configuration templates with typed variables. Rendering validates every
//! value and returns the exact lines that would be sent, for review.

use std::collections::BTreeMap;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::detect::Os;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VarType {
    Text,
    Int,
    Vlan,
    /// comma separated VLAN list or ranges
    Vlans,
    Ip,
    /// a.b.c.d/len or "a.b.c.d m.m.m.m"
    Prefix,
    Interface,
    Choice,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Var {
    pub name: String,
    pub label: String,
    pub kind: VarType,
    pub required: bool,
    pub default: String,
    pub help: String,
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub id: String,
    pub title: String,
    pub category: String,
    pub description: String,
    pub vars: Vec<Var>,
    /// lines for IOS / IOS-XE
    pub ios: Vec<String>,
    /// lines for NX-OS (empty = not supported)
    pub nxos: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rendered {
    pub commands: Vec<String>,
    pub warnings: Vec<String>,
}

fn var(name: &str, label: &str, kind: VarType, required: bool, default: &str, help: &str) -> Var {
    Var { name: name.into(), label: label.into(), kind, required, default: default.into(), help: help.into(), choices: Vec::new() }
}

fn choice(name: &str, label: &str, choices: &[&str], default: &str, help: &str) -> Var {
    Var { name: name.into(), label: label.into(), kind: VarType::Choice, required: true, default: default.into(), help: help.into(), choices: choices.iter().map(|s| s.to_string()).collect() }
}

fn lines(s: &str) -> Vec<String> {
    s.lines().map(|l| l.to_string()).filter(|l| !l.trim().is_empty()).collect()
}

/// Every template. Line syntax: `{{var}}` substitutes; a line starting with
/// `?var ` is kept only when var is true/non-empty, `!var ` only when it is not.
pub fn catalog() -> Vec<Template> {
    let iface = || var("if", "Interface(s)", VarType::Interface, true, "", "One interface or a range: Gi1/0/1 or Gi1/0/1-24");
    let desc = || var("desc", "Description", VarType::Text, false, "", "");
    vec![
        Template {
            id: "access-port".into(), title: "Access port".into(), category: "Ports".into(),
            description: "A user or server port in one VLAN, with PortFast and BPDU guard.".into(),
            vars: vec![iface(), var("vlan", "Access VLAN", VarType::Vlan, true, "", ""), desc(), var("portsec", "Port security (max 2 MACs)", VarType::Bool, false, "", "Sticky MACs, shutdown on violation"), var("stormcontrol", "Storm control", VarType::Bool, false, "true", "Broadcast 5 %")],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport mode access\n switchport access vlan {{vlan}}\n switchport nonegotiate\n spanning-tree portfast\n spanning-tree bpduguard enable\n?portsec  switchport port-security maximum 2\n?portsec  switchport port-security mac-address sticky\n?portsec  switchport port-security\n?stormcontrol  storm-control broadcast level 5.00\n no shutdown\nend"),
            nxos: lines("configure terminal\ninterface {{if}}\n description {{desc}}\n switchport\n switchport mode access\n switchport access vlan {{vlan}}\n spanning-tree port type edge\n spanning-tree bpduguard enable\n?stormcontrol  storm-control broadcast level 5.00\n no shutdown\nend"),
        },
        Template {
            id: "ap-port".into(), title: "Access point port".into(), category: "Ports".into(),
            description: "Trunk to a wireless AP with a native management VLAN, or an access port for FlexConnect/local switching off.".into(),
            vars: vec![iface(), var("mgmt", "Management (native) VLAN", VarType::Vlan, true, "", ""), var("vlans", "Allowed VLANs", VarType::Vlans, true, "", "e.g. 10,20,30-35; include the management VLAN"), desc(), var("poe", "PoE priority high", VarType::Bool, false, "true", "")],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport mode trunk\n switchport trunk native vlan {{mgmt}}\n switchport trunk allowed vlan {{vlans}}\n switchport nonegotiate\n spanning-tree portfast trunk\n?poe  power inline port priority high\n no shutdown\nend"),
            nxos: lines("configure terminal\ninterface {{if}}\n description {{desc}}\n switchport\n switchport mode trunk\n switchport trunk native vlan {{mgmt}}\n switchport trunk allowed vlan {{vlans}}\n spanning-tree port type edge trunk\n no shutdown\nend"),
        },
        Template {
            id: "camera-port".into(), title: "Camera port".into(), category: "Ports".into(),
            description: "IP camera: access VLAN, port security to one MAC, PoE, QoS trust.".into(),
            vars: vec![iface(), var("vlan", "Camera VLAN", VarType::Vlan, true, "", ""), desc()],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport mode access\n switchport access vlan {{vlan}}\n switchport nonegotiate\n switchport port-security maximum 1\n switchport port-security mac-address sticky\n switchport port-security violation restrict\n switchport port-security\n spanning-tree portfast\n spanning-tree bpduguard enable\n power inline port priority high\n no shutdown\nend"),
            nxos: vec![],
        },
        Template {
            id: "voice-port".into(), title: "Voice + data port".into(), category: "Ports".into(),
            description: "A phone with a PC behind it: data VLAN plus voice VLAN, QoS trust for the phone.".into(),
            vars: vec![iface(), var("data", "Data VLAN", VarType::Vlan, true, "", ""), var("voice", "Voice VLAN", VarType::Vlan, true, "", ""), desc()],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport mode access\n switchport access vlan {{data}}\n switchport voice vlan {{voice}}\n switchport nonegotiate\n spanning-tree portfast\n spanning-tree bpduguard enable\n trust device cisco-phone\n auto qos voip cisco-phone\n no shutdown\nend"),
            nxos: vec![],
        },
        Template {
            id: "trunk-port".into(), title: "Trunk port".into(), category: "Ports".into(),
            description: "802.1Q trunk to another switch with a dedicated native VLAN and a pruned allowed list.".into(),
            vars: vec![iface(), var("native", "Native VLAN", VarType::Vlan, true, "999", "Use an unused VLAN, not 1"), var("vlans", "Allowed VLANs", VarType::Vlans, true, "", ""), desc()],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n switchport mode trunk\n switchport nonegotiate\n no shutdown\nend"),
            nxos: lines("configure terminal\ninterface {{if}}\n description {{desc}}\n switchport\n switchport mode trunk\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n spanning-tree port type network\n no shutdown\nend"),
        },
        Template {
            id: "uplink-port".into(), title: "Uplink to core".into(), category: "Ports".into(),
            description: "Trunk uplink with root guard off, loop guard on, and UDLD.".into(),
            vars: vec![iface(), var("native", "Native VLAN", VarType::Vlan, true, "999", ""), var("vlans", "Allowed VLANs", VarType::Vlans, true, "", ""), desc(), var("udld", "UDLD aggressive", VarType::Bool, false, "true", "Fiber links")],
            ios: lines("configure terminal\ninterface range {{if}}\n description {{desc}}\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n switchport mode trunk\n switchport nonegotiate\n spanning-tree guard loop\n?udld  udld port aggressive\n no shutdown\nend"),
            nxos: lines("configure terminal\ninterface {{if}}\n description {{desc}}\n switchport\n switchport mode trunk\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n spanning-tree port type network\n spanning-tree guard loop\n?udld  udld aggressive\n no shutdown\nend"),
        },
        Template {
            id: "port-channel".into(), title: "Port channel (LACP)".into(), category: "Ports".into(),
            description: "Bundle interfaces into an LACP port channel carrying a trunk.".into(),
            vars: vec![var("if", "Member interfaces", VarType::Interface, true, "", "e.g. Gi1/0/49-50 or Te1/1/1,Te2/1/1"), var("po", "Port-channel number", VarType::Int, true, "", "1-128"), var("native", "Native VLAN", VarType::Vlan, true, "999", ""), var("vlans", "Allowed VLANs", VarType::Vlans, true, "", ""), desc()],
            ios: lines("configure terminal\ninterface Port-channel{{po}}\n description {{desc}}\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n switchport mode trunk\n switchport nonegotiate\ninterface range {{if}}\n description {{desc}} member\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n switchport mode trunk\n switchport nonegotiate\n channel-group {{po}} mode active\n no shutdown\nend"),
            nxos: lines("configure terminal\nfeature lacp\ninterface port-channel{{po}}\n description {{desc}}\n switchport\n switchport mode trunk\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\ninterface {{if}}\n description {{desc}} member\n switchport\n switchport mode trunk\n switchport trunk native vlan {{native}}\n switchport trunk allowed vlan {{vlans}}\n channel-group {{po}} mode active\n no shutdown\nend"),
        },
        Template {
            id: "vlan".into(), title: "VLAN".into(), category: "Layer 2".into(),
            description: "Create a VLAN with a name, and optionally its SVI.".into(),
            vars: vec![var("vlan", "VLAN", VarType::Vlan, true, "", ""), var("name", "Name", VarType::Text, true, "", "No spaces"), var("svi", "Create an SVI", VarType::Bool, false, "", ""), var("prefix", "SVI address", VarType::Prefix, false, "", "10.1.10.1/24"), var("helper", "DHCP helper (relay) address", VarType::Ip, false, "", "")],
            ios: lines("configure terminal\nvlan {{vlan}}\n name {{name}}\n?svi interface Vlan{{vlan}}\n?svi  description {{name}}\n?svi  ip address {{prefix.ip}} {{prefix.mask}}\n?helper  ip helper-address {{helper}}\n?svi  no shutdown\nend"),
            nxos: lines("configure terminal\nvlan {{vlan}}\n name {{name}}\n?svi feature interface-vlan\n?svi interface Vlan{{vlan}}\n?svi  description {{name}}\n?svi  ip address {{prefix}}\n?helper  ip dhcp relay address {{helper}}\n?svi  no shutdown\nend"),
        },
        Template {
            id: "initial-setup".into(), title: "Initial setup".into(), category: "Provisioning".into(),
            description: "A new switch from the console: hostname, management SVI, gateway, DNS, NTP, SSH, a local admin, logging and SNMP.".into(),
            vars: vec![
                var("hostname", "Hostname", VarType::Text, true, "", ""),
                var("domain", "Domain name", VarType::Text, true, "", "Needed for SSH keys"),
                var("mgmtvlan", "Management VLAN", VarType::Vlan, true, "1", ""),
                var("mgmt", "Management address", VarType::Prefix, true, "", "10.1.1.10/24"),
                var("vrf", "Management VRF", VarType::Text, false, "", "Leave empty for the global table"),
                var("gateway", "Default gateway", VarType::Ip, true, "", ""),
                var("dns", "DNS server", VarType::Ip, true, "", ""),
                var("dns2", "Second DNS server", VarType::Ip, false, "", ""),
                var("ntp", "NTP server", VarType::Ip, true, "", ""),
                var("timezone", "Time zone", VarType::Text, true, "UTC 0", "e.g. CET 1, EST -5"),
                var("admin", "Admin user", VarType::Text, true, "admin", "Password is asked for when deploying; it is never stored"),
                var("syslog", "Syslog server", VarType::Ip, false, "", ""),
                var("snmp", "SNMPv3 user", VarType::Text, false, "", "Auth and privacy passwords asked for when deploying"),
                var("banner", "Login banner", VarType::Text, false, "Authorized access only", ""),
                choice("vtp", "VTP", &["transparent", "off", "keep"], "transparent", "Transparent avoids inheriting a VLAN database"),
            ],
            ios: lines("configure terminal\nhostname {{hostname}}\nip domain name {{domain}}\nno ip domain lookup\nservice password-encryption\nservice timestamps log datetime msec localtime show-timezone\nservice timestamps debug datetime msec localtime show-timezone\nclock timezone {{timezone}}\n?vrf vrf definition {{vrf}}\n?vrf  address-family ipv4\n?vrf  exit-address-family\nvlan {{mgmtvlan}}\n name MGMT\ninterface Vlan{{mgmtvlan}}\n description Management\n?vrf  vrf forwarding {{vrf}}\n ip address {{mgmt.ip}} {{mgmt.mask}}\n no shutdown\n!vrf ip default-gateway {{gateway}}\n!vrf ip route 0.0.0.0 0.0.0.0 {{gateway}}\n?vrf ip route vrf {{vrf}} 0.0.0.0 0.0.0.0 {{gateway}}\n!vrf ip name-server {{dns}}\n?vrf ip name-server vrf {{vrf}} {{dns}}\n?dns2 ip name-server {{dns2}}\n!vrf ntp server {{ntp}}\n?vrf ntp server vrf {{vrf}} {{ntp}}\nusername {{admin}} privilege 15 secret {{admin.password}}\nenable secret {{enable.password}}\naaa new-model\naaa authentication login default local\naaa authorization exec default local\ncrypto key generate rsa modulus 2048\nip ssh version 2\nip ssh time-out 60\nline vty 0 15\n transport input ssh\n exec-timeout 15 0\n logging synchronous\nline con 0\n exec-timeout 15 0\n logging synchronous\n?syslog logging host {{syslog}}\n?syslog logging trap informational\nlogging buffered 64000 informational\n?snmp snmp-server group OMNI v3 priv\n?snmp snmp-server user {{snmp}} OMNI v3 auth sha {{snmp.auth}} priv aes 128 {{snmp.priv}}\n?banner banner motd ^{{banner}}^\n?vtp.transparent vtp mode transparent\n?vtp.off vtp mode off\nno ip http server\nno ip http secure-server\nspanning-tree mode rapid-pvst\nspanning-tree portfast bpduguard default\nerrdisable recovery cause bpduguard\nerrdisable recovery interval 300\nend"),
            nxos: lines("configure terminal\nhostname {{hostname}}\nip domain-name {{domain}}\nclock timezone {{timezone}}\nfeature interface-vlan\nfeature ssh\nvlan {{mgmtvlan}}\n name MGMT\ninterface Vlan{{mgmtvlan}}\n description Management\n no shutdown\n ip address {{mgmt}}\nip route 0.0.0.0/0 {{gateway}}\nip name-server {{dns}}\n?dns2 ip name-server {{dns2}}\nntp server {{ntp}}\nusername {{admin}} password {{admin.password}} role network-admin\nssh key rsa 2048 force\n?syslog logging server {{syslog}} 6\n?snmp snmp-server user {{snmp}} network-admin auth sha {{snmp.auth}} priv aes-128 {{snmp.priv}}\n?banner banner motd ^{{banner}}^\nspanning-tree port type edge bpduguard default\nend"),
        },
    ]
}

/// Variables rendered from secrets the UI collects at deploy time (never stored).
pub const SECRET_VARS: &[(&str, &str, &str)] = &[
    ("admin.password", "initial-setup", "Password for the admin user"),
    ("enable.password", "initial-setup", "Enable secret"),
    ("snmp.auth", "initial-setup", "SNMPv3 authentication password (8+ characters)"),
    ("snmp.priv", "initial-setup", "SNMPv3 privacy password (8+ characters)"),
];

fn ip_ok(s: &str) -> bool {
    s.parse::<std::net::Ipv4Addr>().is_ok()
}

/// "a.b.c.d/len" or "a.b.c.d m.m.m.m" → (ip, mask, len)
fn prefix_parts(s: &str) -> Option<(String, String, u8)> {
    let s = s.trim();
    if let Some((ip, len)) = s.split_once('/') {
        let len: u8 = len.parse().ok()?;
        if !ip_ok(ip) || len > 32 {
            return None;
        }
        let m: u32 = if len == 0 { 0 } else { u32::MAX << (32 - len) };
        return Some((ip.to_string(), std::net::Ipv4Addr::from(m).to_string(), len));
    }
    if let Some((ip, mask)) = s.split_once(' ') {
        let (ip, mask) = (ip.trim(), mask.trim());
        if !ip_ok(ip) || !ip_ok(mask) {
            return None;
        }
        let m = u32::from(mask.parse::<std::net::Ipv4Addr>().ok()?);
        let len = m.count_ones() as u8;
        if m != (if len == 0 { 0 } else { u32::MAX << (32 - len) }) {
            return None;
        }
        return Some((ip.to_string(), mask.to_string(), len));
    }
    None
}

fn vlan_ok(s: &str) -> bool {
    s.parse::<u16>().is_ok_and(|v| (1..=4094).contains(&v))
}

fn vlans_ok(s: &str) -> bool {
    !s.trim().is_empty()
        && s.split(',').all(|part| {
            let p = part.trim();
            match p.split_once('-') {
                Some((a, b)) => vlan_ok(a) && vlan_ok(b) && a.parse::<u16>().unwrap() < b.parse::<u16>().unwrap(),
                None => vlan_ok(p),
            }
        })
}

fn iface_ok(s: &str) -> bool {
    let re = Regex::new(r"(?i)^(gi|te|fa|fo|hu|tw|twe|eth|ethernet|gigabitethernet|tengigabitethernet|fastethernet|fortygigabitethernet|hundredgige|twentyfivegige|port-channel|po)[\d/]+(\s*-\s*\d+)?(\s*,\s*(gi|te|fa|fo|hu|tw|twe|eth|ethernet|gigabitethernet|tengigabitethernet|fastethernet|port-channel|po)?[\d/]+(\s*-\s*\d+)?)*$").unwrap();
    re.is_match(s.trim())
}

fn truthy(v: &str) -> bool {
    !matches!(v.trim().to_lowercase().as_str(), "" | "false" | "0" | "no" | "off")
}

/// Validate and render. Secret variables (`admin.password`) come in `values` too.
pub fn render(id: &str, os: Os, values: &BTreeMap<String, String>) -> Result<Rendered, Vec<FieldError>> {
    let Some(t) = catalog().into_iter().find(|t| t.id == id) else {
        return Err(vec![FieldError { name: "template".into(), message: format!("unknown template {id}") }]);
    };
    let mut errors = Vec::new();
    let mut vals: BTreeMap<String, String> = values.clone();
    for v in &t.vars {
        let raw = values.get(&v.name).cloned().unwrap_or_else(|| v.default.clone());
        let raw = raw.trim().to_string();
        if raw.is_empty() {
            if v.required {
                errors.push(FieldError { name: v.name.clone(), message: format!("{} is required", v.label) });
            }
            vals.insert(v.name.clone(), String::new());
            continue;
        }
        let ok = match v.kind {
            VarType::Text => !raw.contains(['\n', '\r', '?']) && !(v.name == "hostname" && !Regex::new(r"^[A-Za-z][A-Za-z0-9\-_.]{0,62}$").unwrap().is_match(&raw)) && !(v.name == "name" && raw.contains(' ')),
            VarType::Int => raw.parse::<i64>().is_ok_and(|n| n >= 0 && (v.name != "po" || (1..=4096).contains(&n))),
            VarType::Vlan => vlan_ok(&raw),
            VarType::Vlans => vlans_ok(&raw),
            VarType::Ip => ip_ok(&raw),
            VarType::Prefix => prefix_parts(&raw).is_some(),
            VarType::Interface => iface_ok(&raw),
            VarType::Choice => v.choices.iter().any(|c| c == &raw),
            VarType::Bool => true,
        };
        if !ok {
            errors.push(FieldError { name: v.name.clone(), message: format!("{} is not valid", v.label) });
        }
        if v.kind == VarType::Prefix {
            if let Some((ip, mask, len)) = prefix_parts(&raw) {
                vals.insert(format!("{}.ip", v.name), ip.clone());
                vals.insert(format!("{}.mask", v.name), mask);
                vals.insert(v.name.clone(), format!("{ip}/{len}"));
                continue;
            }
        }
        if v.kind == VarType::Choice {
            for c in &v.choices {
                vals.insert(format!("{}.{}", v.name, c), if c == &raw { "true".into() } else { String::new() });
            }
        }
        vals.insert(v.name.clone(), raw);
    }
    if t.id == "initial-setup" {
        for (name, _, label) in SECRET_VARS {
            let v = values.get(*name).map(|s| s.trim()).unwrap_or("");
            let needed = !name.starts_with("snmp") || !vals.get("snmp").is_none_or(|s| s.is_empty());
            if needed && v.is_empty() {
                errors.push(FieldError { name: name.to_string(), message: format!("{label} is required") });
            } else if needed && name.starts_with("snmp") && v.len() < 8 {
                errors.push(FieldError { name: name.to_string(), message: format!("{label}: 8 characters or more") });
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let body = match os {
        Os::NxOs => &t.nxos,
        _ => &t.ios,
    };
    let mut warnings = Vec::new();
    if body.is_empty() {
        return Err(vec![FieldError { name: "template".into(), message: format!("{} has no {} version", t.title, os.label()) }]);
    }
    if !matches!(os, Os::Ios | Os::IosXe | Os::NxOs) {
        warnings.push(format!("Rendered with IOS syntax; check it against {} before deploying.", os.label()));
    }
    if os == Os::NxOs && vals.get("if").is_some_and(|s| s.contains('-') || s.contains(',')) {
        warnings.push("NX-OS: interface ranges use `interface Ethernet1/1-4` syntax; check the range.".into());
    }
    let sub = Regex::new(r"\{\{([\w.]+)\}\}").unwrap();
    let mut commands = Vec::new();
    for line in body {
        let mut l = line.as_str();
        if let Some(rest) = l.strip_prefix('?') {
            let (flag, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            if !vals.get(flag).is_some_and(|v| truthy(v)) {
                continue;
            }
            l = rest;
        } else if let Some(rest) = l.strip_prefix('!') {
            let (flag, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            if vals.get(flag).is_some_and(|v| truthy(v)) {
                continue;
            }
            l = rest;
        }
        let out = sub.replace_all(l, |c: &regex::Captures| vals.get(&c[1]).cloned().unwrap_or_default()).to_string();
        // a description line with nothing to say is dropped
        if out.trim() == "description" || out.trim().ends_with(" member") && out.trim() == "description member" {
            continue;
        }
        if out.trim().is_empty() {
            continue;
        }
        commands.push(out.trim_end().to_string());
    }
    Ok(Rendered { commands, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn access_port_renders_and_validates() {
        let r = render("access-port", Os::IosXe, &vals(&[("if", "Gi1/0/1-24"), ("vlan", "10"), ("desc", "users"), ("portsec", "true")])).unwrap();
        assert!(r.commands.contains(&"interface range Gi1/0/1-24".to_string()));
        assert!(r.commands.contains(&" switchport port-security".to_string()));
        assert!(r.commands.contains(&" storm-control broadcast level 5.00".to_string()));
        assert_eq!(r.commands.last().unwrap(), "end");
        let e = render("access-port", Os::Ios, &vals(&[("if", "Gi1/0/1; reload"), ("vlan", "5000")])).unwrap_err();
        assert_eq!(e.len(), 2);
        // no description line when empty
        let r = render("access-port", Os::Ios, &vals(&[("if", "Gi1/0/1"), ("vlan", "10"), ("stormcontrol", "")])).unwrap();
        assert!(!r.commands.iter().any(|c| c.trim() == "description"));
        assert!(!r.commands.iter().any(|c| c.contains("storm-control")));
    }

    #[test]
    fn vlan_svi_and_setup() {
        let r = render("vlan", Os::Ios, &vals(&[("vlan", "20"), ("name", "VOICE"), ("svi", "true"), ("prefix", "10.1.20.1/24"), ("helper", "10.9.9.9")])).unwrap();
        assert!(r.commands.contains(&" ip address 10.1.20.1 255.255.255.0".to_string()));
        assert!(r.commands.contains(&" ip helper-address 10.9.9.9".to_string()));
        let r = render("vlan", Os::NxOs, &vals(&[("vlan", "20"), ("name", "VOICE"), ("svi", "true"), ("prefix", "10.1.20.1 255.255.255.0")])).unwrap();
        assert!(r.commands.contains(&" ip address 10.1.20.1/24".to_string()));
        let e = render("initial-setup", Os::Ios, &vals(&[("hostname", "sw1"), ("domain", "x.net"), ("mgmt", "10.0.0.5/24"), ("gateway", "10.0.0.1"), ("dns", "1.1.1.1"), ("ntp", "10.0.0.1")])).unwrap_err();
        assert!(e.iter().any(|f| f.name == "admin.password"));
        assert!(!e.iter().any(|f| f.name == "snmp.auth"));
        let r = render("initial-setup", Os::Ios, &vals(&[("hostname", "sw1"), ("domain", "x.net"), ("mgmt", "10.0.0.5/24"), ("gateway", "10.0.0.1"), ("dns", "1.1.1.1"), ("ntp", "10.0.0.1"), ("admin.password", "p"), ("enable.password", "e"), ("vrf", "MGMT")])).unwrap();
        assert!(r.commands.contains(&"ip route vrf MGMT 0.0.0.0 0.0.0.0 10.0.0.1".to_string()));
        assert!(!r.commands.contains(&"ip default-gateway 10.0.0.1".to_string()));
        assert!(r.commands.contains(&"vtp mode transparent".to_string()));
        assert!(render("camera-port", Os::NxOs, &vals(&[("if", "Eth1/1"), ("vlan", "30")])).is_err());
    }
}
