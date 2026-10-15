//! Masks secrets in device output before it is written to a log, an export
//! or (later) sent to an AI provider: Cisco passwords and secrets, SNMP
//! communities and v3 keys, RADIUS/TACACS+ keys, pre-shared keys and PEM
//! private keys.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;

pub const MASK: &str = "<removed>";

fn rules() -> &'static [Regex] {
    static RULES: OnceLock<Vec<Regex>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            // enable secret 9 $9$..., username x secret 5 ..., password 7 0822..., NX-OS password 5 $5$...
            // the optional words name the hash type (`secret sha512 $6$`, `password hash`,
            // `plaintext`) or, on AOS-S, the account (`password manager user-name admin plaintext`)
            r"(?i)\b(?:password|secret)(?:\s+(?:\d{1,2}|sha512|sha256|sha1|md5|hash|plaintext|encrypted|cleartext|type\s*\d+|manager|operator|all|user-name\s+\S+))*\s+(?P<s>\S+)",
            r"(?i)\bsnmp-server\s+community\s+(?P<s>\S+)",
            r"(?i)\b(?:auth\s+(?:md5|sha\S*)|priv\s+(?:des\S*|3des|aes(?:\s+\d+)?))\s+(?P<s>\S+)",
            r"(?i)\b(?:key-string|wpa-psk\s+(?:ascii|hex)|pre-shared-key(?:\s+(?:local|remote|address\s+\S+(?:\s+\S+)?))?|(?:tacacs-server|radius-server|isakmp)\s+key|message-digest-key\s+\d+\s+md5|authentication-key\s+\d+\s+md5|authentication\s+text)(?:\s+\d)?\s+(?P<s>\S+)",
            // "key 7 xxxx" inside radius/tacacs server blocks; "key chain NAME" has a third word,
            // and "key 1" (a key chain entry number) is skipped below
            r"(?i)^\s*key\s+(?:\d\s+)?(?P<s>\S+)\s*$",
        ]
        .iter()
        .map(|r| Regex::new(r).expect("redaction rule"))
        .collect()
    })
}

/// Mask secrets in one line of text.
pub fn line(text: &str) -> Cow<'_, str> {
    let mut out: Cow<str> = Cow::Borrowed(text);
    for (i, re) in rules().iter().enumerate() {
        let key_rule = i == rules().len() - 1;
        if !re.is_match(&out) {
            continue;
        }
        let replaced = re
            .replace_all(&out, |c: &regex::Captures| {
                let all = c.get(0).unwrap();
                let s = c.name("s").unwrap();
                // already masked, or a bare number such as "key 1"
                if s.as_str() == MASK || (key_rule && s.as_str().bytes().all(|b| b.is_ascii_digit())) {
                    return all.as_str().to_string();
                }
                let start = s.start() - all.start();
                let end = s.end() - all.start();
                format!("{}{}{}", &all.as_str()[..start], MASK, &all.as_str()[end..])
            })
            .into_owned();
        out = Cow::Owned(replaced);
    }
    out
}

/// Line-by-line redactor that also hides PEM private key blocks.
#[derive(Default)]
pub struct Redactor {
    in_private_key: bool,
}

impl Redactor {
    pub fn line<'a>(&mut self, text: &'a str) -> Cow<'a, str> {
        if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
            self.in_private_key = true;
            return Cow::Borrowed(text);
        }
        if self.in_private_key {
            if text.contains("-----END") {
                self.in_private_key = false;
                return Cow::Borrowed(text);
            }
            return Cow::Borrowed(MASK);
        }
        line(text)
    }
}

/// Mask a whole multi-line text.
pub fn text(text: &str) -> String {
    let mut r = Redactor::default();
    text.split('\n').map(|l| r.line(l).into_owned()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_typed_secrets() {
        for (line, leaked) in [
            ("username admin secret sha512 $6$abc$def", "$6$abc$def"),
            ("username admin password hash $6$xyz", "$6$xyz"),
            ("password manager user-name admin plaintext Secret1", "Secret1"),
            ("username x password 5 $5$salt$hash", "$5$salt$hash"),
            ("enable secret 9 $9$abc", "$9$abc"),
        ] {
            let out = super::line(line);
            assert!(!out.contains(leaked), "{line} -> {out}");
            assert!(out.contains(MASK), "{line} -> {out}");
        }
    }

    #[test]
    fn cisco_secrets() {
        let cases = [
            ("enable secret 9 $9$abc$def", "enable secret 9 <removed>"),
            ("username admin privilege 15 secret 5 $1$xyz", "username admin privilege 15 secret 5 <removed>"),
            (" password 7 0822455D0A16", " password 7 <removed>"),
            ("username admin password 5 $5$Q  role network-admin", "username admin password 5 <removed>  role network-admin"),
            ("enable password 1234", "enable password <removed>"),
            ("snmp-server community s3cr3t RO 10", "snmp-server community <removed> RO 10"),
            ("snmp-server user mon grp v3 auth sha AuthPass priv aes 128 PrivPass", "snmp-server user mon grp v3 auth sha <removed> priv aes 128 <removed>"),
            ("tacacs-server key 7 1234ABCD", "tacacs-server key 7 <removed>"),
            (" key 7 070C285F4D06", " key 7 <removed>"),
            (" key-string MyKey", " key-string <removed>"),
            ("crypto isakmp key Sup3r address 1.2.3.4", "crypto isakmp key <removed> address 1.2.3.4"),
            (" wpa-psk ascii 0 wifipass", " wpa-psk ascii 0 <removed>"),
            (" ip ospf message-digest-key 1 md5 ospfkey", " ip ospf message-digest-key 1 md5 <removed>"),
        ];
        for (input, want) in cases {
            assert_eq!(line(input), want, "input: {input}");
        }
    }

    #[test]
    fn leaves_normal_lines_alone() {
        for l in [
            "service password-encryption",
            "key chain OSPF",
            " key 1",
            "Password:",
            "interface GigabitEthernet1/0/1",
            "crypto key generate rsa modulus 2048",
            "no password",
        ] {
            assert_eq!(line(l), l, "changed: {l}");
        }
    }

    #[test]
    fn private_key_blocks() {
        let t = "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----\nb";
        assert_eq!(text(t), "a\n-----BEGIN RSA PRIVATE KEY-----\n<removed>\n<removed>\n-----END RSA PRIVATE KEY-----\nb");
    }
}
