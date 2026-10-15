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
    /// Aruba / HPE ProCurve switches (ArubaOS-Switch)
    #[serde(rename = "aos-s")]
    ArubaOsS,
    /// Aruba CX
    #[serde(rename = "aos-cx")]
    ArubaCx,
    Junos,
    /// Arista EOS
    Eos,
    /// Cisco Identity Services Engine (ADE-OS CLI)
    Ise,
    /// Cisco AireOS wireless controllers (legacy WLC)
    Aireos,
    /// SONiC (Cisco S1 distribution and community)
    Sonic,
    /// NVIDIA Cumulus Linux
    Cumulus,
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
            Os::ArubaOsS => "Aruba AOS-S",
            Os::ArubaCx => "Aruba AOS-CX",
            Os::Ise => "Cisco ISE",
            Os::Aireos => "Cisco AireOS",
            Os::Junos => "Juniper Junos",
            Os::Eos => "Arista EOS",
            Os::Sonic => "SONiC",
            Os::Cumulus => "NVIDIA Cumulus Linux",
            Os::Unknown => "Unknown",
        }
    }

    pub fn is_cisco(self) -> bool {
        matches!(self, Os::Ios | Os::IosXe | Os::NxOs | Os::IosXr | Os::Asa | Os::SmallBusiness | Os::CatOs)
    }

    /// A Linux shell rather than a network CLI.
    pub fn is_shell(self) -> bool {
        matches!(self, Os::Cumulus)
    }

    pub fn all() -> &'static [Os] {
        &[Os::Ios, Os::IosXe, Os::NxOs, Os::IosXr, Os::Asa, Os::SmallBusiness, Os::CatOs, Os::Ise, Os::Aireos, Os::ArubaOsS, Os::ArubaCx, Os::Junos, Os::Eos, Os::Sonic, Os::Cumulus]
    }

    /// Command that turns the pager off for this terminal session only.
    pub fn no_paging(self) -> Option<&'static str> {
        match self {
            Os::Ios | Os::IosXe | Os::NxOs | Os::IosXr => Some("terminal length 0"),
            // ADE-OS has no terminal length; probes carry `| nomore` instead
            Os::Ise => None,
            Os::Aireos => Some("config paging disable"),
            Os::Asa => Some("terminal pager 0"),
            Os::SmallBusiness => Some("terminal datadump"),
            Os::CatOs => Some("set length 0"),
            Os::ArubaOsS | Os::ArubaCx => Some("no page"),
            Os::Junos => Some("set cli screen-length 0"),
            Os::Eos | Os::Sonic => Some("terminal length 0"),
            Os::Cumulus | Os::Unknown => None,
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
    /// what the box does when the OS alone does not say: "wireless-controller" for a Catalyst 9800
    #[serde(default)]
    pub role: String,
}

/// AireOS answers `show version` with a usage error; its own command is `show sysinfo`.
pub const WLC_PROBE: &str = "show sysinfo";

pub fn looks_like_aireos(output: &str) -> bool {
    output.contains("Incorrect usage") || output.contains("(Cisco Controller)") || output.contains("Incorrect input!")
}

fn cap(re: &str, text: &str) -> String {
    Regex::new(re).ok().and_then(|r| r.captures(text)).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_string()).unwrap_or_default()
}

/// The second thing to ask when `show version` did not identify the device:
/// a Linux shell (Cumulus, SONiC's bash) answers this.
pub const SHELL_PROBE: &str = "cat /etc/lsb-release /etc/os-release 2>/dev/null; cat /etc/sonic/sonic_version.yml 2>/dev/null | head -5";

/// `show version` came back from a shell, not a network CLI.
pub fn looks_like_shell(output: &str) -> bool {
    let t = output.trim();
    t.contains("command not found") || t.contains("No such file") || t.starts_with("-bash:") || t.starts_with("bash:") || t.starts_with("sh:")
}

