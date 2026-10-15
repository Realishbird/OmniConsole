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
use crate::platforms;
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
    /// how to undo, when the platform offers it (several lines = run in order)
    pub rollback_command: Option<String>,
    /// a `reload in N` (Cisco) or an unconfirmed commit is pending on the device
    pub safety_reload: bool,
    /// what is pending and how `deploy_finish` settles it, for the UI
    #[serde(default)]
    pub pending: String,
    /// the platform's save step, empty when a commit is already persistent
    #[serde(default)]
    pub save_command: String,
    pub steps: Vec<DeployStep>,
    pub sent: usize,
    pub failed: bool,
    pub error: String,
}

fn looks_like_error(output: &str) -> bool {
    output.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("% ") || t.starts_with("%Error") || t.starts_with("ERROR:") || t.starts_with("Error:") || t.contains("Invalid command")
            || t.starts_with("Invalid input") || t.starts_with("syntax error") || t.starts_with("error:") || t.starts_with("unknown command") || t.contains("Error: ")
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
    let profile = platforms::profile(os);
    let wf = &profile.workflow;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let name = format!("omni-{stamp}");
    let minutes = options.safety_reload_minutes.clamp(1, 240);
    let started = chrono::Utc::now().timestamp();
    let mut report = DeployReport {
        started, finished: started, backup: None, rollback_command: None, safety_reload: false, pending: String::new(),
        save_command: wf.save.join("\n"), steps: Vec::new(), sent: 0, failed: false, error: String::new(),
    };

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

    // the lines to send, wrapped in the platform's configuration mode unless they already are
    let already_wrapped = commands.first().map(|c| wf.enter.iter().any(|e| platforms::fill(e, &name, minutes).split_whitespace().next() == c.split_whitespace().next()) || c.trim_start().starts_with("configure") || c.trim_start().starts_with("nv set") || c.trim_start().starts_with("nv unset")).unwrap_or(false);
    let mut lines: Vec<String> = Vec::new();
    if !already_wrapped {
        lines.extend(wf.enter.iter().map(|e| platforms::fill(e, &name, minutes)));
    }
    lines.extend(commands.iter().cloned());
    // candidate platforms: the commit makes it live; a timed commit reverts unless confirmed.
    // Junos and IOS-XR commit from inside configuration mode, so the commit
    // goes before the exit lines; leaving first would prompt about uncommitted
    // changes and then type `commit` at the operational prompt.
    let mut commit: Vec<String> = Vec::new();
    if wf.candidate {
        if options.safety_reload_minutes > 0 && !wf.commit_confirmed.is_empty() {
            commit.push(platforms::fill(&wf.commit_confirmed, &name, minutes));
        } else {
            commit.extend(wf.commit.iter().map(|c| platforms::fill(c, &name, minutes)));
        }
    }
    let exit_lines: Vec<String> = if already_wrapped { Vec::new() } else { wf.exit.clone() };
    let total = lines.len() + commit.len() + exit_lines.len();

    if options.backup {
        let kind = profile.configs.first().cloned().unwrap_or(platforms::ConfigKind { kind: "running".into(), label: "Running configuration".into(), command: wf.restorable.clone() });
        let _ = events.send(DeployEvent::Step { phase: "backup".into(), command: kind.command.clone(), index: 0, total });
        let text = sessions.run_command(&id, &kind.command, Duration::from_secs(300)).await.map_err(err)?;
        let root = crate::tools::config_root_pub(&app);
        let b = configs::save(&root, &site, &device, &kind.kind, &text).map_err(err)?;
        report.backup = Some(b.path.clone());
        step("backup", &kind.command, Ok(format!("saved to {}", b.path)), &mut report);
    }

    if options.rollback_point {
        if !wf.checkpoint.is_empty() {
            let cmd = platforms::fill(&wf.checkpoint, &name, minutes);
            let _ = events.send(DeployEvent::Step { phase: "rollback".into(), command: cmd.clone(), index: 0, total });
            let out = sessions.run_command_with(&id, &cmd, Duration::from_secs(120), YES).await.map_err(err);
            if step("rollback", &cmd, out, &mut report) {
                report.rollback_command = Some(platforms::fill(&wf.rollback, &name, minutes));
            }
        } else if !wf.rollback.is_empty() {
            // Junos / IOS-XR / NVUE keep history on their own: undo = roll back, then commit
            let mut undo = vec![platforms::fill(&wf.rollback, &name, minutes)];
            if wf.candidate && !wf.commit.is_empty() && !wf.rollback.starts_with("nv ") {
                undo.extend(wf.commit.iter().map(|c| platforms::fill(c, &name, minutes)));
            }
            report.rollback_command = Some(undo.join("\n"));
            step("rollback", &undo.join(" ; "), Ok(format!("{} keeps the previous configuration; this undoes the change", profile.label)), &mut report);
        }
    }

    if options.safety_reload_minutes > 0 && matches!(os, Os::Ios | Os::IosXe) {
        let cmd = format!("reload in {minutes}");
        let _ = events.send(DeployEvent::Step { phase: "safety".into(), command: cmd.clone(), index: 0, total });
        // "Save? [yes/no]" must be answered no: nothing has changed yet
        let out = sessions.run_command_with(&id, &cmd, Duration::from_secs(30), NO).await.map_err(err);
        report.safety_reload = step("safety", &cmd, out, &mut report);
        if report.safety_reload {
            report.pending = format!("reload in {minutes} minutes unless cancelled");
        }
    }

    for (i, command) in lines.iter().enumerate() {
        let _ = events.send(DeployEvent::Step { phase: "send".into(), command: command.clone(), index: i, total });
        let out = sessions.run_command_with(&id, command, Duration::from_secs(120), YES).await.map_err(err);
        let ok = step("send", command, out.clone(), &mut report);
        if !ok {
            report.failed = true;
            report.error = crate::redact::text(&format!("`{command}` was rejected: {}", out.unwrap_or_else(|e| e).lines().find(|l| l.trim_start().starts_with('%') || l.contains("rror") || l.contains("syntax") || l.contains("unknown")).unwrap_or("see the output").trim()));
            break;
        }
        report.sent += 1;
    }
    if report.failed {
        // leave configuration mode so the finish step (reload cancel, save) runs at the exec prompt
        for command in &exit_lines {
            let out = sessions.run_command_with(&id, command, Duration::from_secs(60), NO).await.map_err(err);
            step("send", command, out, &mut report);
        }
    }

    if !report.failed {
        for (i, command) in commit.iter().enumerate() {
            let _ = events.send(DeployEvent::Step { phase: "commit".into(), command: command.clone(), index: lines.len() + i, total });
            let out = sessions.run_command_with(&id, command, Duration::from_secs(300), YES).await.map_err(err);
            if !step("commit", command, out.clone(), &mut report) {
                report.failed = true;
                report.error = crate::redact::text(&format!("`{command}` failed: {}", out.unwrap_or_else(|e| e).lines().find(|l| l.contains("rror") || l.contains("fail")).unwrap_or("see the output").trim()));
                break;
            }
            if command.contains("confirmed") || command.contains("timer") {
                report.safety_reload = true;
                report.pending = format!("the commit reverts on its own in {minutes} minutes unless confirmed");
            }
        }
        for (i, command) in exit_lines.iter().enumerate() {
            let _ = events.send(DeployEvent::Step { phase: "send".into(), command: command.clone(), index: lines.len() + commit.len() + i, total });
            // a failed commit must not be discarded by "exit with uncommitted changes? yes"
            let out = sessions.run_command_with(&id, command, Duration::from_secs(60), if report.failed { NO } else { YES }).await.map_err(err);
            step("send", command, out, &mut report);
        }
    } else {
        for command in &exit_lines {
            let out = sessions.run_command_with(&id, command, Duration::from_secs(60), NO).await.map_err(err);
            step("send", command, out, &mut report);
        }
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

/// After a deployment: cancel the safety reload or confirm the timed commit, and/or save.
#[tauri::command]
pub async fn deploy_finish(sessions: State<'_, SessionManager>, id: String, os: Option<Os>, cancel_reload: bool, save: bool) -> CmdResult<Vec<DeployStep>> {
    let os = os.unwrap_or(Os::Ios);
    let wf = platforms::profile(os).workflow;
    let mut steps = Vec::new();
    if cancel_reload {
        if matches!(os, Os::Ios | Os::IosXe) {
            steps.push(finish_step(&sessions, &id, "reload cancel").await);
        } else if wf.candidate && save {
            // confirming the timed commit keeps the change
            for c in &wf.confirm {
                steps.push(finish_step(&sessions, &id, c).await);
            }
        }
        // a candidate platform's timed commit that is not confirmed reverts by itself
    }
    if save {
        for c in &wf.save {
            steps.push(finish_step(&sessions, &id, c).await);
        }
    }
    Ok(steps)
}

async fn finish_step(sessions: &SessionManager, id: &str, cmd: &str) -> DeployStep {
    let out = sessions.run_command_with(id, cmd, Duration::from_secs(120), YES).await;
    DeployStep { phase: "finish".into(), command: cmd.into(), output: out.as_ref().map(|s| s.clone()).unwrap_or_else(|e| format!("{e:#}")), ok: out.as_ref().is_ok_and(|o| !looks_like_error(o)) }
}

/// Configuration lifecycle actions straight from the platform profile:
/// `save` (persist the running config), `checkpoint` (named rollback point),
/// `rollback` (to a checkpoint or the platform's last-commit rollback) and
/// `history` (list what is on the device). Only the profile's own commands
/// run, so the UI cannot smuggle anything else in.
#[tauri::command]
pub async fn lifecycle_run(sessions: State<'_, SessionManager>, id: String, os: Option<Os>, action: String, name: String) -> CmdResult<Vec<DeployStep>> {
    let os = os.unwrap_or(Os::Ios);
    let wf = platforms::profile(os).workflow;
    let name = name.trim().to_string();
    let safe = name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let commands: Vec<String> = match action.as_str() {
        "save" => wf.save.clone(),
        "checkpoint" => {
            if wf.checkpoint.is_empty() {
                return Err(format!("{} has no checkpoint command; its restorable backup is the saved config text", os.label()));
            }
            if name.is_empty() || !safe {
                return Err("checkpoint names use letters, digits, - _ and . only".into());
            }
            vec![platforms::fill(&wf.checkpoint, &name, 0)]
        }
        "rollback" => {
            if wf.rollback.is_empty() {
                return Err(format!("{} has no rollback command; restore from a saved backup instead", os.label()));
            }
            if wf.rollback.contains("{name}") && (name.is_empty() || !safe) {
                return Err("pick the checkpoint to roll back to".into());
            }
            let mut v: Vec<String> = platforms::fill(&wf.rollback, &name, 0).lines().map(|l| l.to_string()).collect();
            // candidate platforms need the rollback committed to take effect
            if wf.candidate {
                v.extend(wf.commit.iter().map(|c| platforms::fill(c, &name, 0)));
            }
            v
        }
        "history" => {
            if wf.history.is_empty() {
                return Err(format!("{} keeps no checkpoint list", os.label()));
            }
            vec![wf.history.clone()]
        }
        _ => return Err(format!("unknown lifecycle action {action}")),
    };
    if commands.is_empty() {
        return Err(format!("{} needs no explicit save: commits are persistent", os.label()));
    }
    if let Some(cmd) = os.no_paging() {
        let _ = sessions.run_command(&id, cmd, Duration::from_secs(15)).await;
    }
    let mut steps = Vec::new();
    for c in commands {
        let step = finish_step(&sessions, &id, &c).await;
        let ok = step.ok;
        steps.push(step);
        if !ok {
            break;
        }
    }
    Ok(steps)
}

/// Run the platform's rollback command(s) from a deployment report.
#[tauri::command]
pub async fn deploy_rollback(sessions: State<'_, SessionManager>, id: String, os: Option<Os>, command: String) -> CmdResult<DeployStep> {
    let wf = platforms::profile(os.unwrap_or(Os::Ios)).workflow;
    let allowed = |c: &str| {
        let head = |t: &str| t.split('{').next().unwrap_or("").trim().to_string();
        (!wf.rollback.is_empty() && c.starts_with(head(&wf.rollback).as_str())) || wf.commit.iter().any(|k| c == platforms::fill(k, "", 0).trim() || c.starts_with(head(k).as_str()))
    };
    let mut output = String::new();
    let mut ok = true;
    for c in command.lines().map(str::trim).filter(|c| !c.is_empty()) {
        if !allowed(c) {
            return Err(format!("`{c}` is not this platform's rollback command"));
        }
        let out = sessions.run_command_with(&id, c, Duration::from_secs(300), YES).await;
        let text = out.as_ref().map(|s| s.clone()).unwrap_or_else(|e| format!("{e:#}"));
        ok &= out.as_ref().is_ok_and(|o| !looks_like_error(o));
        output.push_str(&format!("{c}\n{text}\n"));
        if !ok {
            break;
        }
    }
    Ok(DeployStep { phase: "rollback".into(), command, output, ok })
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
