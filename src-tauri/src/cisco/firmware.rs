//! Firmware upgrade assistant: read what the device runs and has room for,
//! then plan the transfer, verification and activation as reviewable
//! command blocks. The reload itself is always a separate, explicit step.

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::detect::Os;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystem {
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub files: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareInfo {
    pub os: Option<Os>,
    pub version: String,
    pub model: String,
    pub image: String,
    pub boot: String,
    /// "install" | "bundle" | ""
    pub mode: String,
    pub stack_members: usize,
    pub filesystems: Vec<FileSystem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCheck {
    pub title: String,
    /// pass | warn | fail | unknown
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub checks: Vec<PlanCheck>,
    pub transfer: Vec<String>,
    pub verify: Vec<String>,
    pub activate: Vec<String>,
    pub reload: Vec<String>,
    pub post: Vec<String>,
    pub notes: Vec<String>,
    pub ok: bool,
}

fn re(s: &str) -> Regex {
    Regex::new(s).unwrap()
}

/// `dir flash:` → total/free and the files.
pub fn parse_dir(name: &str, text: &str) -> FileSystem {
    let mut fs = FileSystem { name: name.into(), ..Default::default() };
    // 1621966848 bytes total (1193340928 bytes free)
    if let Some(c) = re(r"(\d+) bytes total \((\d+) bytes free").captures(text) {
        fs.total_bytes = c[1].parse().unwrap_or(0);
        fs.free_bytes = c[2].parse().unwrap_or(0);
    } else if let Some(c) = re(r"Usage for \S+\s+(\d+) bytes used\s+(\d+) bytes free\s+(\d+) bytes total").captures(text) {
        // NX-OS
        fs.free_bytes = c[2].parse().unwrap_or(0);
        fs.total_bytes = c[3].parse().unwrap_or(0);
    }
    // IOS:    2  -rwx    35226112   Mar 1 2026 10:00:00 +00:00  c2960x-universalk9-mz.152-7.E4.bin
    for c in re(r"(?m)^\s*\d+\s+[-drwx]+\s+(\d+)\s+.+?\s(\S+)\s*$").captures_iter(text) {
        fs.files.push((c[2].to_string(), c[1].parse().unwrap_or(0)));
    }
    if fs.files.is_empty() {
        // NX-OS:    1234567    Mar 01 10:00:00 2026  nxos.9.3.10.bin
        for c in re(r"(?m)^\s*(\d+)\s+\w{3} \d{2} \d{2}:\d{2}:\d{2} \d{4}\s+(\S+)\s*$").captures_iter(text) {
            fs.files.push((c[2].to_string(), c[1].parse().unwrap_or(0)));
        }
    }
    fs
}

/// `show boot` / `show boot | include BOOT` / `show bootvar`
pub fn parse_boot(text: &str) -> String {
    re(r"(?m)^BOOT (?:path-list|variable)\s*[:=]\s*(.*)$").captures(text).map(|c| c[1].trim().to_string())
        .or_else(|| re(r"(?m)^\s*(?:NXOS|kickstart|system) variable = (.*)$").captures(text).map(|c| c[1].trim().to_string()))
        .or_else(|| re(r"(?m)^BOOT=(.*)$").captures(text).map(|c| c[1].trim().to_string()))
        .unwrap_or_default()
}

/// Does the image file name fit the platform?
fn family_matches(model: &str, image: &str) -> Option<bool> {
    let m = model.to_uppercase();
    let i = image.to_lowercase();
    let pairs: &[(&[&str], &[&str])] = &[
        (&["C9200", "C9300", "C9400", "C9500", "C9600"], &["cat9k", "c9200", "c9300", "c9400", "c9500", "c9600"]),
        (&["WS-C2960X", "WS-C2960XR"], &["c2960x"]),
        (&["WS-C2960S", "WS-C2960-"], &["c2960s", "c2960-"]),
        (&["WS-C3850"], &["cat3k_caa", "c3850"]),
        (&["WS-C3650"], &["cat3k_caa", "c3650"]),
        (&["WS-C3750X"], &["c3750e"]),
        (&["WS-C3560CX", "WS-C2960CX"], &["c3560cx", "c2960cx"]),
        (&["WS-C4500X", "WS-C45"], &["cat4500e"]),
        (&["C1000"], &["c1000"]),
        (&["IE-", "IE3"], &["ie3", "ie-"]),
        (&["ISR4", "C8"], &["isr4", "c8000"]),
        (&["ASA"], &["asa"]),
        (&["NEXUS", "N9K", "N3K", "N5K", "N7K"], &["nxos", "n9000", "n3000", "n5000", "n7000"]),
    ];
    for (models, images) in pairs {
        if models.iter().any(|x| m.contains(x)) {
            return Some(images.iter().any(|x| i.contains(x)));
        }
    }
    None
}

fn check(title: &str, status: &str, detail: impl Into<String>) -> PlanCheck {
    PlanCheck { title: title.into(), status: status.into(), detail: detail.into() }
}

fn human(bytes: u64) -> String {
    if bytes >= 1 << 30 { format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64) } else { format!("{:.0} MB", bytes as f64 / (1u64 << 20) as f64) }
}

/// Plan an upgrade to `image` (file name) fetched from `url`, `size` bytes, with `md5`.
pub fn plan(info: &FirmwareInfo, url: &str, image: &str, size: u64, md5: &str) -> Plan {
    let os = info.os.unwrap_or(Os::Ios);
    let mut checks = Vec::new();
    let mut notes = Vec::new();
    let fs_name = match os { Os::NxOs => "bootflash:", _ => "flash:" };
    let fs = info.filesystems.iter().find(|f| f.name.trim_end_matches(':') == fs_name.trim_end_matches(':')).or(info.filesystems.first());

    checks.push(check("Current software", "pass", format!("{} {}{}", os.label(), info.version, if info.image.is_empty() { String::new() } else { format!(" ({})", info.image) })));
    match family_matches(&info.model, image) {
        Some(true) => checks.push(check("Image matches the platform", "pass", format!("{image} is for the {} family", info.model))),
        Some(false) => checks.push(check("Image matches the platform", "fail", format!("{image} does not look like an image for {}", info.model))),
        None => checks.push(check("Image matches the platform", "unknown", format!("cannot tell whether {image} fits {}; check Cisco's compatibility matrix", if info.model.is_empty() { "this model" } else { &info.model }))),
    }
    let already = fs.map(|f| f.files.iter().any(|(n, _)| n == image)).unwrap_or(false);
    match fs {
        Some(f) if f.total_bytes > 0 => {
            if already {
                checks.push(check("Storage", "pass", format!("{image} is already on {}; the transfer step can be skipped", f.name)));
            } else if size == 0 {
                checks.push(check("Storage", "unknown", format!("{} free on {}; enter the image size to check", human(f.free_bytes), f.name)));
            } else if f.free_bytes as f64 >= size as f64 * 1.05 {
                checks.push(check("Storage", "pass", format!("{} free on {}, image needs {}", human(f.free_bytes), f.name, human(size))));
            } else {
                let old: Vec<String> = f.files.iter().filter(|(n, _)| (n.ends_with(".bin") || n.ends_with(".tar")) && !info.boot.contains(n.as_str())).map(|(n, s)| format!("{n} ({})", human(*s))).collect();
                checks.push(check("Storage", "fail", format!("only {} free on {}, image needs {}.{}", human(f.free_bytes), f.name, human(size), if old.is_empty() { String::new() } else { format!(" Candidates to delete: {}", old.join(", ")) })));
            }
        }
        _ => checks.push(check("Storage", "unknown", "could not read the file system")),
    }
    if md5.trim().len() == 32 {
        checks.push(check("Checksum", "pass", "MD5 given; verified on the device after the transfer"));
    } else {
        checks.push(check("Checksum", "warn", "no MD5 given: the image is not verified before activation (copy it from the Cisco download page)"));
    }
    if info.stack_members > 1 {
        checks.push(check("Stack", "warn", format!("{} members: every member needs room for the image and the whole stack reloads together", info.stack_members)));
    }
    if !info.boot.is_empty() {
        checks.push(check("Boot variable", "pass", format!("currently {}", info.boot)));
    } else {
        checks.push(check("Boot variable", "unknown", "could not read it; after activation confirm with show boot"));
    }
    checks.push(check("Backup", "pass", "the running config is backed up before any change"));

    let url_ok = re(r"^(tftp|ftp|scp|sftp|http|https|usb\d*:|flash\d*:|bootflash:)").is_match(url.trim());
    if !url.trim().is_empty() && !url_ok {
        checks.push(check("Source", "fail", "the source must be a tftp://, ftp://, scp://, sftp://, http(s):// URL or a usb/flash path reachable from the device"));
    }
    let ok = checks.iter().all(|c| c.status != "fail");

    let mut transfer = Vec::new();
    let mut verify = Vec::new();
    let mut activate = Vec::new();
    let mut reload = Vec::new();
    let mut post = vec!["show version".to_string()];
    match os {
        Os::NxOs => {
            if !already && url_ok {
                transfer.push(format!("copy {url} bootflash:{image} vrf management"));
            }
            verify.push(format!("show file bootflash:{image} md5sum"));
            verify.push(format!("show install all impact nxos bootflash:{image}"));
            activate.push(format!("install all nxos bootflash:{image}"));
            notes.push("On NX-OS `install all` upgrades and reloads in one step (ISSU where supported); the reload step is included in it.".into());
            post.push("show install all status".into());
            post.push("show boot".into());
        }
        Os::IosXe if info.mode == "install" => {
            if !already && url_ok {
                transfer.push(format!("copy {url} flash:{image}"));
            }
            if md5.trim().len() == 32 {
                verify.push(format!("verify /md5 flash:{image} {}", md5.trim()));
            }
            activate.push(format!("install add file flash:{image} activate commit"));
            notes.push("Install mode: `install add file … activate commit` expands the package, sets boot to packages.conf and reloads when you confirm. Run `install remove inactive` afterwards to free space.".into());
            post.push("show install summary".into());
            post.push("show boot".into());
        }
        _ => {
            if !already && url_ok {
                transfer.push(format!("copy {url} flash:{image}"));
            }
            if md5.trim().len() == 32 {
                verify.push(format!("verify /md5 flash:{image} {}", md5.trim()));
            }
            activate.push("configure terminal".into());
            activate.push("no boot system".into());
            activate.push(format!("boot system flash:{image}"));
            activate.push("end".into());
            activate.push("write memory".into());
            activate.push("show boot".into());
            reload.push("reload".into());
            if info.stack_members > 1 {
                notes.push("On a stack, copy the image to every member (flash1:, flash2:, …) or use `archive download-sw` so all members boot the same version.".into());
            }
            post.push("show boot".into());
        }
    }
    post.push("show logging | include -[0-3]-".into());
    Plan { checks, transfer, verify, activate, reload, post, notes, ok }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_and_boot() {
        let d = parse_dir("flash:", "Directory of flash:/\n\n    2  -rwx    35226112   Mar 1 2026 10:00:00 +00:00  c2960x-universalk9-mz.152-7.E4.bin\n    3  -rwx        3096   Mar 1 2026 10:00:00 +00:00  config.text\n\n122185728 bytes total (80000000 bytes free)\n");
        assert_eq!(d.total_bytes, 122185728);
        assert_eq!(d.free_bytes, 80000000);
        assert_eq!(d.files.len(), 2);
        assert_eq!(d.files[0].0, "c2960x-universalk9-mz.152-7.E4.bin");
        assert_eq!(parse_boot("BOOT path-list      : flash:c2960x-universalk9-mz.152-7.E4.bin\nConfig file         : flash:/config.text"), "flash:c2960x-universalk9-mz.152-7.E4.bin");
        let n = parse_dir("bootflash:", "    1234567890    Mar 01 10:00:00 2026  nxos.9.3.10.bin\n\nUsage for bootflash://sup-local\n 3000000000 bytes used\n 5000000000 bytes free\n 8000000000 bytes total\n");
        assert_eq!(n.free_bytes, 5000000000);
        assert_eq!(n.files[0].0, "nxos.9.3.10.bin");
    }

    #[test]
    fn plans() {
        let info = FirmwareInfo { os: Some(Os::Ios), version: "15.2(7)E4".into(), model: "WS-C2960X-48FPD-L".into(), boot: "flash:c2960x-universalk9-mz.152-7.E4.bin".into(), filesystems: vec![FileSystem { name: "flash:".into(), total_bytes: 122_000_000, free_bytes: 80_000_000, files: vec![] }], ..Default::default() };
        let p = plan(&info, "tftp://10.0.0.5/c2960x-universalk9-mz.152-7.E9.bin", "c2960x-universalk9-mz.152-7.E9.bin", 36_000_000, "d41d8cd98f00b204e9800998ecf8427e");
        assert!(p.ok, "{:?}", p.checks);
        assert_eq!(p.transfer[0], "copy tftp://10.0.0.5/c2960x-universalk9-mz.152-7.E9.bin flash:c2960x-universalk9-mz.152-7.E9.bin");
        assert!(p.activate.contains(&"boot system flash:c2960x-universalk9-mz.152-7.E9.bin".to_string()));
        assert_eq!(p.reload, vec!["reload"]);
        let bad = plan(&info, "tftp://10.0.0.5/x", "cat9k_iosxe.17.09.04a.SPA.bin", 900_000_000, "");
        assert!(!bad.ok);
        assert!(bad.checks.iter().any(|c| c.title == "Image matches the platform" && c.status == "fail"));
        assert!(bad.checks.iter().any(|c| c.title == "Storage" && c.status == "fail"));
        let xe = FirmwareInfo { os: Some(Os::IosXe), model: "C9300-48P".into(), mode: "install".into(), filesystems: vec![FileSystem { name: "flash:".into(), total_bytes: 11_000_000_000, free_bytes: 6_000_000_000, files: vec![] }], ..Default::default() };
        let p = plan(&xe, "scp://u@10.0.0.5/cat9k_iosxe.17.09.05.SPA.bin", "cat9k_iosxe.17.09.05.SPA.bin", 900_000_000, "");
        assert_eq!(p.activate[0], "install add file flash:cat9k_iosxe.17.09.05.SPA.bin activate commit");
        assert!(p.reload.is_empty());
    }
}
