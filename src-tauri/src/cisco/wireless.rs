//! Wireless controller evaluators shared by the Catalyst 9800 (IOS-XE) and
//! AireOS. Device output is data: every verdict quotes the lines it rests on.

use super::checks::{first, grep, num, re, Outputs, Status, Verdict};
use super::detect::Os;
use super::generic::commands;

pub fn evaluate(os: Os, id: &str, out: &Outputs) -> Verdict {
    let cmds = commands(os, id);
    let Some(text) = first(out, &cmds) else { return Verdict::unknown("the command did not run") };
    let all = || -> String { cmds.iter().filter_map(|c| out.get(*c).and_then(|r| r.as_ref().ok())).map(|s| s.as_str()).collect::<Vec<_>>().join("\n") };
    match id {
        "ap-inventory" => ap_inventory(&all()),
        "ap-join" => ap_join(os, text),
        "ap-images" => ap_images(text),
        "wlans" => wlans(&all()),
        "clients" => clients(os, &all()),
        "rf" => rf(&all()),
        _ => Verdict::unknown(format!("no wireless evaluator for {id}")),
    }
}

/// Count of joined APs. 9800: "Number of APs: 42"; AireOS: "Number of APs.................................... 42".
/// The expected count is compared in the UI, which knows the site.
fn ap_inventory(text: &str) -> Verdict {
    let total = re(r"(?im)^\s*Number of APs\.*\s*:?\s*(\d+)").captures(text).and_then(|c| num(&c[1]));
    // AP rows: name, slots, model, mac, location, country, ip, state (9800) — a down/disabled AP has a non-registered state
    let rows: Vec<&str> = text.lines().filter(|l| re(r"(?i)\b(AIR-|C9\d{3}|CW9\d{3}|AP\d{3,4})\b").is_match(l)).collect();
    let bad: Vec<String> = rows.iter().filter(|l| re(r"(?i)\b(disabled|not joined|downloading|down|dtls)\b").is_match(l)).map(|l| l.trim_end().to_string()).collect();
    let n = total.map(|t| t as usize).unwrap_or(rows.len());
    if total.is_none() && rows.is_empty() {
        return Verdict::unknown("could not count access points").evidence(text.lines().take(6).map(|l| l.to_string()).collect());
    }
    let v = if n == 0 {
        Verdict::new(Status::Critical, "No access points joined")
    } else if !bad.is_empty() {
        Verdict::new(Status::Warning, format!("{n} APs joined, {} not fully registered", bad.len()))
    } else {
        Verdict::new(Status::Healthy, format!("{n} APs joined"))
    };
    let mut ev = bad.clone();
    ev.extend(grep(text, r"(?i)Number of APs|Country|Standby").into_iter().take(5));
    let v = v.evidence(ev);
    if n == 0 || !bad.is_empty() { v.follow_up(&["show ap summary", "show ap uptime", "show wireless stats ap join summary"]) } else { v }
}

/// Join statistics. 9800 summary: "Number of APs failed to join : 3"; AireOS: "Number of APs that failed to join... 3" plus a per-AP table.
fn ap_join(_os: Os, text: &str) -> Verdict {
    let failed = re(r"(?im)^\s*Number of APs(?: that)? failed to join\.*\s*:?\s*(\d+)").captures(text).and_then(|c| num(&c[1]));
    let joined = re(r"(?im)^\s*Number of APs(?: that)? (?:successfully )?joined\.*\s*:?\s*(\d+)").captures(text).and_then(|c| num(&c[1]));
    let fail_rows = grep(text, r"(?i)\b(fail|reject|dtls|not joined|disjoin)").into_iter().filter(|l| !re(r"(?i)failed to join\.*\s*:?\s*0\b").is_match(l)).collect::<Vec<_>>();
    match failed {
        Some(f) if f > 0.0 => Verdict::new(Status::Warning, format!("{f:.0} AP join failure(s){}", joined.map(|j| format!(", {j:.0} joined")).unwrap_or_default()))
            .evidence(fail_rows)
            .follow_up(&["show wireless stats ap join summary", "show ap join stats summary all", "show ap name <AP> join stats detailed", "show logging | include CAPWAP|DTLS|JOIN"]),
        Some(_) => Verdict::new(Status::Healthy, format!("No AP join failures{}", joined.map(|j| format!(" ({j:.0} joined)")).unwrap_or_default())).evidence(grep(text, r"(?i)join")),
        None if !fail_rows.is_empty() => Verdict::new(Status::Warning, format!("{} AP(s) report join problems", fail_rows.len())).evidence(fail_rows).follow_up(&["show ap join stats summary all", "show logging | include CAPWAP|DTLS|JOIN"]),
        None => Verdict::unknown("could not read join statistics").evidence(text.lines().take(6).map(|l| l.to_string()).collect()),
    }
}