pub fn detect_os(show_version: &str) -> Os {
    let t = show_version;
    if t.contains("Cisco Application Deployment Engine") || t.contains("Identity Services Engine") || t.contains("ADE-OS") {
        Os::Ise
    } else if t.contains("Product Name") && t.contains("Cisco Controller") || t.contains("AireOS") {
        Os::Aireos
    } else if t.contains("Cumulus Linux") || t.contains("DISTRIB_ID=\"Cumulus") || t.contains("ID=cumulus") {
        Os::Cumulus
    } else if t.contains("SONiC") || t.contains("sonic") && t.contains("build_version") {
        Os::Sonic
    } else if t.contains("Junos") || t.contains("JUNOS") {
        Os::Junos
    } else if t.contains("Arista") && (t.contains("Software image version") || t.contains("EOS")) {
        Os::Eos
    } else if t.contains("ArubaOS-CX") || t.contains("Aruba CX") || t.contains("AOS-CX") {
        Os::ArubaCx
    } else if t.contains("ArubaOS") && (t.contains("MODEL:") || t.contains("Aruba Operating System") || t.contains("Mobility")) {
        // a Mobility Controller or Instant AP: not a switch, so no AOS-S commands
        Os::Unknown
    } else if t.contains("ArubaOS") || t.contains("ProCurve") || t.contains("Software revision") && (t.contains("Aruba") || t.contains("HP ")) {
        Os::ArubaOsS
    } else if t.contains("Cisco Nexus") || t.contains("NX-OS") || t.contains("NXOS:") {
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
    if os == Os::IosXe && (t.contains("C9800") || t.contains("Wireless LAN Controller") || t.contains("WLC")) {
        f.role = "wireless-controller".into();
    }
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
        Os::Ise => {
            // Cisco Application Deployment Engine OS Release: 3.2
            // Cisco Identity Services Engine ... Version : 3.2.0.542 ; Build Date
            f.version = cap(r"(?m)^\s*Version\s*:\s*(\S+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)OS Release:\s*(\S+)", t);
            }
            f.image = cap(r"(?m)ADE-OS Build Version:\s*(\S+)", t);
            f.hostname = cap(r"(?m)^\s*Hostname\s*:\s*(\S+)", t);
            f.model = "ISE node".into();
            f.role = "identity-services".into();
        }
        Os::Aireos => {
            // Product Name..... Cisco Controller / Product Version..... 8.10.185.0 / System Name..... WLC1 / System Up Time.... 120 days 3 hrs
            f.version = cap(r"(?m)^Product Version\.*\s*(\S+)", t);
            f.hostname = cap(r"(?m)^System Name\.*\s*(\S+)", t);
            f.uptime = cap(r"(?m)^System Up Time\.*\s*(.+?)\s*$", t);
            f.model = cap(r"(?m)^Machine Model\.*\s*(\S+)", t);
            f.serial = cap(r"(?m)^(?:Serial Number|Burned-in MAC Address)\.*\s*(\S+)", t);
            f.image = cap(r"(?m)^Bootloader Version\.*\s*(\S+)", t);
            f.role = "wireless-controller".into();
        }
        Os::ArubaOsS => {
            // Software revision  : WC.16.11.0012 ; "Aruba JL322A 2930M-48G-PoE+ Switch"
            f.version = cap(r"(?m)Software revision\s*:\s*(\S+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)Image stamp:[\s\S]*?\n\s*(\S+)\s*$", t);
            }
            f.model = cap(r"(?m)^\s*(?:Aruba|HP|HPE)\s+(J\w{4}A?\s+\S+(?: \S+)*?)\s+Switch", t);
            if f.model.is_empty() {
                f.model = cap(r"(?m)^\s*(?:Aruba|HP|HPE)\s+(J\w{4}A?\s+[^\n]+)", t);
            }
            f.hostname = cap(r"(?m)^\s*System Name\s*:\s*(.+?)\s*$", t);
            f.uptime = cap(r"(?m)^\s*Up Time\s*:\s*(.+?)\s*$", t);
            f.serial = cap(r"(?m)^\s*Serial Number\s*:\s*(\S+)", t);
            f.image = cap(r"(?m)^\s*(?:Build|Boot) Image\s*:\s*(.+?)\s*$", t);
        }
        Os::ArubaCx => {
            // ArubaOS-CX  Version : FL.10.13.1010 ; Product Name : 6300M 48G ...
            f.version = cap(r"(?m)^\s*Version\s*:\s*(\S+)", t);
            f.model = cap(r"(?m)^\s*Product Name\s*:\s*(.+?)\s*$", t);
            f.hostname = cap(r"(?m)^\s*Hostname\s*:\s*(\S+)", t);
            f.uptime = cap(r"(?m)^\s*Up Time\s*:\s*(.+?)\s*$", t);
            f.serial = cap(r"(?m)^\s*(?:Chassis )?Serial (?:Nbr|Number)\s*:\s*(\S+)", t);
            f.image = cap(r"(?m)^\s*Build Date\s*:\s*(.+?)\s*$", t);
        }
        Os::Junos => {
            // Hostname: core1 / Model: ex4300-48p / Junos: 21.4R3-S4.9
            f.hostname = cap(r"(?m)^Hostname:\s*(\S+)", t);
            f.model = cap(r"(?m)^Model:\s*(\S+)", t);
            f.version = cap(r"(?m)^Junos:\s*(\S+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)JUNOS \S+ \[(\S+)\]", t);
            }
            f.serial = cap(r"(?m)^Chassis\s+(\S+)\s+\S+", t);
            f.uptime = cap(r"(?m)System booted:\s*.*?\(([^)]+) ago\)", t);
            if f.uptime.is_empty() {
                f.uptime = cap(r"(?m)up (\d+ days?,\s*[\d:]+|[\d:]+)", t);
            }
            f.image = cap(r"(?m)^Junos:\s*(\S+)", t);
        }
        Os::Eos => {
            // Arista DCS-7050SX3-48YC8-R / Software image version: 4.29.2F / Uptime: 10 days, ...
            f.model = cap(r"(?m)^(?:Arista\s+)?(DCS-\S+|CCS-\S+|vEOS\S*|\S+-\S+)\s*$", t);
            f.version = cap(r"(?m)^Software image version:\s*(\S+)", t);
            f.serial = cap(r"(?m)^Serial number:\s*(\S+)", t);
            f.uptime = cap(r"(?m)^Uptime:\s*(.+?)\s*$", t);
            f.image = cap(r"(?m)^Architecture:\s*(\S+)", t);
        }
        Os::Sonic => {
            f.version = cap(r"(?m)SONiC Software Version:\s*(\S+)", t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)build_version:\s*'?([^'\n]+)", t);
            }
            f.model = cap(r"(?m)^\s*HwSKU:\s*(\S+)", t);
            if f.model.is_empty() {
                f.model = cap(r"(?m)^\s*Platform:\s*(\S+)", t);
            }
            f.serial = cap(r"(?m)^\s*Serial Number:\s*(\S+)", t);
            f.uptime = cap(r"(?m)^\s*Uptime:\s*(.+?)\s*$", t);
            f.image = cap(r"(?m)^\s*Distribution:\s*(.+?)\s*$", t);
        }
        Os::Cumulus => {
            // DISTRIB_RELEASE=5.9.0 or "Cumulus Linux 4.4.5" or nv show system
            f.version = cap(r#"(?m)DISTRIB_RELEASE="?([\d.]+)"#, t);
            if f.version.is_empty() {
                f.version = cap(r"(?m)Cumulus Linux\s+v?([\d.]+)", t);
            }
            if f.version.is_empty() {
                f.version = cap(r"(?m)^\s*version\s+(\S+)", t);
            }
            f.hostname = cap(r"(?m)^\s*hostname\s+(\S+)", t);
            f.model = cap(r"(?m)^\s*(?:model|Model)\s*[: ]\s*(.+?)\s*$", t);
            f.uptime = cap(r"(?m)^\s*uptime\s+(.+?)\s*$", t);
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

    const AOSS: &str = "Image stamp:    /ws/swbuildm/rel_ukiah_qaoff/code/build/lvm(swbuildm_rel_ukiah_qaoff_rel_ukiah)\n                Jul  6 2023 10:11:57\n                WC.16.11.0012\n                1023\nBoot Image:     Primary\n\nBoot ROM Version:    WC.16.01.0010\nActive Boot ROM:     WC.16.01.0010\n\n Status and Counters - General System Information\n\n  System Name        : sw-floor2\n  System Contact     :\n  Software revision  : WC.16.11.0012\n  Up Time            : 120 days\n  Serial Number      : CN12ABC345\n  Aruba JL322A 2930M-48G-PoE+ Switch\n";
    const CX: &str = "ArubaOS-CX\n(c) Copyright 2017-2023 Hewlett Packard Enterprise Development LP\n\nVersion      : FL.10.13.1010\nBuild Date   : 2023-11-06 09:32:01 PDT\nBuild ID     : ArubaOS-CX:FL.10.13.1010:f7a2:20231106\nBuild SHA    : f7a2b\nActive Image : primary\n\nService OS Version : FL.01.13.0001\nBIOS Version       : FL.01.0003\n\nHostname      : core-cx1\nProduct Name  : 6300M 48G CL4 PoE 4SFP56 Swch\nChassis Serial Nbr : SG1ZK7D0AB\nUp Time       : 45 days, 3 hours, 10 minutes\n";
    const JUNOS: &str = "Hostname: edge-ex1\nModel: ex4300-48p\nJunos: 21.4R3-S4.9\nJUNOS OS Kernel 64-bit  [20231020.ba4fb1a_builder_stable_12_212]\nJUNOS OS libs [20231020.ba4fb1a_builder_stable_12_212]\n";
    const EOS: &str = "Arista DCS-7050SX3-48YC8-R\nHardware version: 11.00\nSerial number: JPE19283746\nHardware MAC address: 2899.3a1b.2c3d\nSystem MAC address: 2899.3a1b.2c3d\n\nSoftware image version: 4.29.2F\nArchitecture: x86_64\nInternal build version: 4.29.2F-32308478.4292F\nInternal build ID: 1234\nImage format version: 3.0\nImage optimization: Default\n\nUptime: 10 days, 4 hours and 2 minutes\nTotal memory: 8098976 kB\nFree memory: 5560316 kB\n";
    const SONIC: &str = "SONiC Software Version: SONiC.4.3.0-cisco\nSONiC OS Version: 12\nDistribution: Debian 12.1\nKernel: 6.1.0\nBuild commit: 0a1b2c3\nBuild date: Tue Jan 09 2024\nBuilt by: builder\n\nPlatform: x86_64-cisco_8101_32fh_o-r0\nHwSKU: Cisco-8101-O32\nASIC: cisco-8000\nASIC Count: 1\nSerial Number: FLM2740ABCD\nModel Number: 8101-32FH-O\nHardware Revision: 1.0\nUptime: 09:23:11 up 30 days, 2:10, 1 user, load average: 1.20, 1.10, 1.05\n";
    const CUMULUS: &str = "DISTRIB_ID=\"Cumulus Linux\"\nDISTRIB_RELEASE=5.9.0\nDISTRIB_DESCRIPTION=\"Cumulus Linux 5.9.0\"\nNAME=\"Cumulus Linux\"\nVERSION_ID=5.9.0\nID=cumulus-linux\n";

    #[test]
    fn detects_ise_and_aireos() {
        let ise = "Cisco Application Deployment Engine OS Release: 3.2\nADE-OS Build Version: 3.2.0.542\nADE-OS System Architecture: x86_64\n\nCopyright (c) 2005-2022 by Cisco Systems, Inc.\nAll rights reserved.\nHostname: ise-pan1\n\nVersion information of installed applications\n---------------------------------------------\n\nCisco Identity Services Engine\n---------------------------------------------\nVersion      : 3.2.0.542\nBuild Date   : Wed Aug 31 2022\n";
        let f = parse_facts(ise);
        assert_eq!(f.os, Some(Os::Ise));
        assert_eq!(f.version, "3.2.0.542");
        assert_eq!(f.hostname, "ise-pan1");
        let wlc = "Manufacturer's Name.............................. Cisco Systems Inc.\nProduct Name..................................... Cisco Controller\nProduct Version.................................. 8.10.185.0\nBootloader Version............................... 8.5.103.0\nSystem Name...................................... WLC-HQ\nMachine Model.................................... AIR-CT5520-K9\nSystem Up Time................................... 120 days 3 hrs 10 mins 5 secs\n";
        let f = parse_facts(wlc);
        assert_eq!(f.os, Some(Os::Aireos));
        assert_eq!(f.version, "8.10.185.0");
        assert_eq!(f.model, "AIR-CT5520-K9");
        assert_eq!(f.hostname, "WLC-HQ");
        assert!(looks_like_aireos("Incorrect usage.  Use the '?' or <TAB> key to list commands."));
        let f = parse_facts("Cisco IOS XE Software, Version 17.09.04a\nCisco IOS Software [Cupertino], C9800 Software (C9800_IOSXE-K9), Version 17.9.4a\ncisco C9800-40-K9 (1RU) processor\n");
        assert_eq!(f.os, Some(Os::IosXe));
        assert_eq!(f.role, "wireless-controller");
    }

    #[test]
    fn os_wire_names_match_the_frontend() {
        assert_eq!(serde_json::to_string(&Os::ArubaCx).unwrap(), "\"aos-cx\"");
        assert_eq!(serde_json::to_string(&Os::ArubaOsS).unwrap(), "\"aos-s\"");
        assert_eq!(serde_json::to_string(&Os::IosXe).unwrap(), "\"ios-xe\"");
        assert_eq!(serde_json::to_string(&Os::SmallBusiness).unwrap(), "\"small-business\"");
    }

    #[test]
    fn detects_other_vendors() {
        assert_eq!(detect_os(AOSS), Os::ArubaOsS);
        assert_eq!(detect_os(CX), Os::ArubaCx);
        assert_eq!(detect_os(JUNOS), Os::Junos);
        assert_eq!(detect_os(EOS), Os::Eos);
        assert_eq!(detect_os(SONIC), Os::Sonic);
        assert_eq!(detect_os(CUMULUS), Os::Cumulus);
        assert!(looks_like_shell("-bash: show: command not found"));
        assert!(!looks_like_shell(IOS));
    }

    #[test]
    fn facts_other_vendors() {
        let f = parse_facts(AOSS);
        assert_eq!(f.version, "WC.16.11.0012");
        assert_eq!(f.hostname, "sw-floor2");
        assert_eq!(f.serial, "CN12ABC345");
        assert!(f.model.starts_with("JL322A 2930M"), "{}", f.model);
        let f = parse_facts(CX);
        assert_eq!(f.version, "FL.10.13.1010");
        assert_eq!(f.hostname, "core-cx1");
        assert_eq!(f.model, "6300M 48G CL4 PoE 4SFP56 Swch");
        assert_eq!(f.serial, "SG1ZK7D0AB");
        let f = parse_facts(JUNOS);
        assert_eq!(f.hostname, "edge-ex1");
        assert_eq!(f.model, "ex4300-48p");
        assert_eq!(f.version, "21.4R3-S4.9");
        let f = parse_facts(EOS);
        assert_eq!(f.model, "DCS-7050SX3-48YC8-R");
        assert_eq!(f.version, "4.29.2F");
        assert_eq!(f.serial, "JPE19283746");
        assert!(f.uptime.starts_with("10 days"));
        let f = parse_facts(SONIC);
        assert_eq!(f.version, "SONiC.4.3.0-cisco");
        assert_eq!(f.model, "Cisco-8101-O32");
        assert_eq!(f.serial, "FLM2740ABCD");
        let f = parse_facts(CUMULUS);
        assert_eq!(f.version, "5.9.0");
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
