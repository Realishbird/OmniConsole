//! Which Cisco OS is on the other end, from `show version`, plus the basic
//! facts every report starts with (hostname, model, version, uptime, serial).

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Os {
    Ios,
    IosXe,
    NxOs,
    IosXr,
    Asa,
    /// Small Business SG/CBS/SF switches
    SmallBusiness,
    CatOs,
    Unknown,
}

impl Os {
    pub fn label(self) -> &'static str {
        match self {
            Os::Ios => "Cisco IOS",
            Os::IosXe => "Cisco IOS-XE",
            Os::NxOs => "Cisco NX-OS",
            Os::IosXr => "Cisco IOS-XR",
            Os::Asa => "Cisco ASA",
            Os::SmallBusiness => "Cisco Small Business (SG/CBS)",
            Os::CatOs => "Cisco CatOS",
            Os::Unknown => "Unknown",
        }
    }

    /// Command that turns the pager off for this terminal session only.
    pub fn no_paging(self) -> Option<&'static str> {
        match self {
            Os::Ios | Os::IosXe | Os::NxOs | Os::IosXr => Some("terminal length 0"),
            Os::Asa => Some("terminal pager 0"),
            Os::SmallBusiness => Some("terminal datadump"),
            Os::CatOs => Some("set length 0"),
            Os::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub os: Option<Os>,
    pub hostname: String,
    pub model: String,
    pub version: String,
    pub image: String,
    pub uptime: String,
    pub serial: String,
    pub reload_reason: String,
}

fn cap(re: &str, text: &str) -> String {
    Regex::new(re).ok().and_then(|r| r.captures(text)).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_string()).unwrap_or_default()
}

pub fn detect_os(show_version: &str) -> Os {
    let t = show_version;
    if t.contains("Cisco Nexus") || t.contains("NX-OS") || t.contains("NXOS:") {
        Os::NxOs
    } else if t.contains("IOS XR") || t.contains("IOS-XR") {
        Os::IosXr
    } else if t.contains("Adaptive Security Appliance") || t.contains("Cisco ASA") {
        Os::Asa
    } else if t.contains("IOS XE") || t.contains("IOS-XE") || t.contains("Cisco IOS XE") {
        Os::IosXe
    } else if t.contains("WS-C") && t.contains("NmpSW") || t.contains("Catalyst Operating System") {
        Os::CatOs
    } else if t.contains("Cisco IOS Software") || t.contains("Cisco Internetwork Operating System") {
        Os::Ios
    } else if t.contains("SW version") || t.contains("Boot version") || Regex::new(r"(?m)^\s*\d+\s+(SG|SF|CBS|SX)\d").unwrap().is_match(t) {
        Os::SmallBusiness
    } else {
        Os::Unknown
    }
}