/// AP images should match the controller's version; mixed versions after an upgrade mean predownload is incomplete.
fn ap_images(text: &str) -> Verdict {
    let ver = re(r"\b(\d{1,2}\.\d{1,2}\.\d{1,3}\.\d{1,3})\b");
    let mut versions: std::collections::BTreeMap<String, usize> = Default::default();
    for l in text.lines() {
        if let Some(c) = ver.captures(l) {
            if re(r"(?i)^\s*(Primary|Backup|Predownload|Total|Image)").is_match(l) && !re(r"(?i)\bAP\b").is_match(l) {
                continue; // controller-level summary lines
            }
            *versions.entry(c[1].to_string()).or_insert(0) += 1;
        }
    }
    let pending = grep(text, r"(?i)predownload(ing)?|downloading|in progress|mismatch");
    if versions.is_empty() {
        return Verdict::unknown("could not read AP image versions").evidence(text.lines().take(6).map(|l| l.to_string()).collect());
    }
    let listing: Vec<String> = versions.iter().map(|(v, n)| format!("{v}: {n} AP(s)")).collect();
    let v = if versions.len() > 1 {
        Verdict::new(Status::Warning, format!("{} different AP image versions", versions.len())).follow_up(&["show ap image summary", "show ap image all", "ap image predownload (after approval)"])
    } else if !pending.is_empty() {
        Verdict::new(Status::Warning, format!("{} AP(s) still downloading an image", pending.len()))
    } else {
        Verdict::new(Status::Healthy, format!("All APs on {}", versions.keys().next().cloned().unwrap_or_default()))
    };
    let mut ev = listing;
    ev.extend(pending.into_iter().take(10));
    v.evidence(ev)
}

/// WLANs: at least one enabled; tags (9800) resolve. Disabled WLANs are listed, not failed.
fn wlans(text: &str) -> Verdict {
    let rows: Vec<&str> = text.lines().filter(|l| re(r"(?m)^\s*\d{1,4}\s+\S").is_match(l)).collect();
    let enabled = rows.iter().filter(|l| re(r"(?i)\b(UP|Enabled)\b").is_match(l)).count();
    let disabled: Vec<String> = rows.iter().filter(|l| re(r"(?i)\b(DOWN|Disabled)\b").is_match(l)).map(|l| l.trim_end().to_string()).collect();
    let open: Vec<String> = rows.iter().filter(|l| re(r"(?i)\[open\]|\bopen\b").is_match(l) && !re(r"(?i)wpa|802\.1x|psk").is_match(l)).map(|l| l.trim_end().to_string()).collect();
    if rows.is_empty() {
        return Verdict::unknown("no WLANs listed").evidence(text.lines().take(6).map(|l| l.to_string()).collect());
    }
    let v = if enabled == 0 {
        Verdict::new(Status::Critical, format!("{} WLANs, none enabled", rows.len())).follow_up(&["show wlan summary", "show wireless tag policy summary", "show wireless profile policy summary"])
    } else if !open.is_empty() {
        Verdict::new(Status::Warning, format!("{enabled} of {} WLANs enabled; {} open (no security)", rows.len(), open.len()))
    } else {
        Verdict::new(Status::Healthy, format!("{enabled} of {} WLANs enabled", rows.len()))
    };
    let mut ev = open;
    ev.extend(disabled);
    ev.extend(rows.iter().take(15).map(|l| l.trim_end().to_string()));
    v.evidence(ev)
}

