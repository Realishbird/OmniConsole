//! Runs a command on a live session and collects its output: the bytes the
//! device sends are copied into a buffer until the prompt comes back, with
//! `--More--` pages answered automatically. The user still sees everything
//! in the terminal; nothing is hidden.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// Shared between the session task (which feeds output) and the runner.
#[derive(Default)]
pub struct Capture {
    buf: Mutex<Vec<u8>>,
    active: Mutex<bool>,
    changed: Notify,
}

impl Capture {
    pub fn feed(&self, bytes: &[u8]) {
        if *self.active.lock().unwrap() {
            self.buf.lock().unwrap().extend_from_slice(bytes);
            self.changed.notify_one();
        }
    }

    fn start(&self) {
        self.buf.lock().unwrap().clear();
        *self.active.lock().unwrap() = true;
    }

    fn stop(&self) -> Vec<u8> {
        *self.active.lock().unwrap() = false;
        std::mem::take(&mut *self.buf.lock().unwrap())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().unwrap()).into_owned()
    }
}

/// Cisco prompts: `sw1#`, `sw1>`, `sw1(config-if)#`, `RP/0/RSP0/CPU0:r1#`,
/// ASA `fw/pri/act#`; also plain shells ending in `$`.
pub fn prompt_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[A-Za-z0-9][\w.\-/:@~]{0,62}(?:\([\w\-.]+\))?[#>$] ?$").unwrap())
}

fn more_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)-+ ?\(?more\)?(?: \d+%)? ?-+\s*$|<--- More --->\s*$|Press any key to continue").unwrap())
}

/// Remove escape sequences, carriage returns and backspaces.
fn strip_controls(raw: &str) -> String {
    static ESC: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let esc = ESC.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b[()][A-Za-z0-9]|\x1b[=>]").unwrap());
    let text = esc.replace_all(raw, "");
    let mut out = String::with_capacity(text.len());
    // a carriage return moves back to column 0 and later text overwrites,
    // which is how the pager wipes " --More-- "
    let mut line: Vec<char> = Vec::new();
    let mut col = 0usize;
    let put = |line: &mut Vec<char>, col: &mut usize, c: char| {
        if *col < line.len() {
            line[*col] = c;
        } else {
            line.push(c);
        }
        *col += 1;
    };
    for ch in text.chars() {
        match ch {
            '\x08' => col = col.saturating_sub(1),
            '\r' => col = 0,
            '\n' => {
                let l: String = line.iter().collect();
                out.push_str(l.trim_end());
                out.push('\n');
                line.clear();
                col = 0;
            }
            c if c.is_control() && c != '\t' => {}
            c => put(&mut line, &mut col, c),
        }
    }
    let line: String = line.iter().collect();
    out.push_str(line.trim_end());
    out
}

/// `strip_controls` plus the pager's own text removed.
pub fn clean(raw: &str) -> String {
    let out = strip_controls(raw);
    // leftover pager prompts on their own line
    static MORE_LINE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let more_line = MORE_LINE.get_or_init(|| Regex::new(r"(?m)^\s*-+ ?\(?[Mm]ore\)?(?: \d+%)? ?-+\s*$\n?|^<--- More --->\s*$\n?").unwrap());
    more_line.replace_all(&out, "").into_owned()
}

/// Strip the echoed command from the top and the prompt from the bottom.
fn trim_echo_and_prompt(text: &str, command: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.last().is_some_and(|l| prompt_regex().is_match(l.trim_end())) {
        lines.pop();
    }
    let cmd = command.trim();
    let mut start = 0;
    for (i, l) in lines.iter().enumerate().take(3) {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        // "sw1#show version" or just "show version" (serial echoes without the prompt)
        if l.ends_with(cmd) || cmd.starts_with(l) {
            start = i + 1;
        }
        break;
    }
    lines[start..].join("\n").trim_matches('\n').to_string()
}

/// How to answer the questions a command asks (`[confirm]`, `[yes/no]`,
/// `Destination filename [x]?`). Only reviewed deployments use this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Answers {
    /// "yes" or "no" for [yes/no] questions (saving before a reload, for one)
    pub yes_no: YesNo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum YesNo {
    Yes,
    No,
}

fn question_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(\[confirm\]\s*$|\[yes/no\]:?\s*$|\(yes/no\)\??\s*\[?\w*\]?:?\s*$|\[y/n\]:?\s*$|\(y/n\)\??\s*\[?\w*\]?:?\s*$|\[[^\]]*\]\?\s*$|\?\s*$)").unwrap())
}

/// The answer to a question line, with a trailing CR.
fn answer_for(line: &str, answers: Answers) -> Vec<u8> {
    let l = line.to_lowercase();
    if l.contains("[yes/no]") || l.contains("(yes/no)") {
        match answers.yes_no { YesNo::Yes => b"yes\r".to_vec(), YesNo::No => b"no\r".to_vec() }
    } else if l.contains("[y/n]") || l.contains("(y/n)") {
        match answers.yes_no { YesNo::Yes => b"y\r".to_vec(), YesNo::No => b"n\r".to_vec() }
    } else {
        b"\r".to_vec()
    }
}

pub struct Runner<'a> {
    pub capture: &'a Arc<Capture>,
    pub send: &'a (dyn Fn(Vec<u8>) + Send + Sync),
}