pub fn parse_facts(show_version: &str) -> Facts {
    let t = show_version;
    let os = detect_os(t);
    let mut f = Facts { os: Some(os), ..Default::default() };
    match os {
        Os::Ios | Os::IosXe => {
            // prefer the "Cisco IOS Software ... Version 17.9.4a" line over the
            // IOS XE banner's zero-padded "17.09.04a"
            f.version = cap(r"(?m)^Cisco IOS Software.*?Version ([^,\s\[]+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)Version ([^,\s\[]+)", t);
            }
            f.hostname = cap(r"(?m)^(\S+) uptime is", t);
            f.uptime = cap(r"(?m)uptime is (.+)$", t);
            f.image = cap(r#"(?m)System image file is "([^"]+)""#, t);
            f.model = cap(r"(?m)^[Cc]isco (\S+) \([^)]*\) processor", t);
            if f.model.is_empty() {
                f.model = cap(r"(?m)^Model [Nn]umber\s*:\s*(\S+)", t);
            }
            f.serial = cap(r"(?m)^System [Ss]erial [Nn]umber\s*:\s*(\S+)", t);
            if f.serial.is_empty() {
                f.serial = cap(r"(?m)Processor board ID (\S+)", t);
            }
            f.reload_reason = cap(r"(?m)Last reload reason: (.+)$", t);
        }
        Os::NxOs => {
            f.version = cap(r"(?m)^\s*(?:NXOS|system):\s+version (\S+)", t);
            f.hostname = cap(r"(?m)Device name:\s*(\S+)", t);
            f.uptime = cap(r"(?m)Kernel uptime is (.+)$", t);
            f.image = cap(r"(?m)^\s*(?:NXOS|system) image file is:\s*(\S+)", t);
            f.model = cap(r"(?m)^\s*cisco (Nexus\S* \S+)", t);
            f.serial = cap(r"(?m)Processor Board ID (\S+)", t);
            f.reload_reason = cap(r"(?m)Reason: (.+)$", t);
        }
        Os::IosXr => {
            f.version = cap(r"(?m)Version\s*:?\s*(\S+)", t);
            f.hostname = cap(r"(?m)^(\S+) uptime is", t);
            f.uptime = cap(r"(?m)uptime is (.+)$", t);
            f.model = cap(r"(?m)^cisco (\S+(?: \S+)?) \(", t);
        }
        Os::Asa => {
            f.version = cap(r"(?m)Software Version ([^\s<]+)", t);
            f.hostname = cap(r"(?m)^(\S+) up (\d+.*)$", t);
            f.uptime = cap(r"(?m)^\S+ up (\d+.*)$", t);
            f.model = cap(r"(?m)^Hardware:\s*([^,]+)", t);
            f.serial = cap(r"(?m)^Serial Number:\s*(\S+)", t);
        }
        Os::SmallBusiness => {
            f.version = cap(r"(?m)SW version\s+(\S+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)^\s*Version:\s+(\S+)", t);
            }
            if f.version.is_empty() {
                f.version = cap(r"(?m)^\s*\d+\s+\S+\s+(\d+\.\d+\S*)", t);
            }
            f.model = cap(r"(?m)^\s*\d+\s+((?:SG|SF|CBS|SX)\S+)", t);
            f.serial = cap(r"(?m)^\s*\d+\s+(?:SG|SF|CBS|SX)\S+\s+(\S+)", t);
        }
        Os::CatOs => {
            f.version = cap(r"(?m)NmpSW:\s*(\S+)", t);
            f.model = cap(r"(?m)^\s*\d+\s+\d+\s+(WS-\S+)", t);
            f.uptime = cap(r"(?m)Uptime is (.+)$", t);
        }
        Os::Unknown => {}
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const IOS: &str = "Cisco IOS Software, C2960X Software (C2960X-UNIVERSALK9-M), Version 15.2(7)E4, RELEASE SOFTWARE (fc2)\n\
Technical Support: http://www.cisco.com/techsupport\n\
ROM: Bootstrap program is C2960X boot loader\n\
sw-access-1 uptime is 1 year, 12 weeks, 3 days, 4 hours, 2 minutes\n\
System returned to ROM by power-on\n\
System image file is \"flash:/c2960x-universalk9-mz.152-7.E4/c2960x-universalk9-mz.152-7.E4.bin\"\n\
Last reload reason: power-on\n\
cisco WS-C2960X-48FPD-L (APM86XXX) processor (revision M0) with 524288K bytes of memory.\n\
Processor board ID FOC2222X0AB\n\
Model number                    : WS-C2960X-48FPD-L\n\
System serial number            : FOC2222X0AB\n";

    const XE: &str = "Cisco IOS XE Software, Version 17.09.04a\nCisco IOS Software [Cupertino], Catalyst L3 Switch Software (CAT9K_IOSXE), Version 17.9.4a, RELEASE SOFTWARE (fc3)\ncore-sw1 uptime is 20 weeks, 1 day, 7 hours\nSystem image file is \"flash:packages.conf\"\nLast reload reason: Reload Command\ncisco C9300-48P (X86) processor with 1419044K/6147K bytes of memory.\nProcessor board ID FJC2345A0BC\nModel Number                       : C9300-48P\nSystem Serial Number               : FJC2345A0BC\n";

    const NXOS: &str = "Cisco Nexus Operating System (NX-OS) Software\nSoftware\n  BIOS: version 05.47\n  NXOS: version 9.3(10)\n  NXOS image file is: bootflash:///nxos.9.3.10.bin\nHardware\n  cisco Nexus9000 C93180YC-FX Chassis\n  Processor Board ID FDO23110ABC\n  Device name: dc-leaf-1\nKernel uptime is 120 day(s), 3 hour(s), 11 minute(s), 2 second(s)\nLast reset at 123 usecs after Mon Jun 10 2025\n  Reason: Reset Requested by CLI command reload\n";

    const SG: &str = "Active-image: flash://system/images/image_tesla_hybrid_2.5.9.54_release_cisco_signed.bin\n  Version: 2.5.9.54\n  MD5 Digest: abc\n  Date: 10-Feb-2023\nUnit  Active Image  Inactive Image\n1 SG350-28P   DNI24380ABC  2.5.9.54\n";

    #[test]
    fn detects_each_os() {
        assert_eq!(detect_os(IOS), Os::Ios);
        assert_eq!(detect_os(XE), Os::IosXe);
        assert_eq!(detect_os(NXOS), Os::NxOs);
        assert_eq!(detect_os("Cisco Adaptive Security Appliance Software Version 9.16(4)\nfw1 up 3 days 2 hours\nHardware:   ASA5516, 8192 MB RAM"), Os::Asa);
        assert_eq!(detect_os("Cisco IOS XR Software, Version 7.5.2\nasr9k uptime is 1 week"), Os::IosXr);
        assert_eq!(detect_os(SG), Os::SmallBusiness);
        assert_eq!(detect_os("nothing useful"), Os::Unknown);
    }

    #[test]
    fn facts_ios_and_nxos() {
        let f = parse_facts(IOS);
        assert_eq!(f.hostname, "sw-access-1");
        assert_eq!(f.version, "15.2(7)E4");
        assert_eq!(f.model, "WS-C2960X-48FPD-L");
        assert_eq!(f.serial, "FOC2222X0AB");
        assert_eq!(f.reload_reason, "power-on");
        assert!(f.uptime.starts_with("1 year"));
        let f = parse_facts(XE);
        assert_eq!(f.model, "C9300-48P");
        assert_eq!(f.version, "17.9.4a");
        let f = parse_facts(NXOS);
        assert_eq!(f.hostname, "dc-leaf-1");
        assert_eq!(f.version, "9.3(10)");
        assert_eq!(f.model, "Nexus9000 C93180YC-FX");
        assert_eq!(f.serial, "FDO23110ABC");
        let f = parse_facts(SG);
        assert_eq!(f.model, "SG350-28P");
        assert_eq!(f.version, "2.5.9.54");
    }
}
