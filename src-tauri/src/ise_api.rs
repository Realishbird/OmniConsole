//! Cisco ISE checks that the CLI cannot answer: deployment state, system
//! certificates, Active Directory join points, licensing and live sessions.
//! They use ISE's OpenAPI (port 443) and ERS (port 9060) with HTTP basic
//! auth. The password lives in the OS keychain under the host name and is
//! never returned to the UI, logged or sent anywhere but the ISE node.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cisco::checks::{CheckResult, Status};
use crate::store;

type CmdResult<T> = Result<T, String>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IseApiRequest {
    /// admin node name or address
    pub host: String,
    pub username: String,
    /// accept the node's self-signed admin certificate
    #[serde(default)]
    pub insecure: bool,
    /// warn when a system certificate expires within this many days
    #[serde(default)]
    pub cert_warn_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IseApiReport {
    pub host: String,
    pub results: Vec<CheckResult>,
}

/// Save (or clear, with an empty value) the API password for a host.
/// The username is not secret; the UI keeps it with the host.
#[tauri::command]
pub fn ise_api_credential(host: String, password: String) -> CmdResult<bool> {
    let name = store::secret_name("ise-api", &host).map_err(|e| e.to_string())?;
    if password.is_empty() {
        store::secret_delete(&name).map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        store::secret_set(&name, &password).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
pub async fn ise_api_checks(req: IseApiRequest) -> CmdResult<IseApiReport> {
    let name = store::secret_name("ise-api", &req.host).map_err(|e| e.to_string())?;
    let password = store::secret_get(&name).ok_or_else(|| "no API password saved for this host; save one first".to_string())?;
    let client = Client::new(&req, password).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let warn_days = req.cert_warn_days.unwrap_or(30).max(1);
    let mut results = Vec::new();
    results.push(finish(deployment(&client).await, "ise-deployment", "Deployment nodes", now));
    results.push(finish(certificates(&client, warn_days).await, "ise-certificates", "System certificates", now));
    results.push(finish(active_directory(&client).await, "ise-ad", "Active Directory join points", now));
    results.push(finish(licensing(&client).await, "ise-licensing", "Smart licensing", now));
    results.push(finish(sessions(&client).await, "ise-sessions", "Active sessions", now));
    Ok(IseApiReport { host: req.host, results })
}

/* ------------------------------------------------------------- client */

struct Client {
    http: reqwest::Client,
    host: String,
    username: String,
    password: String,
}

impl Client {
    fn new(req: &IseApiRequest, password: String) -> Result<Self> {
        let host = req.host.trim().trim_end_matches('/').to_string();
        if host.is_empty() || host.contains('/') || host.contains(' ') {
            return Err(anyhow!("bad host name"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .danger_accept_invalid_certs(req.insecure)
            .build()?;
        Ok(Client { http, host, username: req.username.clone(), password })
    }

    /// GET a JSON document; `port` picks OpenAPI (443) or ERS (9060).
    async fn get(&self, port: u16, path: &str) -> Result<Value> {
        let url = format!("https://{}:{port}{path}", self.host);
        let resp = self
            .http
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("connecting to {} port {port}", self.host))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            // never echo the body: it may quote the request headers
            return Err(anyhow!(match status.as_u16() {
                401 => "ISE rejected the credentials (401); check the admin or ERS user and that it has the API access role".to_string(),
                403 => "the account lacks API permissions (403); enable ERS/OpenAPI and give the user ERS Admin or ERS Operator".to_string(),
                404 => format!("{path} not found (404); ERS or OpenAPI may be disabled, or this ISE version lacks it"),
                _ => format!("HTTP {status} from {path}"),
            }));
        }
        serde_json::from_str(&body).with_context(|| format!("{path} did not return JSON"))
    }
}

/* ---------------------------------------------------------- verdicts */

struct Verdict {
    status: Status,
    summary: String,
    evidence: Vec<String>,
    next_step: String,
    source: &'static str,
}

fn v(status: Status, summary: impl Into<String>, evidence: Vec<String>, next_step: &str, source: &'static str) -> Verdict {
    Verdict { status, summary: summary.into(), evidence, next_step: next_step.into(), source }
}

fn finish(r: Result<Verdict>, id: &str, title: &str, now: i64) -> CheckResult {
    let v = match r {
        Ok(v) => v,
        Err(e) => Verdict { status: Status::NotCollected, summary: format!("{e:#}"), evidence: Vec::new(), next_step: "Check that the API is enabled on the admin node and the credentials are right".into(), source: "" },
    };
    CheckResult {
        id: id.into(),
        title: title.into(),
        category: "ISE API".into(),
        status: v.status,
        summary: v.summary,
        evidence: v.evidence,
        follow_up: Vec::new(),
        commands: if v.source.is_empty() { Vec::new() } else { vec![v.source.to_string()] },
        table: None,
        collected_at: now,
        next_step: if v.status == Status::Healthy { String::new() } else { v.next_step },
    }
}

fn s<'a>(x: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| x.get(*k).and_then(|v| v.as_str())).unwrap_or("")
}

fn list(doc: &Value, keys: &[&str]) -> Vec<Value> {
    for k in keys {
        if let Some(a) = doc.get(*k).and_then(|v| v.as_array()) {
            return a.clone();
        }
    }
    doc.as_array().cloned().unwrap_or_default()
}

/// OpenAPI GET /api/v1/deployment/node: every node with its personas, roles and sync state.
async fn deployment(c: &Client) -> Result<Verdict> {
    const SRC: &str = "GET /api/v1/deployment/node";
    let doc = c.get(443, "/api/v1/deployment/node").await?;
    let nodes = list(&doc, &["response", "nodes"]);
    if nodes.is_empty() {
        return Ok(v(Status::NotCollected, "the deployment list came back empty", Vec::new(), "Open Administration > System > Deployment", SRC));
    }
    let mut bad = Vec::new();
    let mut lines = Vec::new();
    for n in &nodes {
        let name = s(n, &["hostname", "name"]);
        let status = s(n, &["nodeStatus", "status", "replicationStatus"]);
        let roles: Vec<String> = n.get("roles").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let services: Vec<String> = n.get("services").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let line = format!("{name}: {} [{}] {}", if status.is_empty() { "state unknown" } else { status }, roles.join(", "), services.join(", "));
        if !status.is_empty() && !status.eq_ignore_ascii_case("Connected") && !status.eq_ignore_ascii_case("Complete") && !status.eq_ignore_ascii_case("in sync") && !status.eq_ignore_ascii_case("standalone") {
            bad.push(line.clone());
        }
        lines.push(line);
    }
    let has_primary = nodes.iter().any(|n| n.get("roles").map(|r| r.to_string().contains("PrimaryAdmin")).unwrap_or(false)) || nodes.len() == 1;
    Ok(if !bad.is_empty() {
        v(Status::Critical, format!("{} of {} nodes not in sync", bad.len(), nodes.len()), lines, "Administration > System > Deployment: check replication; sync the node after the cause is fixed", SRC)
    } else if !has_primary {
        v(Status::Warning, format!("{} nodes, no Primary Administration node reported", nodes.len()), lines, "Promote a PAN in Administration > System > Deployment", SRC)
    } else {
        v(Status::Healthy, format!("{} node{} in sync", nodes.len(), if nodes.len() == 1 { "" } else { "s" }), lines, "", SRC)
    })
}

/// OpenAPI GET /api/v1/certs/system-certificate/{hostname} on every node; expiry within `warn_days` warns, expired is critical.
async fn certificates(c: &Client, warn_days: i64) -> Result<Verdict> {
    const SRC: &str = "GET /api/v1/certs/system-certificate/{node}";
    let nodes = list(&c.get(443, "/api/v1/deployment/node").await?, &["response", "nodes"]);
    let mut names: Vec<String> = nodes.iter().map(|n| s(n, &["hostname", "name"]).to_string()).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        names.push(c.host.split('.').next().unwrap_or(&c.host).to_string());
    }
    let today = chrono::Utc::now().date_naive();
    let mut lines = Vec::new();
    let mut worst = Status::Healthy;
    let mut soon = 0usize;
    let mut expired = 0usize;
    for node in &names {
        let doc = match c.get(443, &format!("/api/v1/certs/system-certificate/{node}")).await {
            Ok(d) => d,
            Err(e) => {
                lines.push(format!("{node}: {e:#}"));
                if worst == Status::Healthy { worst = Status::NotCollected; }
                continue;
            }
        };
        for cert in list(&doc, &["response"]) {
            let name = s(&cert, &["friendlyName", "issuedTo"]);
            let used = s(&cert, &["usedBy", "usage"]);
            let expiry = s(&cert, &["expirationDate", "validTo"]);
            let days = parse_date(expiry).map(|d| (d - today).num_days());
            let line = format!("{node}: {name} ({used}) expires {expiry}{}", days.map(|d| format!(", {d} days")).unwrap_or_default());
            match days {
                Some(d) if d < 0 => { expired += 1; worst = Status::Critical; lines.insert(0, line); }
                Some(d) if d <= warn_days => { soon += 1; if worst != Status::Critical { worst = Status::Warning; } lines.insert(0, line); }
                _ => lines.push(line),
            }
        }
    }
    let summary = if expired > 0 {
        format!("{expired} system certificate(s) expired, {soon} expiring within {warn_days} days")
    } else if soon > 0 {
        format!("{soon} system certificate(s) expiring within {warn_days} days")
    } else if worst == Status::NotCollected {
        "some nodes did not answer the certificate query".to_string()
    } else {
        format!("System certificates valid for more than {warn_days} days on {} node(s)", names.len())
    };
    Ok(v(worst, summary, lines, "Administration > System > Certificates: renew the certificate on each node before it expires (EAP certs break authentication when they lapse)", SRC))
}

fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    // chrono cannot parse zone names, so "Thu Mar 05 10:33:49 UTC 2027" loses its zone first
    // (and the weekday, which chrono would cross-check against the date)
    let t = regex::Regex::new(r"\s+[A-Z]{3,5}\s+(\d{4})$").unwrap().replace(s.trim(), " $1").to_string();
    let t = regex::Regex::new(r"^[A-Za-z]{3},?\s+").unwrap().replace(&t, "").to_string();
    let t = t.as_str();
    for f in ["%b %d %H:%M:%S %Y", "%b %e %H:%M:%S %Y", "%Y-%m-%dT%H:%M:%S%.fZ", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d", "%d %b %Y", "%b %d %Y"] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(t, f) {
            return Some(d.date());
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(t, f) {
            return Some(d);
        }
    }
    chrono::DateTime::parse_from_rfc3339(t).ok().map(|d| d.date_naive()).or_else(|| chrono::DateTime::parse_from_rfc2822(t).ok().map(|d| d.date_naive()))
}

/// ERS GET /ers/config/activedirectory, then each join point's node status.
async fn active_directory(c: &Client) -> Result<Verdict> {
    const SRC: &str = "GET /ers/config/activedirectory";
    let doc = c.get(9060, "/ers/config/activedirectory").await?;
    let points = doc.pointer("/SearchResult/resources").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    if points.is_empty() {
        return Ok(v(Status::NotApplicable, "No Active Directory join points configured", Vec::new(), "", SRC));
    }
    let mut lines = Vec::new();
    let mut bad = 0usize;
    for p in &points {
        let id = s(p, &["id"]);
        let name = s(p, &["name"]);
        let detail = c.get(9060, &format!("/ers/config/activedirectory/{id}")).await;
        match detail {
            Ok(d) => {
                let domain = d.pointer("/ERSActiveDirectory/domain").and_then(|x| x.as_str()).unwrap_or("");
                // joined nodes are reported per node when the admin enabled it; otherwise the join point exists only
                let joined = d.pointer("/ERSActiveDirectory/adgroups").is_some() || d.pointer("/ERSActiveDirectory/domain").is_some();
                lines.push(format!("{name}: domain {domain}{}", if joined { "" } else { " (no detail)" }));
            }
            Err(e) => {
                bad += 1;
                lines.push(format!("{name}: {e:#}"));
            }
        }
    }
    Ok(if bad > 0 {
        v(Status::Warning, format!("{} join point(s), {bad} could not be read", points.len()), lines, "Administration > Identity Management > External Identity Sources > Active Directory: test the connection on each node", SRC)
    } else {
        v(Status::Healthy, format!("{} Active Directory join point(s) configured", points.len()), lines, "", SRC)
    })
}

/// OpenAPI GET /api/v1/license/system/smart-state and /api/v1/license/system/tier-state.
async fn licensing(c: &Client) -> Result<Verdict> {
    const SRC: &str = "GET /api/v1/license/system/smart-state";
    let smart = c.get(443, "/api/v1/license/system/smart-state").await?;
    let reg = s(smart.get("response").unwrap_or(&smart), &["registrationState", "state"]).to_string();
    let conn = s(smart.get("response").unwrap_or(&smart), &["connectionType"]).to_string();
    let mut lines = vec![format!("Smart licensing: {} {}", if reg.is_empty() { "state unknown" } else { &reg }, conn).trim().to_string()];
    let mut bad = Vec::new();
    if let Ok(tiers) = c.get(443, "/api/v1/license/system/tier-state").await {
        for t in list(&tiers, &["response"]) {
            let name = s(&t, &["name"]);
            let status = s(&t, &["status", "complianceStatus"]);
            let consumption = t.get("consumptionCounter").map(|x| x.to_string()).unwrap_or_default();
            let line = format!("{name}: {status} {consumption}").trim().to_string();
            if !status.is_empty() && !status.eq_ignore_ascii_case("COMPLIANCE") && !status.eq_ignore_ascii_case("compliant") && !status.eq_ignore_ascii_case("ENABLED") {
                bad.push(line.clone());
            }
            lines.push(line);
        }
    }
    let unregistered = !reg.is_empty() && !reg.eq_ignore_ascii_case("REGISTERED") && !reg.eq_ignore_ascii_case("registered") && !reg.eq_ignore_ascii_case("PLR");
    Ok(if !bad.is_empty() {
        v(Status::Warning, format!("{} license tier(s) out of compliance", bad.len()), lines, "Administration > System > Licensing: check Smart Account entitlements", SRC)
    } else if unregistered {
        v(Status::Warning, format!("Smart licensing {reg}"), lines, "Register the deployment with Smart Software Manager (or check the SSM On-Prem connection)", SRC)
    } else {
        v(Status::Healthy, if reg.is_empty() { "Licensing reported".to_string() } else { format!("Smart licensing {reg}") }, lines, "", SRC)
    })
}

/// MnT GET /admin/API/mnt/Session/ActiveCount (XML; count element), the only live-auth number without a session dump.
async fn sessions(c: &Client) -> Result<Verdict> {
    const SRC: &str = "GET /admin/API/mnt/Session/ActiveCount";
    let url = format!("https://{}/admin/API/mnt/Session/ActiveCount", c.host);
    let resp = c.http.get(&url).basic_auth(&c.username, Some(&c.password)).header("Accept", "application/xml").send().await.context("connecting to the MnT node")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(if status.as_u16() == 401 || status.as_u16() == 403 { "the MnT API refused the credentials; the user needs the MnT Admin or Super Admin role".to_string() } else { format!("HTTP {status} from the MnT session API") }));
    }
    let count = regex::Regex::new(r"<count>\s*(\d+)\s*</count>").unwrap().captures(&body).and_then(|c| c[1].parse::<u64>().ok());
    Ok(match count {
        Some(n) => v(Status::Healthy, format!("{n} active sessions"), vec![format!("Active sessions on the MnT node: {n}")], "", SRC),
        None => v(Status::NotCollected, "the MnT node did not return a session count", Vec::new(), "Operations > RADIUS > Live Sessions", SRC),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ise_expiry_dates() {
        assert!(parse_date("Thu Mar 05 10:33:49 UTC 2027").is_some());
        assert!(parse_date("2027-03-05").is_some());
        assert!(parse_date("2027-03-05T10:33:49.000Z").is_some());
        assert!(parse_date("never").is_none());
    }

    #[test]
    fn deployment_rows_read_names_and_states() {
        let n: Value = serde_json::json!({"hostname":"ise-pan1","nodeStatus":"Connected","roles":["PrimaryAdmin","PrimaryMonitoring"],"services":["Session","Profiler"]});
        assert_eq!(s(&n, &["hostname", "name"]), "ise-pan1");
        assert_eq!(list(&serde_json::json!({"response":[n.clone()]}), &["response"]).len(), 1);
    }
}
