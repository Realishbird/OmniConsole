//! Tools that run on this computer, not on the device: ping, traceroute,
//! DNS lookup, TCP port check and an MTU probe. Long ones stream their
//! lines to the UI and can be cancelled (the child process is killed).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/// Running jobs by id; sending `true` on the watch cancels one.
#[derive(Default)]
pub struct NetJobs {
    jobs: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl NetJobs {
    fn start(&self, id: &str) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        self.jobs.lock().unwrap().insert(id.into(), tx);
        rx
    }
    fn finish(&self, id: &str) {
        self.jobs.lock().unwrap().remove(id);
    }
}

#[tauri::command]
pub fn net_cancel(jobs: State<'_, NetJobs>, id: String) {
    if let Some(tx) = jobs.jobs.lock().unwrap().get(&id) {
        let _ = tx.send(true);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Line {
    Line { text: String },
    Done { ok: bool, summary: String },
}

/// Only hostnames and IP addresses go into a command line.
fn check_target(target: &str) -> CmdResult<String> {
    let t = target.trim();
    if t.is_empty() || t.len() > 253 {
        return Err("enter a host name or IP address".into());
    }
    if t.parse::<IpAddr>().is_ok() {
        return Ok(t.to_string());
    }
    if t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':')) && !t.starts_with('-') {
        Ok(t.to_string())
    } else {
        Err(format!("{t:?} is not a host name or IP address"))
    }
}

/// Run a child, stream its lines, kill it on cancel. Returns (exit ok, line count).
async fn stream(mut cmd: Command, mut cancel: watch::Receiver<bool>, events: &Channel<Line>) -> anyhow::Result<(bool, Vec<String>)> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let _ = tx.send(l);
        }
    });
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let _ = tx2.send(l);
        }
    });
    let mut all = Vec::new();
    let mut status = None;
    loop {
        tokio::select! {
            l = rx.recv() => match l {
                Some(l) => {
                    let _ = events.send(Line::Line { text: l.clone() });
                    all.push(l);
                }
                None => break,
            },
            s = child.wait(), if status.is_none() => { status = Some(s?); }
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    let _ = child.kill().await;
                    let _ = events.send(Line::Line { text: "(cancelled)".into() });
                    return Ok((false, all));
                }
            }
        }
    }
    let status = match status {
        Some(s) => s,
        None => child.wait().await?,
    };
    Ok((status.success(), all))
}

fn ping_summary(lines: &[String]) -> String {
    let text = lines.join("\n");
    let loss = regex::Regex::new(r"(\d+(?:\.\d+)?)% (?:packet )?loss").unwrap().captures(&text).map(|c| c[1].to_string());
    let rtt = regex::Regex::new(r"(?:rtt|round-trip)[^=]*= ([\d.]+)/([\d.]+)/([\d.]+)").unwrap().captures(&text).map(|c| format!("min/avg/max {}/{}/{} ms", &c[1], &c[2], &c[3]));
    let win = regex::Regex::new(r"Minimum = (\d+)ms, Maximum = (\d+)ms, Average = (\d+)ms").unwrap().captures(&text).map(|c| format!("min/avg/max {}/{}/{} ms", &c[1], &c[3], &c[2]));
    match (loss, rtt.or(win)) {
        (Some(l), Some(r)) => format!("{l}% loss, {r}"),
        (Some(l), None) => format!("{l}% loss"),
        _ => if lines.iter().any(|l| l.contains("could not find host") || l.contains("unknown host") || l.contains("Name or service not known")) { "name not found".into() } else { "no reply".into() },
    }
}

/// ping: `count` echoes, from this computer.
#[tauri::command]
pub async fn net_ping(jobs: State<'_, NetJobs>, id: String, target: String, count: u32, size: Option<u32>, events: Channel<Line>) -> CmdResult<String> {
    let target = check_target(&target)?;
    let count = count.clamp(1, 100).to_string();
    let cancel = jobs.start(&id);
    let mut cmd = Command::new("ping");
    #[cfg(windows)]
    {
        cmd.args(["-n", &count]);
        if let Some(s) = size { cmd.args(["-l", &s.to_string()]); }
    }
    #[cfg(not(windows))]
    {
        cmd.args(["-c", &count]);
        if let Some(s) = size { cmd.args(["-s", &s.to_string()]); }
    }
    cmd.arg(&target);
    let r = stream(cmd, cancel, &events).await;
    jobs.finish(&id);
    let (ok, lines) = r.map_err(err)?;
    let summary = ping_summary(&lines);
    let _ = events.send(Line::Done { ok, summary: summary.clone() });
    Ok(summary)
}

