//! Auto-login for Telnet and serial consoles: answer the `Username:` and
//! `Password:` questions once each. SSH authenticates on its own.

use regex::Regex;

pub struct AutoLogin {
    username: String,
    tail: String,
    sent_user: bool,
    sent_pass: bool,
    /// the device asked for a password and we have none saved
    pub wants_password: bool,
}

pub enum Reply {
    Send(Vec<u8>),
    /// ask the user for the password, then call `password()`
    NeedPassword,
    None,
}

fn user_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(user ?name|login|user):\s*$").unwrap())
}

fn pass_regex() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)pass(word|code|phrase)?:\s*$").unwrap())
}

impl AutoLogin {
    pub fn new(username: &str) -> Self {
        AutoLogin { username: username.to_string(), tail: String::new(), sent_user: false, sent_pass: false, wants_password: false }
    }

    /// Feed device output; says what to send back, if anything.
    pub fn feed(&mut self, bytes: &[u8], password: Option<&str>) -> Reply {
        self.tail.push_str(&String::from_utf8_lossy(bytes));
        if self.tail.len() > 400 {
            let cut = self.tail.len() - 400;
            let cut = (cut..self.tail.len()).find(|i| self.tail.is_char_boundary(*i)).unwrap_or(cut);
            self.tail.drain(..cut);
        }
        let line = self.tail.rsplit(['\n', '\r']).next().unwrap_or("").trim_end();
        if self.sent_pass && (self.tail.contains("% Login invalid") || self.tail.contains("Authentication failed") || self.tail.contains("% Bad passwords")) {
            // leave the next attempt to the user
            self.tail.clear();
            return Reply::None;
        }
        if !self.sent_user && !self.username.is_empty() && user_regex().is_match(line) {
            self.sent_user = true;
            self.tail.clear();
            return Reply::Send(format!("{}\r", self.username).into_bytes());
        }
        if !self.sent_pass && pass_regex().is_match(line) {
            match password {
                Some(p) => {
                    self.sent_pass = true;
                    self.tail.clear();
                    Reply::Send(format!("{p}\r").into_bytes())
                }
                None => {
                    self.wants_password = true;
                    self.tail.clear();
                    Reply::NeedPassword
                }
            }
        } else {
            Reply::None
        }
    }

    /// The user supplied a password after `NeedPassword`.
    pub fn password(&mut self, p: &str) -> Vec<u8> {
        self.sent_pass = true;
        self.wants_password = false;
        format!("{p}\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_once() {
        let mut a = AutoLogin::new("admin");
        assert!(matches!(a.feed(b"\r\nUser Access Verification\r\n\r\nUsername: ", Some("pw")), Reply::Send(v) if v == b"admin\r"));
        assert!(matches!(a.feed(b"admin\r\nPassword: ", Some("pw")), Reply::Send(v) if v == b"pw\r"));
        assert!(matches!(a.feed(b"\r\nsw1>", Some("pw")), Reply::None));
        // a second password prompt is left to the user
        assert!(matches!(a.feed(b"\r\nPassword: ", Some("pw")), Reply::None));
        let mut b = AutoLogin::new("");
        assert!(matches!(b.feed(b"Password: ", None), Reply::NeedPassword));
        assert_eq!(b.password("x"), b"x\r");
    }
}
