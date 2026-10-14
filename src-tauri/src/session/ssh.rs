//! SSH sessions (russh). Older Cisco gear only speaks SHA-1 key exchange,
//! ssh-rsa and CBC ciphers, so those are offered after the modern ones.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PublicKeyOrCertificate};
use russh::{cipher, kex, mac, ChannelMsg, Disconnect, Preferred};
use tokio::sync::mpsc;

use super::{ConnectSpec, Input, Io};
use crate::store::{self, HostKeyCheck};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

fn preferred() -> Preferred {
    let mut kex_list: Vec<kex::Name> = Preferred::DEFAULT.kex.to_vec();
    // legacy key exchange goes before the extension markers at the end
    let at = kex_list.iter().position(|k| *k == kex::EXTENSION_SUPPORT_AS_CLIENT).unwrap_or(kex_list.len());
    for (i, k) in [
        kex::ECDH_SHA2_NISTP256,
        kex::ECDH_SHA2_NISTP384,
        kex::ECDH_SHA2_NISTP521,
        kex::DH_GEX_SHA1,
        kex::DH_G14_SHA1,
        kex::DH_G1_SHA1,
    ]
    .into_iter()
    .enumerate()
    {
        kex_list.insert(at + i, k);
    }
    let mut ciphers: Vec<cipher::Name> = Preferred::DEFAULT.cipher.to_vec();
    ciphers.extend([cipher::AES_256_CBC, cipher::AES_192_CBC, cipher::AES_128_CBC, cipher::TRIPLE_DES_CBC]);
    let mut macs: Vec<mac::Name> = Preferred::DEFAULT.mac.to_vec();
    macs.extend([mac::HMAC_SHA1_ETM, mac::HMAC_SHA1]);
    Preferred {
        kex: Cow::Owned(kex_list),
        cipher: Cow::Owned(ciphers),
        mac: Cow::Owned(macs),
        ..Preferred::DEFAULT
    }
}

struct Client {
    io: Io,
    host_port: String,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn auth_banner(&mut self, banner: &str, _session: &mut client::Session) -> Result<(), Self::Error> {
        self.io.output(banner.replace('\n', "\r\n").as_bytes());
        Ok(())
    }

    /// Trust on first use, like OpenSSH: a new key is shown to the user,
    /// a changed key is a loud warning.
    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key().clone().into(),
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let mut stored = key.clone();
        stored.set_comment("");
        let Ok(openssh) = stored.to_openssh() else {
            return Ok(false);
        };
        match store::check_host_key(&self.io.config_dir, &self.host_port, &openssh) {
            HostKeyCheck::Known => Ok(true),
            HostKeyCheck::New => {
                let msg = format!("First connection to {}. Check the host key fingerprint before trusting it.", self.host_port);
                let ok = self.io.ask("hostKeyNew", msg, Some(fingerprint), false).await.is_some();
                if ok {
                    let _ = store::remember_host_key(&self.io.config_dir, &self.host_port, &openssh);
                }
                Ok(ok)
            }
            HostKeyCheck::Changed => {
                let msg = format!(
                    "The host key of {} has CHANGED. This happens after a device is replaced or re-keyed, \
                     but it can also mean someone is intercepting the connection.",
                    self.host_port
                );
                let ok = self.io.ask("hostKeyChanged", msg, Some(fingerprint), false).await.is_some();
                if ok {
                    let _ = store::remember_host_key(&self.io.config_dir, &self.host_port, &openssh);
                }
                Ok(ok)
            }
        }
    }
}

/// keyboard-interactive: hidden prompts get the password, others are asked
async fn keyboard_interactive(
    session: &mut client::Handle<Client>,
    io: &Io,
    user: &str,
    password: &str,
) -> Result<bool> {
    let mut reply = session.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..8 {
        match reply {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let mut answers = Vec::with_capacity(prompts.len());
                for p in prompts {
                    if !p.echo {
                        answers.push(password.to_string());
                    } else {
                        match io.ask("keyboard", p.prompt.clone(), None, true).await {
                            Some(a) => answers.push(a),
                            None => return Ok(false),
                        }
                    }
                }
                reply = session.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

async fn try_password(session: &mut client::Handle<Client>, io: &Io, user: &str, password: &str) -> Result<bool> {
    if session.authenticate_password(user, password).await?.success() {
        return Ok(true);
    }
    // many network devices only offer keyboard-interactive
    keyboard_interactive(session, io, user, password).await
}

pub async fn run(spec: &ConnectSpec, io: &Io, mut input: mpsc::UnboundedReceiver<Input>) -> Result<()> {
    let port = if spec.port == 0 { 22 } else { spec.port };
    let host_port = format!("{}:{}", spec.host, port);
    let config = Arc::new(client::Config {
        preferred: preferred(),
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });
    let handler = Client { io: io.clone(), host_port: host_port.clone() };
    let mut session = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, (spec.host.as_str(), port), handler))
        .await
        .map_err(|_| anyhow!("no answer from {host_port} after {} seconds", CONNECT_TIMEOUT.as_secs()))?
        .with_context(|| format!("connecting to {host_port}"))?;

    // user name
    let user = if spec.username.is_empty() {
        io.ask("username", format!("Username for {host_port}"), None, true)
            .await
            .ok_or_else(|| anyhow!("cancelled"))?
    } else {
        spec.username.clone()
    };

    // saved or typed password first, then ask (three tries)
    let known = spec
        .password
        .clone()
        .or_else(|| spec.device_id.as_deref().and_then(store::password_get));
    let mut authed = false;
    if let Some(pw) = known {
        authed = try_password(&mut session, io, &user, &pw).await?;
        if !authed {
            io.notice("The saved password was not accepted.");
        }
    }
    let mut tries = 0;
    while !authed && tries < 3 {
        tries += 1;
        let Some(pw) = io.ask("password", format!("Password for {user}@{host_port}"), None, false).await else {
            bail!("login cancelled");
        };
        authed = try_password(&mut session, io, &user, &pw).await?;
        if !authed {
            io.notice("Access denied.");
        }
    }
    if !authed {
        bail!("access denied for {user}");
    }

    let channel = session.channel_open_session().await?;
    channel
        .request_pty(false, "xterm-256color", spec.cols as u32, spec.rows as u32, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;
    io.status("connected", format!("SSH {user}@{host_port}"));

    let (mut reader, writer) = channel.split();
    loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) => io.output(&data),
                Some(ChannelMsg::ExtendedData { data, .. }) => io.output(&data),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(ChannelMsg::ExitStatus { .. }) => {}
                Some(_) => {}
            },
            cmd = input.recv() => match cmd {
                Some(Input::Data(bytes)) => writer.data(&bytes[..]).await?,
                Some(Input::Resize(cols, rows)) => {
                    let _ = writer.window_change(cols as u32, rows as u32, 0, 0).await;
                }
                Some(Input::Close) | None => break,
            },
        }
    }
    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
    Ok(())
}
