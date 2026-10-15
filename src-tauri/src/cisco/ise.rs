//! Cisco ISE (ADE-OS) CLI evaluators. Everything the CLI cannot tell us
//! (deployment sync, certificates, AD, licensing) comes from `ise_api`.

use super::checks::{grep, re, Status, Verdict};

/// `show application status ise`: one row per process with a state column.
pub fn services(text: &str) -> Verdict {
    let row = re(r"(?m)^\s*([A-Za-z][A-Za-z0-9 ./()&+-]+?)\s{2,}(running|not running|disabled|initializing|starting|stopped|failed|unknown)\b(?:\s+(\d+))?", );
    let mut running = Vec::new();
    let mut down = Vec::new();
    let mut disabled = Vec::new();
    for c in row.captures_iter(text) {
        let name = c[1].trim().to_string();
        let state = c[2].to_lowercase();
        match state.as_str() {
            "running" => running.push(name),
            "disabled" => disabled.push(name),
            _ => down.push(format!("{name} ({state})")),
        }
    }
    if running.is_empty() && down.is_empty() {
        return Verdict::unknown("could not read process states from show application status ise").evidence(text.lines().take(8).map(|l| l.to_string()).collect());
    }
    // services that must run on every persona
    let core = ["Application Server", "Database Server", "Database Listener"];
    let core_down: Vec<&String> = down.iter().filter(|d| core.iter().any(|c| d.starts_with(c))).collect();
    let v = if !core_down.is_empty() {
        Verdict::new(Status::Critical, format!("{} core service(s) not running: {}", core_down.len(), core_down.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")))
    } else if !down.is_empty() {
        Verdict::new(Status::Warning, format!("{} service(s) not running ({} running, {} disabled by persona)", down.len(), running.len(), disabled.len()))
    } else {
        Verdict::new(Status::Healthy, format!("{} services running, {} disabled by persona", running.len(), disabled.len()))
    };
    let mut ev: Vec<String> = down.clone();
    ev.extend(grep(text, r"(?i)running|initializing|starting").into_iter().take(30));
    if down.is_empty() { v.evidence(ev) } else { v.evidence(ev).follow_up(&["show application status ise", "show logging application ise-psc.log tail", "application stop ise / application start ise (after approval)"]) }
}

/// `show backup history`: newest success wins; none in 7 days is a warning.
pub fn backups(text: &str) -> Verdict {
    let date = re(r"(?i)(?:\b|_)(\d{4}[-/]\d{2}[-/]\d{2}|[A-Z][a-z]{2}\s+[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\w+\s+\d{4})");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.iter().any(|l| re(r"(?i)no backup|not performed|no entries").is_match(l)) || lines.is_empty() {
        return Verdict::new(Status::Warning, "No backups recorded on this node").evidence(lines.iter().take(5).map(|l| l.to_string()).collect()).follow_up(&["show backup history", "show repository", "backup <name> repository <repo> ise-config encryption-key plain <key> (after approval)"]);
    }
    let failed = grep(text, r"(?i)\bfail");
    let ok = grep(text, r"(?i)success|completed");
    let last = ok.last().cloned().unwrap_or_default();
    let when = date.captures(&last).map(|c| c[1].to_string()).unwrap_or_default();
    let mut v = if ok.is_empty() {
        Verdict::new(Status::Critical, format!("No successful backup in history ({} failed)", failed.len()))
    } else if !failed.is_empty() && failed.iter().any(|f| lines.last().map(|l| l.trim() == f.trim()).unwrap_or(false)) {
        Verdict::new(Status::Warning, format!("Most recent backup failed; last success {when}"))
    } else {
        Verdict::new(Status::Healthy, if when.is_empty() { "Last backup succeeded".to_string() } else { format!("Last successful backup {when}") })
    };
    let mut ev = failed.clone();
    ev.push(last);
    v = v.evidence(ev);
    if ok.is_empty() || !failed.is_empty() { v.follow_up(&["show backup history", "show repository", "show logging system | include backup"]) } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ise_services_core_down_is_critical() {
        let t = "ISE PROCESS NAME                       STATE            PROCESS ID\n--------------------------------------------------------------------\nDatabase Listener                      running          12345\nDatabase Server                        not running\nApplication Server                     running          23456\nProfiler Database                      running          34567\nISE Indexing Engine                    disabled\nAD Connector                           running          45678\nM&T Session Database                   running          56789\nM&T Log Processor                      running          67890\nCertificate Authority Service          disabled\nEST Service                            disabled\nSXP Engine Service                     disabled\nTC-NAC Service                         disabled\npxGrid Infrastructure Service          running          78901\n";
        let v = services(t);
        assert_eq!(v.status, Status::Critical, "{}", v.summary);
        assert!(v.summary.contains("Database Server"));
        let v = services(&t.replace("Database Server                        not running", "Database Server                        running          11111"));
        assert_eq!(v.status, Status::Healthy, "{}", v.summary);
    }

    #[test]
    fn ise_backup_history() {
        let t = "Wed Oct 01 02:00:11 UTC 2026: backup nightly-CFG10-260901-0200.tar.gpg to repository ftp-backups: success\nThu Oct 02 02:00:09 UTC 2026: backup nightly-CFG10-260902-0200.tar.gpg to repository ftp-backups: success\n";
        let v = backups(t);
        assert_eq!(v.status, Status::Healthy, "{}", v.summary);
        assert!(v.summary.contains("Oct 02"), "{}", v.summary);
        let v = backups(&format!("{t}Fri Oct 03 02:00:09 UTC 2026: backup nightly to repository ftp-backups: failed (repository unreachable)\n"));
        assert_eq!(v.status, Status::Warning, "{}", v.summary);
        assert_eq!(backups("% No backup history found\n").status, Status::Warning);
    }
}
