//! Things that leave the app: the system RDP/VNC client for a saved device,
//! and a second OmniConsole window.

use std::process::Command;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

type CmdResult<T> = Result<T, String>;

fn check_host(host: &str) -> CmdResult<&str> {
    let h = host.trim();
    if h.is_empty() || !h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']')) || h.starts_with('-') {
        return Err("not a host name or IP address".into());
    }
    Ok(h)
}

/// Open the platform's RDP or VNC client for host:port.
#[tauri::command]
pub fn launch_external(protocol: String, host: String, port: u16, username: String) -> CmdResult<String> {
    let host = check_host(&host)?;
    let user = username.trim().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | '\\')).collect::<String>();
    let (program, args): (&str, Vec<String>) = match (protocol.as_str(), std::env::consts::OS) {
        ("rdp", "windows") => {
            let mut a = vec![format!("/v:{host}:{}", if port == 0 { 3389 } else { port })];
            let _ = &user; // mstsc takes the user from the saved credentials
            a.push("/prompt".into());
            ("mstsc", a)
        }
        ("rdp", "macos") => ("open", vec![format!("rdp://full%20address=s:{host}:{}{}", if port == 0 { 3389 } else { port }, if user.is_empty() { String::new() } else { format!("&username=s:{user}") })]),
        ("rdp", _) => {
            let p = if port == 0 { 3389 } else { port };
            if which("xfreerdp") { ("xfreerdp", { let mut a = vec![format!("/v:{host}:{p}"), "/cert:tofu".into(), "/dynamic-resolution".into()]; if !user.is_empty() { a.push(format!("/u:{user}")); } a }) }
            else if which("xfreerdp3") { ("xfreerdp3", { let mut a = vec![format!("/v:{host}:{p}"), "/cert:tofu".into(), "/dynamic-resolution".into()]; if !user.is_empty() { a.push(format!("/u:{user}")); } a }) }
            else if which("remmina") { ("remmina", vec!["-c".into(), format!("rdp://{}{host}:{p}", if user.is_empty() { String::new() } else { format!("{user}@") })]) }
            else { return Err("no RDP client found: install freerdp (xfreerdp) or remmina".into()); }
        }
        ("vnc", "windows") => {
            let p = if port == 0 { 5900 } else { port };
            if which("vncviewer") { ("vncviewer", vec![format!("{host}:{p}")]) }
            else if which("tvnviewer") { ("tvnviewer", vec![format!("{host}::{p}")]) }
            else { return Err("no VNC client found: install TigerVNC (vncviewer) or RealVNC".into()); }
        }
        ("vnc", "macos") => ("open", vec![format!("vnc://{}{host}:{}", if user.is_empty() { String::new() } else { format!("{user}@") }, if port == 0 { 5900 } else { port })]),
        ("vnc", _) => {
            let p = if port == 0 { 5900 } else { port };
            if which("vncviewer") { ("vncviewer", vec![format!("{host}:{p}")]) }
            else if which("remmina") { ("remmina", vec!["-c".into(), format!("vnc://{host}:{p}")]) }
            else if which("vinagre") { ("vinagre", vec![format!("{host}:{p}")]) }
            else { return Err("no VNC client found: install TigerVNC (vncviewer) or remmina".into()); }
        }
        _ => return Err(format!("unknown protocol {protocol}")),
    };
    Command::new(program).args(&args).spawn().map_err(|e| format!("starting {program}: {e}"))?;
    Ok(format!("{program} {}", args.join(" ")))
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(bin).exists() || d.join(format!("{bin}.exe")).exists())).unwrap_or(false)
}

/// A second OmniConsole window. `query` is appended to the app URL, e.g.
/// `connect=<device id>` to open a device there straight away.
#[tauri::command]
pub fn open_window(app: AppHandle, query: String) -> CmdResult<String> {
    let n = app.webview_windows().len();
    let label = format!("win{}-{}", n, chrono::Utc::now().timestamp_millis() % 100000);
    let url = if query.is_empty() { "index.html".to_string() } else { format!("index.html?{}", query.replace(['"', '<', '>', ' '], "")) };
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title("OmniConsole")
        .inner_size(1100.0, 700.0)
        .min_inner_size(760.0, 440.0)
        .build()
        .map_err(|e| format!("{e}"))?;
    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::check_host;

    #[test]
    fn hosts_are_checked_before_spawning() {
        assert!(check_host("10.0.0.9").is_ok());
        assert!(check_host("srv-1.example.net").is_ok());
        assert!(check_host("[fe80::1]").is_ok());
        assert!(check_host("-v").is_err());
        assert!(check_host("host; rm -rf /").is_err());
        assert!(check_host("").is_err());
    }
}
