//! Telnet sessions: raw TCP with the option negotiation a terminal needs
//! (echo, suppress go-ahead, terminal type, window size). Everything else
//! is refused.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use super::{ConnectSpec, Input, Io};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Data,
    Iac,
    Command(u8),
    Sub,
    SubIac,
}

struct Telnet {
    state: State,
    sub: Vec<u8>,
    naws: bool,
    cols: u16,
    rows: u16,
}

impl Telnet {
    fn naws_bytes(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, OPT_NAWS];
        for b in [self.cols.to_be_bytes(), self.rows.to_be_bytes()].concat() {
            out.push(b);
            if b == IAC {
                out.push(IAC);
            }
        }
        out.extend([IAC, SE]);
        out
    }

    /// Split received bytes into terminal data and replies to send back.
    fn receive(&mut self, input: &[u8], data: &mut Vec<u8>, reply: &mut Vec<u8>) {
        for &b in input {
            self.state = match self.state {
                State::Data => {
                    if b == IAC {
                        State::Iac
                    } else {
                        data.push(b);
                        State::Data
                    }
                }
                State::Iac => match b {
                    IAC => {
                        data.push(IAC);
                        State::Data
                    }
                    DO | DONT | WILL | WONT => State::Command(b),
                    SB => {
                        self.sub.clear();
                        State::Sub
                    }
                    _ => State::Data,
                },
                State::Command(cmd) => {
                    match (cmd, b) {
                        (WILL, OPT_ECHO) | (WILL, OPT_SGA) => reply.extend([IAC, DO, b]),
                        (WILL, _) => reply.extend([IAC, DONT, b]),
                        (DO, OPT_TTYPE) | (DO, OPT_SGA) => reply.extend([IAC, WILL, b]),
                        (DO, OPT_NAWS) => {
                            reply.extend([IAC, WILL, b]);
                            self.naws = true;
                            reply.extend(self.naws_bytes());
                        }
                        (DO, _) => reply.extend([IAC, WONT, b]),
                        (DONT, OPT_NAWS) => self.naws = false,
                        _ => {}
                    }
                    State::Data
                }
                State::Sub => {
                    if b == IAC {
                        State::SubIac
                    } else {
                        self.sub.push(b);
                        State::Sub
                    }
                }
                State::SubIac => {
                    if b == SE {
                        // terminal type SEND
                        if self.sub.first() == Some(&OPT_TTYPE) && self.sub.get(1) == Some(&1) {
                            reply.extend([IAC, SB, OPT_TTYPE, 0]);
                            reply.extend(b"XTERM");
                            reply.extend([IAC, SE]);
                        }
                        State::Data
                    } else {
                        self.sub.push(b);
                        State::Sub
                    }
                }
            };
        }
    }
}

/// user input: IAC doubled, CR sent as CR NUL (RFC 854)
fn encode_input(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 4);
    for &b in bytes {
        match b {
            IAC => out.extend([IAC, IAC]),
            b'\r' => out.extend([b'\r', 0]),
            _ => out.push(b),
        }
    }
    out
}

pub async fn run(spec: &ConnectSpec, io: &Io, mut input: mpsc::UnboundedReceiver<Input>) -> Result<()> {
    let port = if spec.port == 0 { 23 } else { spec.port };
    let host_port = format!("{}:{}", spec.host, port);
    let mut stream = tokio::time::timeout(Duration::from_secs(15), TcpStream::connect((spec.host.as_str(), port)))
        .await
        .map_err(|_| anyhow!("no answer from {host_port}"))?
        .with_context(|| format!("connecting to {host_port}"))?;
    stream.set_nodelay(true).ok();
    io.status("connected", format!("Telnet {host_port}"));

    let mut t = Telnet { state: State::Data, sub: Vec::new(), naws: false, cols: spec.cols, rows: spec.rows };
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            n = stream.read(&mut buf) => {
                let n = n?;
                if n == 0 {
                    break;
                }
                let mut data = Vec::with_capacity(n);
                let mut reply = Vec::new();
                t.receive(&buf[..n], &mut data, &mut reply);
                if !data.is_empty() {
                    io.output(&data);
                }
                if !reply.is_empty() {
                    stream.write_all(&reply).await?;
                }
            }
            cmd = input.recv() => match cmd {
                Some(Input::Data(bytes)) => stream.write_all(&encode_input(&bytes)).await?,
                Some(Input::Resize(cols, rows)) => {
                    t.cols = cols;
                    t.rows = rows;
                    if t.naws {
                        stream.write_all(&t.naws_bytes()).await?;
                    }
                }
                Some(Input::Break) => stream.write_all(&[IAC, 243]).await?, // IAC BRK
                Some(Input::Close) | None => break,
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_and_data() {
        let mut t = Telnet { state: State::Data, sub: Vec::new(), naws: false, cols: 80, rows: 24 };
        let mut data = Vec::new();
        let mut reply = Vec::new();
        t.receive(&[b'h', IAC, WILL, OPT_ECHO, b'i', IAC, IAC, IAC, DO, OPT_NAWS], &mut data, &mut reply);
        assert_eq!(data, vec![b'h', b'i', IAC]);
        assert_eq!(&reply[..3], &[IAC, DO, OPT_ECHO]);
        assert!(t.naws);
        assert_eq!(&reply[3..6], &[IAC, WILL, OPT_NAWS]);
        assert_eq!(&reply[6..], &[IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE]);
    }

    #[test]
    fn terminal_type() {
        let mut t = Telnet { state: State::Data, sub: Vec::new(), naws: false, cols: 80, rows: 24 };
        let (mut data, mut reply) = (Vec::new(), Vec::new());
        t.receive(&[IAC, SB, OPT_TTYPE, 1, IAC, SE], &mut data, &mut reply);
        assert!(data.is_empty());
        assert_eq!(reply, [&[IAC, SB, OPT_TTYPE, 0][..], b"XTERM", &[IAC, SE]].concat());
    }

    #[test]
    fn input_escaping() {
        assert_eq!(encode_input(&[b'a', b'\r', IAC]), vec![b'a', b'\r', 0, IAC, IAC]);
    }
}