#[tauri::command]
pub async fn net_traceroute(jobs: State<'_, NetJobs>, id: String, target: String, max_hops: Option<u32>, events: Channel<Line>) -> CmdResult<String> {
    let target = check_target(&target)?;
    let hops = max_hops.unwrap_or(30).clamp(1, 64).to_string();
    let cancel = jobs.start(&id);
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("tracert");
        c.args(["-d", "-h", &hops, "-w", "2000"]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        // traceroute may be missing on Linux; tracepath is in iputils
        let has = |b: &str| std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(b).exists())).unwrap_or(false);
        if has("traceroute") {
            let mut c = Command::new("traceroute");
            c.args(["-n", "-m", &hops, "-w", "2", "-q", "3"]);
            c
        } else if has("tracepath") {
            let mut c = Command::new("tracepath");
            c.args(["-n", "-m", &hops]);
            c
        } else {
            jobs.finish(&id);
            return Err("neither traceroute nor tracepath is installed on this computer".into());
        }
    };
    cmd.arg(&target);
    let r = stream(cmd, cancel, &events).await;
    jobs.finish(&id);
    let (ok, lines) = r.map_err(err)?;
    let hops_seen = lines.iter().filter(|l| l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit())).count();
    let summary = format!("{hops_seen} hop{}", if hops_seen == 1 { "" } else { "s" });
    let _ = events.send(Line::Done { ok, summary: summary.clone() });
    Ok(summary)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsResult {
    pub query: String,
    pub addresses: Vec<String>,
    pub names: Vec<String>,
    pub elapsed_ms: u128,
    pub detail: String,
}

/// Forward lookup natively; reverse and other record types through nslookup.
#[tauri::command]
pub async fn net_dns(target: String, record: Option<String>, server: Option<String>) -> CmdResult<DnsResult> {
    let target = check_target(&target)?;
    let start = Instant::now();
    let mut out = DnsResult { query: target.clone(), addresses: Vec::new(), names: Vec::new(), elapsed_ms: 0, detail: String::new() };
    let record = record.unwrap_or_else(|| "A".into()).to_uppercase();
    let server = match server.filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(check_target(&s)?),
        None => None,
    };
    if record == "A" && server.is_none() {
        if target.parse::<IpAddr>().is_ok() {
            // reverse: no std API, use nslookup
        } else {
            match tokio::net::lookup_host((target.as_str(), 0)).await {
                Ok(addrs) => {
                    for a in addrs {
                        let s = a.ip().to_string();
                        if !out.addresses.contains(&s) {
                            out.addresses.push(s);
                        }
                    }
                }
                Err(e) => out.detail = format!("{e}"),
            }
            out.elapsed_ms = start.elapsed().as_millis();
            return Ok(out);
        }
    }
    let allowed = ["A", "AAAA", "PTR", "MX", "TXT", "NS", "CNAME", "SOA", "SRV", "ANY"];
    if !allowed.contains(&record.as_str()) {
        return Err(format!("unsupported record type {record}"));
    }
    let mut cmd = Command::new("nslookup");
    cmd.arg(format!("-type={record}")).arg(&target);
    if let Some(s) = &server {
        cmd.arg(s);
    }
    cmd.stdin(Stdio::null());
    let output = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.map_err(|_| "nslookup timed out".to_string())?.map_err(err)?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    for c in regex::Regex::new(r"(?mi)^(?:Address(?:es)?:\s*|\s+)?((?:\d{1,3}\.){3}\d{1,3}|[0-9a-f:]{3,39})\s*$").unwrap().captures_iter(&text).skip(1) {
        let a = c[1].to_string();
        if a.parse::<IpAddr>().is_ok() && !out.addresses.contains(&a) {
            out.addresses.push(a);
        }
    }
    for c in regex::Regex::new(r"(?mi)(?:name = |canonical name = |mail exchanger = (?:\d+ )?|nameserver = )(\S+?)\.?\s*$").unwrap().captures_iter(&text) {
        let n = c[1].to_string();
        if !out.names.contains(&n) {
            out.names.push(n);
        }
    }
    out.detail = text;
    out.elapsed_ms = start.elapsed().as_millis();
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortResult {
    pub port: u16,
    pub open: bool,
    pub elapsed_ms: u128,
    pub detail: String,
}

/// TCP connect to each port, 3 s timeout each, all in parallel.
#[tauri::command]
pub async fn net_ports(target: String, ports: Vec<u16>, timeout_ms: Option<u64>) -> CmdResult<Vec<PortResult>> {
    let target = check_target(&target)?;
    if ports.is_empty() || ports.len() > 1024 {
        return Err("give between 1 and 1024 ports".into());
    }
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(3000).clamp(200, 20_000));
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((target.as_str(), 0)).await.map_err(|e| format!("{target}: {e}"))?.collect();
    let ip = addrs.first().map(|a| a.ip()).ok_or_else(|| format!("{target}: no address"))?;
    let sem = Arc::new(tokio::sync::Semaphore::new(64));
    let mut tasks = Vec::new();
    for port in ports {
        let sem = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _p = sem.acquire().await;
            let start = Instant::now();
            let r = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(SocketAddr::new(ip, port))).await;
            let (open, detail) = match r {
                Ok(Ok(_)) => (true, "open".to_string()),
                Ok(Err(e)) => (false, match e.kind() {
                    std::io::ErrorKind::ConnectionRefused => "closed (refused)".into(),
                    _ => format!("closed ({e})"),
                }),
                Err(_) => (false, "filtered (no answer)".into()),
            };
            PortResult { port, open, elapsed_ms: start.elapsed().as_millis(), detail }
        }));
    }
    let mut out = Vec::new();
    for t in tasks {
        out.push(t.await.map_err(err)?);
    }
    out.sort_by_key(|r| r.port);
    Ok(out)
}