impl Runner<'_> {
    /// Send `command` and wait for the prompt. `timeout` bounds the whole
    /// command; a device that keeps printing resets the quiet timer.
    #[cfg(test)]
    pub async fn run(&self, command: &str, timeout: Duration) -> Result<String> {
        self.run_with(command, timeout, None).await
    }

    /// `run`, answering the command's questions when `answers` is given.
    pub async fn run_with(&self, command: &str, timeout: Duration, answers: Option<Answers>) -> Result<String> {
        self.capture.start();
        (self.send)(format!("{}\r", command.trim()).into_bytes());
        let deadline = tokio::time::Instant::now() + timeout;
        let mut quiet_since = tokio::time::Instant::now();
        let mut seen_len = 0usize;
        let mut answered_at = 0usize;
        let result = loop {
            let text = self.capture.text();
            if text.len() != seen_len {
                seen_len = text.len();
                quiet_since = tokio::time::Instant::now();
                let tail: String = text.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
                // pager text must survive here, so no clean()
                let tail = strip_controls(&tail);
                if more_regex().is_match(tail.trim_end()) {
                    (self.send)(b" ".to_vec());
                } else if tail.lines().last().is_some_and(|l| prompt_regex().is_match(l.trim_end())) && text.contains('\n') {
                    break Ok(());
                }
            } else if let (Some(a), true) = (answers, quiet_since.elapsed() > Duration::from_millis(700) && seen_len > answered_at) {
                // the device went quiet on a question: answer it once
                let tail = strip_controls(&text);
                let last = tail.lines().last().unwrap_or("").trim_end();
                if question_regex().is_match(last) && !prompt_regex().is_match(last) {
                    (self.send)(answer_for(last, a));
                    answered_at = seen_len;
                    quiet_since = tokio::time::Instant::now();
                }
            } else if quiet_since.elapsed() > Duration::from_millis(2500) {
                // no prompt seen but the device went quiet (a prompt we do not recognize)
                let tail = strip_controls(&text);
                if tail.trim_end().ends_with(['#', '>', '$', ':']) {
                    break Ok(());
                }
            }
            if tokio::time::Instant::now() > deadline {
                break Err(anyhow!("no prompt after {} s running `{}`", timeout.as_secs(), command));
            }
            let _ = tokio::time::timeout(Duration::from_millis(250), self.capture.changed.notified()).await;
        };
        let raw = String::from_utf8_lossy(&self.capture.stop()).into_owned();
        result?;
        let text = clean(&raw);
        if text.contains("% Invalid input") || text.contains("% Incomplete command") || text.contains("% Ambiguous command") {
            bail!("the device rejected `{command}`");
        }
        Ok(trim_echo_and_prompt(&text, command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_pager_and_prompt() {
        let raw = "show ip int brief\r\nInterface   IP-Address  Status\r\nVlan1       10.0.0.1    up\r\n --More-- \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08          \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08Gi1/0/1  unassigned  down\r\nsw1#";
        let text = clean(raw);
        let out = trim_echo_and_prompt(&text, "show ip int brief");
        assert_eq!(out, "Interface   IP-Address  Status\nVlan1       10.0.0.1    up\nGi1/0/1  unassigned  down");
    }

    #[test]
    fn prompt_shapes() {
        for p in ["sw1#", "sw1>", "sw1(config-if)#", "RP/0/RSP0/CPU0:asr9k#", "fw/pri/act# ", "SG350#"] {
            assert!(prompt_regex().is_match(p), "{p}");
        }
        for p in ["Password:", "Building configuration...", "% Invalid input detected at '^' marker."] {
            assert!(!prompt_regex().is_match(p), "{p}");
        }
    }

    #[test]
    fn questions_and_answers() {
        for q in ["Destination filename [running-config]?", "Proceed with reload? [confirm]", "System configuration has been modified. Save? [yes/no]:", "Do you want to continue? (y/n) [n]", "Delete flash:/old.bin? [confirm]"] {
            assert!(question_regex().is_match(q), "{q}");
            assert!(!prompt_regex().is_match(q), "{q}");
        }
        assert!(!question_regex().is_match("sw1#"));
        assert_eq!(answer_for("Save? [yes/no]:", Answers { yes_no: YesNo::No }), b"no\r");
        assert_eq!(answer_for("continue? (y/n) [n]", Answers { yes_no: YesNo::Yes }), b"y\r");
        assert_eq!(answer_for("Destination filename [x]?", Answers { yes_no: YesNo::Yes }), b"\r");
    }

    #[tokio::test]
    async fn answers_a_question() {
        let capture = Arc::new(Capture::default());
        let cap = capture.clone();
        let send = move |bytes: Vec<u8>| {
            let cap = cap.clone();
            if bytes == b"\r" {
                tokio::spawn(async move { cap.feed(b"\r\nBuilding configuration...\r\n[OK]\r\nsw1#") });
            } else {
                tokio::spawn(async move { cap.feed(b"copy run flash:x.cfg\r\nDestination filename [x.cfg]? ") });
            }
        };
        let runner = Runner { capture: &capture, send: &send };
        let out = runner.run_with("copy run flash:x.cfg", Duration::from_secs(5), Some(Answers { yes_no: YesNo::Yes })).await.unwrap();
        assert!(out.contains("[OK]"), "{out}");
    }

    #[tokio::test]
    async fn runs_against_a_fake_device() {
        let capture = Arc::new(Capture::default());
        let cap = capture.clone();
        let send = move |bytes: Vec<u8>| {
            // the "device" echoes the command, pages once, then prompts
            let cap = cap.clone();
            if bytes == b" " {
                tokio::spawn(async move { cap.feed(b"\r          \rline 3\r\nsw1#") });
            } else {
                tokio::spawn(async move {
                    cap.feed(b"show run\r\nline 1\r\nline 2\r\n");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    cap.feed(b" --More-- ");
                });
            }
        };
        let runner = Runner { capture: &capture, send: &send };
        let out = runner.run("show run", Duration::from_secs(5)).await.unwrap();
        assert_eq!(out, "line 1\nline 2\nline 3");
    }
}
