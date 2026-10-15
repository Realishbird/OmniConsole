//! Session logs: device output as plain text, one line at a time, with
//! escape sequences, pager redraws and backspaces resolved, optional
//! timestamps, and secrets masked (see redact.rs).

use std::fs::{self, File};

/// Widest line the log will pad to, whatever an escape sequence asks for.
const MAX_COL: usize = 4096;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::redact::Redactor;

#[derive(Clone, Copy, PartialEq)]
enum Esc {
    None,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

pub struct SessionLog {
    out: BufWriter<File>,
    pub path: PathBuf,
    timestamps: bool,
    redactor: Redactor,
    line: Vec<char>,
    col: usize,
    esc: Esc,
    csi: String,
    pending_utf8: Vec<u8>,
}

fn safe_name(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let s = s.trim_matches('_');
    if s.is_empty() { "session".into() } else { s.chars().take(60).collect() }
}

impl SessionLog {
    /// <dir>/<YYYY-MM-DD>/<label>_<HH-MM-SS>.log
    pub fn create(dir: &Path, label: &str, describe: &str, timestamps: bool) -> Result<Self> {
        let now = chrono::Local::now();
        let day = dir.join(now.format("%Y-%m-%d").to_string());
        fs::create_dir_all(&day).with_context(|| format!("creating {}", day.display()))?;
        let path = day.join(format!("{}_{}.log", safe_name(label), now.format("%H-%M-%S")));
        let file = File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        let mut log = SessionLog {
            out: BufWriter::new(file),
            path,
            timestamps,
            redactor: Redactor::default(),
            line: Vec::new(),
            col: 0,
            esc: Esc::None,
            csi: String::new(),
            pending_utf8: Vec::new(),
        };
        writeln!(log.out, "# OmniConsole session log: {describe}, started {}", now.format("%Y-%m-%d %H:%M:%S %z"))?;
        writeln!(log.out, "# secrets such as passwords and keys are replaced with <removed>")?;
        log.out.flush()?;
        Ok(log)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        // keep an incomplete UTF-8 sequence for the next chunk
        let mut data = std::mem::take(&mut self.pending_utf8);
        data.extend_from_slice(bytes);
        let valid_up_to = match std::str::from_utf8(&data) {
            Ok(_) => data.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => data.len(),
        };
        self.pending_utf8 = data.split_off(valid_up_to);
        let text = String::from_utf8_lossy(&data).into_owned();
        for c in text.chars() {
            self.char(c);
        }
        let _ = self.out.flush();
    }

    fn char(&mut self, c: char) {
        match self.esc {
            Esc::Esc => {
                self.esc = match c {
                    '[' => {
                        self.csi.clear();
                        Esc::Csi
                    }
                    ']' => Esc::Osc,
                    _ => Esc::None,
                };
                return;
            }
            Esc::Csi => {
                if ('\x40'..='\x7e').contains(&c) {
                    self.esc = Esc::None;
                    self.csi_final(c);
                } else {
                    self.csi.push(c);
                }
                return;
            }
            Esc::Osc => {
                if c == '\x07' {
                    self.esc = Esc::None;
                } else if c == '\x1b' {
                    self.esc = Esc::OscEsc;
                }
                return;
            }
            Esc::OscEsc => {
                self.esc = if c == '\\' { Esc::None } else { Esc::Osc };
                return;
            }
            Esc::None => {}
        }
        match c {
            '\x1b' => self.esc = Esc::Esc,
            '\n' => self.end_line(),
            '\r' => self.col = 0,
            '\x08' => self.col = self.col.saturating_sub(1),
            '\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.col < next {
                    self.put(' ');
                }
            }
            c if c.is_control() => {}
            c => self.put(c),
        }
    }

    fn put(&mut self, c: char) {
        if self.col < self.line.len() {
            self.line[self.col] = c;
        } else {
            while self.line.len() < self.col {
                self.line.push(' ');
            }
            self.line.push(c);
        }
        self.col += 1;
    }

    fn csi_final(&mut self, c: char) {
        let n: usize = self.csi.trim_start_matches('?').split(';').next().and_then(|v| v.parse().ok()).unwrap_or(0);
        match c {
            'K' => match n {
                0 => self.line.truncate(self.col),
                1 => {
                    for ch in self.line.iter_mut().take(self.col) {
                        *ch = ' ';
                    }
                }
                _ => self.line.clear(),
            },
            'D' => self.col = self.col.saturating_sub(n.max(1)),
            // a device can send any number here; the line never needs more than a screen
            'C' => self.col = (self.col + n.max(1)).min(MAX_COL),
            'G' => self.col = (n.max(1) - 1).min(MAX_COL),
            _ => {}
        }
    }

    fn end_line(&mut self) {
        let text: String = self.line.iter().collect::<String>().trim_end().to_string();
        let text = self.redactor.line(&text).into_owned();
        if self.timestamps {
            let _ = writeln!(self.out, "[{}] {}", chrono::Local::now().format("%H:%M:%S%.3f"), text);
        } else {
            let _ = writeln!(self.out, "{text}");
        }
        self.line.clear();
        self.col = 0;
    }
}

impl Drop for SessionLog {
    fn drop(&mut self) {
        if !self.line.is_empty() {
            self.end_line();
        }
        let _ = writeln!(self.out, "# session ended {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_pager_colors_and_secrets() {
        let dir = std::env::temp_dir().join(format!("omni-log-{}", uuid::Uuid::new_v4()));
        let path;
        {
            let mut log = SessionLog::create(&dir, "core sw1", "SSH core-sw1:22", false).unwrap();
            path = log.path.clone();
            log.feed(b"sw1#show run\r\n\x1b[1mBuilding\x1b[0m configuration...\r\n");
            // Cisco pager: " --More-- " then backspaces / CR and spaces to wipe it
            log.feed(b" --More-- \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08          \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08enable secret 9 $9$abc\r\n");
            log.feed(&"caf\u{e9}".as_bytes()[..4]);
            log.feed(&"caf\u{e9}\r\n".as_bytes()[4..]);
            log.feed(b"sw1#");
        }
        let text = fs::read_to_string(&path).unwrap();
        let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(body, vec!["sw1#show run", "Building configuration...", "enable secret 9 <removed>", "caf\u{e9}", "sw1#"]);
        let _ = fs::remove_dir_all(dir);
    }
}