/// Largest payload that gets through without fragmentation, by binary
/// search over do-not-fragment pings. Reports the IP MTU (payload + 28).
#[tauri::command]
pub async fn net_mtu(jobs: State<'_, NetJobs>, id: String, target: String, events: Channel<Line>) -> CmdResult<u32> {
    let target = check_target(&target)?;
    let mut cancel = jobs.start(&id);
    let (mut lo, mut hi) = (548u32, 1472u32 + 7 * 1000); // allow jumbo frames up to ~8500
    let probe = |size: u32| {
        let mut cmd = Command::new("ping");
        #[cfg(windows)]
        cmd.args(["-n", "1", "-w", "1500", "-f", "-l", &size.to_string()]);
        #[cfg(target_os = "macos")]
        cmd.args(["-c", "1", "-W", "1500", "-D", "-s", &size.to_string()]);
        #[cfg(all(unix, not(target_os = "macos")))]
        cmd.args(["-c", "1", "-W", "2", "-M", "do", "-s", &size.to_string()]);
        cmd.arg(&target).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd
    };
    // first make sure the host answers at all
    let base = probe(56).output().await.map_err(err)?;
    if !base.status.success() {
        jobs.finish(&id);
        let _ = events.send(Line::Done { ok: false, summary: "no reply to a small ping".into() });
        return Err(format!("{target} does not answer a 56 byte ping"));
    }
    let mut best = 0;
    // widen the top first so jumbo paths are found quickly
    let mut tries = 0;
    while lo <= hi && tries < 20 {
        if *cancel.borrow_and_update() {
            jobs.finish(&id);
            return Err("cancelled".into());
        }
        tries += 1;
        let mid = (lo + hi) / 2;
        let out = probe(mid).output().await.map_err(err)?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let ok = out.status.success() && !text.contains("needs to be fragmented") && !text.contains("Message too long") && !text.contains("frag needed");
        let _ = events.send(Line::Line { text: format!("{} byte payload: {}", mid, if ok { "ok" } else { "too big" }) });
        if ok {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    jobs.finish(&id);
    let mtu = best + 28;
    let _ = events.send(Line::Done { ok: best > 0, summary: format!("path MTU {mtu} bytes ({best} byte ICMP payload)") });
    Ok(mtu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_checked() {
        assert!(check_target("10.0.0.1").is_ok());
        assert!(check_target("core-sw1.example.net").is_ok());
        assert!(check_target("2001:db8::1").is_ok());
        assert!(check_target("-c 1000 host").is_err());
        assert!(check_target("host; rm -rf /").is_err());
        assert!(check_target("").is_err());
    }

    #[test]
    fn ping_summaries() {
        let linux = ["PING 10.0.0.1 (10.0.0.1) 56(84) bytes of data.", "64 bytes from 10.0.0.1: icmp_seq=1 ttl=255 time=0.42 ms", "--- 10.0.0.1 ping statistics ---", "2 packets transmitted, 2 received, 0% packet loss, time 1001ms", "rtt min/avg/max/mdev = 0.42/0.51/0.60/0.09 ms"].map(String::from);
        assert_eq!(ping_summary(&linux), "0% loss, min/avg/max 0.42/0.51/0.60 ms");
        let win = ["Pinging 10.0.0.1 with 32 bytes of data:", "Reply from 10.0.0.1: bytes=32 time<1ms TTL=255", "    Packets: Sent = 4, Received = 4, Lost = 0 (0% loss),", "    Minimum = 0ms, Maximum = 1ms, Average = 0ms"].map(String::from);
        assert_eq!(ping_summary(&win), "0% loss, min/avg/max 0/0/1 ms");
        assert_eq!(ping_summary(&["ping: unknown host nope.invalid".to_string()]), "name not found");
    }

    #[tokio::test]
    async fn port_check_local() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().port();
        let closed = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
        let r = net_ports("127.0.0.1".into(), vec![open, closed], Some(1000)).await.unwrap();
        assert!(r.iter().find(|p| p.port == open).unwrap().open);
        assert!(!r.iter().find(|p| p.port == closed).unwrap().open);
        let dns = net_dns("localhost".into(), None, None).await.unwrap();
        assert!(dns.addresses.iter().any(|a| a == "127.0.0.1" || a == "::1"));
    }
}
