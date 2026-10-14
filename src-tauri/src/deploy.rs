//! Reviewed deployment: back up, set a rollback point, send the reviewed
//! lines one by one with progress, verify, and roll back or finish on the
//! user's word. Also the firmware inspection and the template commands.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::cisco::configs;
use crate::cisco::detect::{self, Os};
use crate::cisco::firmware::{self, FirmwareInfo, Plan};
use crate::cisco::templates::{self, FieldError, Rendered, Template};
use crate::session::runner::{Answers, YesNo};
use crate::session::SessionManager;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

const YES: Option<Answers> = Some(Answers { yes_no: YesNo::Yes });
const NO: Option<Answers> = Some(Answers { yes_no: YesNo::No });

/*
 *  templates
 */

#[tauri::command]
pub fn templates_list() -> Vec<Template> {
    templates::catalog()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum RenderResult {
    Ok { rendered: Rendered },
    Invalid { errors: Vec<FieldError> },
}

#[tauri::command]
pub fn template_render(id: String, os: Option<Os>, values: BTreeMap<String, String>) -> RenderResult {
    match templates::render(&id, os.unwrap_or(Os::Ios), &values) {
        Ok(rendered) => RenderResult::Ok { rendered },
        Err(errors) => RenderResult::Invalid { errors },
    }
}

/*
 *  deployment
 */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployOptions {
    /// back up the running config first
    pub backup: bool,
    /// save a rollback point on the device (copy to flash / checkpoint)
    pub rollback_point: bool,
    /// `reload in N` before, cancelled by `deploy_finish`; 0 = off
    pub safety_reload_minutes: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DeployEvent {
    /// phase: backup | rollback | safety | send | verify
    Step { phase: String, command: String, index: usize, total: usize },
    Output { phase: String, command: String, output: String, ok: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployStep {
    pub phase: String,
    pub command: String,
    pub output: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployReport {
    pub started: i64,
    pub finished: i64,
    pub backup: Option<String>,
    /// how to undo, when the platform offers it
    pub rollback_command: Option<String>,
    pub safety_reload: bool,
    pub steps: Vec<DeployStep>,
    pub sent: usize,
    pub failed: bool,
    pub error: String,
}

fn looks_like_error(output: &str) -> bool {
    output.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("% ") || t.starts_with("%Error") || t.starts_with("ERROR:") || t.starts_with("Error:") || t.contains("Invalid command")
    })
}

#[tauri::command]
pub async fn deploy_run(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    id: String,
    os: Option<Os>,
    site: String,
    device: String,
    commands: Vec<String>,
    verify: Vec<String>,
    options: DeployOptions,
    events: Channel<DeployEvent>,
) -> CmdResult<DeployReport> {
    let os = os.unwrap_or(Os::Ios);
    let started = chrono::Utc::now().timestamp();
    let mut report = DeployReport { started, finished: started, backup: None, rollback_command: None, safety_reload: false, steps: Vec::new(), sent: 0, failed: false, error: String::new() };
    let total = commands.len();

    let step = |phase: &str, command: &str, output: Result<String, String>, report: &mut DeployReport| -> bool {
        let (text, ok) = match output {
            Ok(o) => {
                let bad = looks_like_error(&o);
                (o, !bad)
            }
            Err(e) => (e, false),
        };
        let _ = events.send(DeployEvent::Output { phase: phase.into(), command: command.into(), output: text.clone(), ok });
        report.steps.push(DeployStep { phase: phase.into(), command: command.into(), output: text, ok });
        ok
    };

    if let Some(cmd) = os.no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }

    if options.backup {
        let _ = events.send(DeployEvent::Step { phase: "backup".into(), command: "show running-config".into(), index: 0, total });
        let text = sessions.run_command(&id, "show running-config", Duration::from_secs(300)).await.map_err(err)?;
        let root = crate::tools::config_root_pub(&app);
        let b = configs::save(&root, &site, &device, "running", &text).map_err(err)?;
        report.backup = Some(b.path.clone());
        step("backup", "show running-config", Ok(format!("saved to {}", b.path)), &mut report);
    }

    if options.rollback_point {
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let (cmd, undo): (String, String) = match os {
            Os::NxOs => (format!("checkpoint omni-{stamp}"), format!("rollback running-config checkpoint omni-{stamp}")),
            Os::Asa | Os::SmallBusiness | Os::CatOs | Os::Unknown => (String::new(), String::new()),
            _ => (format!("copy running-config flash:omni-{stamp}.cfg"), format!("configure replace flash:omni-{stamp}.cfg force")),
        };
        if !cmd.is_empty() {
            let _ = events.send(DeployEvent::Step { phase: "rollback".into(), command: cmd.clone(), index: 0, total });
            let out = sessions.run_command_with(&id, &cmd, Duration::from_secs(120), YES).await.map_err(err);
            if step("rollback", &cmd, out, &mut report) {
                report.rollback_command = Some(undo);
            }
        }
    }

    if options.safety_reload_minutes > 0 && matches!(os, Os::Ios | Os::IosXe) {
        let cmd = format!("reload in {}", options.safety_reload_minutes.clamp(1, 240));
        let _ = events.send(DeployEvent::Step { phase: "safety".into(), command: cmd.clone(), index: 0, total });
        // "Save? [yes/no]" must be answered no: nothing has changed yet
        let out = sessions.run_command_with(&id, &cmd, Duration::from_secs(30), NO).await.map_err(err);
        report.safety_reload = step("safety", &cmd, out, &mut report);
    }

    for (i, command) in commands.iter().enumerate() {
        let _ = events.send(DeployEvent::Step { phase: "send".into(), command: command.clone(), index: i, total });
        let out = sessions.run_command_with(&id, command, Duration::from_secs(120), YES).await.map_err(err);
        let ok = step("send", command, out.clone(), &mut report);
        if !ok {
            report.failed = true;
            report.error = format!("`{command}` was rejected: {}", out.unwrap_or_else(|e| e).lines().find(|l| l.trim_start().starts_with('%') || l.contains("rror")).unwrap_or("see the output").trim());
            break;
        }
        report.sent += 1;
    }

    if !report.failed {
        for command in &verify {
            let _ = events.send(DeployEvent::Step { phase: "verify".into(), command: command.clone(), index: 0, total: verify.len() });
            let out = sessions.run_command(&id, command, Duration::from_secs(120)).await.map_err(err);
            step("verify", command, out, &mut report);
        }
    }
    report.finished = chrono::Utc::now().timestamp();
    Ok(report)
}

/// After a deployment: cancel the safety reload and/or save.
#[tauri::command]
pub async fn deploy_finish(sessions: State<'_, SessionManager>, id: String, os: Option<Os>, cancel_reload: bool, save: bool) -> CmdResult<Vec<DeployStep>> {
    let os = os.unwrap_or(Os::Ios);
    let mut steps = Vec::new();
    if cancel_reload {
        let out = sessions.run_command_with(&id, "reload cancel", Duration::from_secs(30), YES).await;
        steps.push(DeployStep { phase: "finish".into(), command: "reload cancel".into(), output: out.as_ref().map(|s| s.clone()).unwrap_or_else(|e| format!("{e:#}")), ok: out.is_ok() });
    }
    if save {
        let cmd = match os { Os::NxOs => "copy running-config startup-config", Os::Asa => "write memory", _ => "write memory" };
        let out = sessions.run_command_with(&id, cmd, Duration::from_secs(120), YES).await;
        steps.push(DeployStep { phase: "finish".into(), command: cmd.into(), output: out.as_ref().map(|s| s.clone()).unwrap_or_else(|e| format!("{e:#}")), ok: out.is_ok() });
    }
    Ok(steps)
}

/// Run the platform's rollback command from a deployment report.
#[tauri::command]
pub async fn deploy_rollback(sessions: State<'_, SessionManager>, id: String, command: String) -> CmdResult<DeployStep> {
    if !(command.starts_with("configure replace ") || command.starts_with("rollback running-config checkpoint ")) {
        return Err("not a rollback command".into());
    }
    let out = sessions.run_command_with(&id, &command, Duration::from_secs(300), YES).await;
    Ok(DeployStep { phase: "rollback".into(), command, output: out.as_ref().map(|s| s.clone()).unwrap_or_else(|e| format!("{e:#}")), ok: out.as_ref().is_ok_and(|o| !looks_like_error(o)) })
}

/*
 *  firmware
 */

#[tauri::command]
pub async fn firmware_inspect(sessions: State<'_, SessionManager>, id: String) -> CmdResult<FirmwareInfo> {
    let version = sessions.run_command(&id, "show version", Duration::from_secs(90)).await.map_err(err)?;
    let facts = detect::parse_facts(&version);
    let os = facts.os.unwrap_or(Os::Ios);
    if let Some(cmd) = os.no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }
    let mut info = FirmwareInfo { os: facts.os, version: facts.version, model: facts.model, image: facts.image, ..Default::default() };
    let fs_names: &[&str] = match os { Os::NxOs => &["bootflash:"], _ => &["flash:", "flash1:", "flash2:", "flash3:", "flash4:", "flash5:", "flash6:", "flash7:", "flash8:"] };
    for name in fs_names {
        match sessions.run_command(&id, &format!("dir {name}"), Duration::from_secs(60)).await {
            Ok(text) if !text.contains("% Invalid") && !text.contains("No such") && !text.contains("Error") => {
                let fs = firmware::parse_dir(name, &text);
                if fs.total_bytes > 0 || !fs.files.is_empty() {
                    info.filesystems.push(fs);
                } else if *name != "flash:" && *name != "bootflash:" {
                    break;
                }
            }
            _ => {
                if *name != "flash:" && *name != "bootflash:" {
                    break;
                }
            }
        }
    }
    info.stack_members = info.filesystems.iter().filter(|f| f.name.starts_with("flash")).count().max(1);
    if let Ok(t) = sessions.run_command(&id, "show boot", Duration::from_secs(30)).await {
        info.boot = firmware::parse_boot(&t);
    }
    if info.boot.is_empty() {
        if let Ok(t) = sessions.run_command(&id, "show bootvar", Duration::from_secs(30)).await {
            info.boot = firmware::parse_boot(&t);
        }
    }
    if os == Os::IosXe {
        info.mode = if version.contains("INSTALL") { "install".into() } else if version.contains("BUNDLE") { "bundle".into() } else if info.boot.contains("packages.conf") { "install".into() } else { "bundle".into() };
    }
    Ok(info)
}

#[tauri::command]
pub fn firmware_plan(info: FirmwareInfo, url: String, image: String, size: u64, md5: String) -> Plan {
    firmware::plan(&info, &url, &image, size, &md5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_lines() {
        assert!(looks_like_error("% Invalid input detected at '^' marker."));
        assert!(looks_like_error("ERROR: Ambiguous command"));
        assert!(!looks_like_error("Building configuration...\n[OK]"));
        assert!(!looks_like_error("100% complete"));
    }
}