/// Client counts and auth trouble. 9800: "Number of Clients: N" and delete reasons; AireOS: client table with a Status column.
fn clients(_os: Os, text: &str) -> Verdict {
    let total = re(r"(?im)^\s*Number of (?:Local )?Clients\.*\s*:?\s*(\d+)").captures(text).and_then(|c| num(&c[1]));
    let excluded = re(r"(?im)^\s*Number of Excluded Clients\.*\s*:?\s*(\d+)").captures(text).and_then(|c| num(&c[1])).unwrap_or(0.0);
    // AireOS rows where the status column is not Associated
    let stuck = grep(text, r"(?i)\b(Probing|Excluded|Authenticating|802\.1x_Reqd|WebAuth_Reqd|IP Learn|Ip_Learn|Dhcp_Reqd)\b");
    // 9800 delete reasons with counts > 0 are only informative; AAA / auth ones warn
    let auth_reasons = grep(text, r"(?i)(AAA|auth|802\.1x|EAP|key exchange|4-way|timeout).*?:\s*[1-9]\d*\s*$");
    let count = total.map(|t| format!("{t:.0} clients")).unwrap_or_else(|| "client counts unavailable".into());
    let v = if excluded > 10.0 || stuck.len() > 10 {
        Verdict::new(Status::Warning, format!("{count}; {} excluded, {} stuck before run state", excluded as usize, stuck.len())).follow_up(&["show wireless client summary", "show wireless exclusionlist", "show wireless stats client delete reasons", "show client summary", "show exclusionlist"])
    } else if auth_reasons.len() > 3 {
        Verdict::new(Status::Warning, format!("{count}; {} authentication-related delete reasons", auth_reasons.len())).follow_up(&["show wireless stats client delete reasons", "show aaa servers", "show radius statistics"])
    } else if total.is_none() && stuck.is_empty() && auth_reasons.is_empty() {
        return Verdict::unknown("could not read the client summary").evidence(text.lines().take(6).map(|l| l.to_string()).collect());
    } else {
        Verdict::new(Status::Healthy, count)
    };
    let mut ev = stuck.into_iter().take(10).collect::<Vec<_>>();
    ev.extend(auth_reasons.into_iter().take(10));
    ev.extend(grep(text, r"(?i)Number of").into_iter().take(6));
    v.evidence(ev)
}

/// RF: channel utilization per radio. 9800 load-info columns: AP Name, Slot, Channel Util %, ... ; AireOS advanced summary lists DCA/TPC state.
fn rf(text: &str) -> Verdict {
    let mut busy = Vec::new();
    let mut radios = 0usize;
    for l in text.lines() {
        if !re(r"(?i)\b(AIR-|C9\d{3}|CW9\d{3}|AP[\w-]*\d)\b").is_match(l) {
            continue;
        }
        // columns after the AP name: slot, then the utilization percentages
        let nums: Vec<f64> = l.split_whitespace().skip(1).filter_map(|t| num(t.trim_end_matches('%'))).collect();
        if let Some(util) = nums.iter().skip(1).find(|n| **n <= 100.0) {
            radios += 1;
            if *util >= 70.0 {
                busy.push(format!("{} (util {util:.0}%)", l.split_whitespace().next().unwrap_or("")));
            }
        }
    }
    let rrm_off = grep(text, r"(?i)(DCA|TPC|Dynamic Channel|Tx Power).*?(off|disabled|static)");
    if radios == 0 && rrm_off.is_empty() {
        return Verdict::unknown("no per-radio load information").evidence(text.lines().take(6).map(|l| l.to_string()).collect());
    }
    let v = if busy.len() > radios / 4 && !busy.is_empty() {
        Verdict::new(Status::Warning, format!("{} of {radios} radios above 70% channel utilization", busy.len())).follow_up(&["show ap dot11 5ghz load-info", "show ap dot11 24ghz summary", "show ap auto-rf dot11 5ghz <AP>", "show advanced 802.11a channel"])
    } else if !rrm_off.is_empty() {
        Verdict::new(Status::Warning, "RRM (DCA/TPC) not automatic").follow_up(&["show advanced 802.11a channel", "show advanced 802.11a txpower"])
    } else {
        Verdict::new(Status::Healthy, format!("{radios} radios, {} busy", busy.len()))
    };
    let mut ev = busy;
    ev.extend(rrm_off);
    v.evidence(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(pairs: &[(&str, &str)]) -> Outputs {
        pairs.iter().map(|(c, t)| (c.to_string(), Ok(t.to_string()))).collect()
    }

    #[test]
    fn nine800_ap_summary_and_join() {
        let o = out(&[
            ("show wireless summary", "Access Point Summary\n\n                         Total    Up    Down\n--------------------------------------------\n802.11ax 2.4 GHz Radios   42       42    0\n\nNumber of APs: 42\n"),
            ("show ap summary", "Number of APs: 42\n\nAP Name   Slots  AP Model     Ethernet MAC    Radio MAC       Location   Country  IP Address    State\nAP-101    2      C9120AXI-E   0000.1111.2222  0000.1111.2230  default    US       10.1.1.101    Registered\nAP-102    2      C9120AXI-E   0000.1111.3333  0000.1111.3340  default    US       10.1.1.102    Registered\n"),
            ("show wireless stats ap join summary", "Number of APs: 42\n\nBase MAC   Ethernet MAC  AP Name   IP Address  Status   Last Failure Phase  Last Disconnect Reason\n...\nNumber of APs failed to join : 0\n"),
        ]);
        let v = evaluate(Os::IosXe, "ap-inventory", &o);
        assert_eq!(v.status, Status::Healthy, "{}", v.summary);
        assert!(v.summary.starts_with("42 APs"));
        assert_eq!(evaluate(Os::IosXe, "ap-join", &o).status, Status::Healthy);
        let o = out(&[("show wireless stats ap join summary", "Number of APs failed to join : 3\nAP-200  10.1.1.200  Not Joined  DTLS setup  DTLS close alert from peer\n")]);
        let v = evaluate(Os::IosXe, "ap-join", &o);
        assert_eq!(v.status, Status::Warning);
        assert!(v.evidence.iter().any(|e| e.contains("AP-200")));
    }

    #[test]
    fn aireos_clients_and_images() {
        let o = out(&[("show client summary", "Number of Clients................................ 153\nNumber of EoGRE Clients.......................... 0\n\nMAC Address       AP Name           Slot Status        WLAN Auth Protocol  Port Wired\n00:11:22:33:44:55 AP-1              1    Associated    1    Yes  802.11ac   13   No\n00:11:22:33:44:66 AP-2              1    Probing       N/A  No   802.11n    13   No\n")]);
        let v = evaluate(Os::Aireos, "clients", &o);
        assert_eq!(v.status, Status::Healthy, "{}", v.summary);
        assert!(v.summary.contains("153"));
        let o = out(&[("show ap image all", "AP Name   Primary Image   Backup Image   Predownload Status\nAP-1      8.10.185.0      8.10.183.0     None\nAP-2      8.10.183.0      8.10.185.0     None\n")]);
        let v = evaluate(Os::Aireos, "ap-images", &o);
        assert_eq!(v.status, Status::Warning, "{}", v.summary);
    }

    #[test]
    fn wlans_open_and_disabled() {
        let o = out(&[("show wlan summary", "Number of WLANs: 3\n\nID   Profile Name   SSID        Status   Security\n1    corp           Corp        UP       [WPA2][802.1x][AES]\n2    guest          Guest       UP       [open],MAC Filtering\n3    iot            IoT         DOWN     [WPA2][PSK][AES]\n")]);
        let v = evaluate(Os::IosXe, "wlans", &o);
        assert_eq!(v.status, Status::Warning, "{}", v.summary);
        assert!(v.summary.contains("1 open"));
    }

    #[test]
    fn rf_busy_radios() {
        let o = out(&[("show ap dot11 5ghz load-info", "AP Name   Slot  Channel Util  Tx Util  Rx Util  Clients\nAP-1      1     12            3        5        9\nAP-2      1     88            40       30       31\n"), ("show ap dot11 24ghz load-info", "")]);
        let v = evaluate(Os::IosXe, "rf", &o);
        assert_eq!(v.status, Status::Warning, "{}", v.summary);
        assert!(v.evidence[0].starts_with("AP-2"));
    }
}
