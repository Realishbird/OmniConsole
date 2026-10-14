//! SSH sessions (russh). Older Cisco gear only speaks SHA-1 key exchange,
//! ssh-rsa and CBC ciphers, so those are offered after the modern ones.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use russh::keys::{self, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{cipher, kex, mac, ChannelMsg, Disconnect, Preferred};
use tokio::io::{AsyncRead, AsyncWrite};
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

pub struct Client {
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

/// Who to log in as and how.
struct Login<'a> {
    host_port: &'a str,
    username: &'a str,
    device_id: Option<&'a str>,
    password: Option<String>,
    auth_method: &'a str,
    key_path: &'a str,
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

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Load a private key file, asking for its passphrase when it is encrypted.
async fn load_key(io: &Io, login: &Login<'_>) -> Result<Option<PrivateKey>> {
    let path = expand_home(login.key_path.trim());
    match keys::load_secret_key(&path, None) {
        Ok(k) => return Ok(Some(k)),
        Err(keys::Error::KeyIsEncrypted) => {}
        Err(e) => {
            io.notice(&format!("Could not read the key {}: {e}", path.display()));
            return Ok(None);
        }
    }
    if let Some(saved) = login.device_id.and_then(store::passphrase_get) {
        if let Ok(k) = keys::load_secret_key(&path, Some(&saved)) {
            return Ok(Some(k));
        }
        io.notice("The saved key passphrase did not work.");
    }
    for _ in 0..3 {
        let Some(pass) = io.ask("passphrase", format!("Passphrase for {}", path.display()), None, false).await else {
            return Ok(None);
        };
        match keys::load_secret_key(&path, Some(&pass)) {
            Ok(k) => return Ok(Some(k)),
            Err(_) => io.notice("Wrong passphrase."),
        }
    }
    Ok(None)
}

async fn try_key(session: &mut client::Handle<Client>, io: &Io, user: &str, login: &Login<'_>) -> Result<bool> {
    let Some(key) = load_key(io, login).await? else {
        return Ok(false);
    };
    let hash = session.best_supported_rsa_hash().await?.flatten();
    let ok = session
        .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await?
        .success();
    if !ok {
        io.notice("The key was not accepted.");
    }
    Ok(ok)
}

async fn agent_identities<S>(session: &mut client::Handle<Client>, user: &str, mut agent: AgentClient<S>) -> Result<bool>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let hash = session.best_supported_rsa_hash().await?.flatten();
    for id in agent.request_identities().await? {
        if let AgentIdentity::PublicKey { key, .. } = id {
            match session.authenticate_publickey_with(user, key, hash, &mut agent).await {
                Ok(r) if r.success() => return Ok(true),
                Ok(_) => {}
                Err(e) => log::debug!("agent signing failed: {e}"),
            }
        }
    }
    Ok(false)
}

/// ssh-agent on macOS/Linux, the OpenSSH agent or Pageant on Windows.
async fn try_agent(session: &mut client::Handle<Client>, io: &Io, user: &str) -> Result<bool> {
    #[cfg(unix)]
    let result = match AgentClient::connect_env().await {
        Ok(agent) => agent_identities(session, user, agent).await,
        Err(e) => Err(anyhow!("no SSH agent running ({e})")),
    };
    #[cfg(windows)]
    let result = match AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        Ok(agent) => agent_identities(session, user, agent).await,
        Err(_) => match AgentClient::connect_pageant().await {
            Ok(agent) => agent_identities(session, user, agent).await,
            Err(e) => Err(anyhow!("no SSH agent or Pageant running ({e})")),
        },
    };
    match result {
        Ok(true) => Ok(true),
        Ok(false) => {
            io.notice("None of the SSH agent's keys were accepted.");
            Ok(false)
        }
        Err(e) => {
            io.notice(&format!("{e:#}"));
            Ok(false)
        }
    }
}

/// Key or agent first when configured, then the saved or typed password,
/// then up to three password prompts. Returns the user name.
async fn authenticate(session: &mut client::Handle<Client>, io: &Io, login: &Login<'_>) -> Result<String> {
    let host_port = login.host_port;
    let user = if login.username.is_empty() {
        io.ask("username", format!("Username for {host_port}"), None, true)
            .await
            .ok_or_else(|| anyhow!("cancelled"))?
    } else {
        login.username.to_string()
    };

    match login.auth_method {
        "key" if !login.key_path.trim().is_empty() => {
            if try_key(session, io, &user, login).await? {
                return Ok(user);
            }
        }
        "agent" => {
            if try_agent(session, io, &user).await? {
                return Ok(user);
            }
        }
        _ => {}
    }

    let known = login.password.clone().or_else(|| login.device_id.and_then(store::password_get));
    if let Some(pw) = known {
        if try_password(session, io, &user, &pw).await? {
            return Ok(user);
        }
        io.notice("The saved password was not accepted.");
    }
    for _ in 0..3 {
        let Some(pw) = io.ask("password", format!("Password for {user}@{host_port}"), None, false).await else {
            bail!("login cancelled");
        };
        if try_password(session, io, &user, &pw).await? {
            return Ok(user);
        }
        io.notice("Access denied.");
    }
    bail!("access denied for {user}@{host_port}")
}

fn config(keepalive: u32) -> Arc<client::Config> {
    Arc::new(client::Config {
        preferred: preferred(),
        inactivity_timeout: None,
        keepalive_interval: (keepalive > 0).then(|| Duration::from_secs(keepalive as u64)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

fn timed_out(host_port: &str) -> anyhow::Error {
    anyhow!("no answer from {host_port} after {} seconds", CONNECT_TIMEOUT.as_secs())
}

/// An authenticated SSH session, plus the jump host session that carries
/// it when there is one (dropping that would close the tunnel).
pub struct Connected {
    pub session: client::Handle<Client>,
    pub jump: Option<client::Handle<Client>>,
    pub user: String,
    pub host_port: String,
}

/// Connect (through the jump host when set) and authenticate.
pub async fn connect(spec: &ConnectSpec, io: &Io) -> Result<Connected> {
    let port = if spec.port == 0 { 22 } else { spec.port };
    let host_port = format!("{}:{}", spec.host, port);
    let handler = Client { io: io.clone(), host_port: host_port.clone() };

    // through a jump host: log in there, open a direct-tcpip channel to the
    // target and run the target's SSH session inside it
    let mut jump_session = None;
    let mut session = if let Some(jump) = &spec.jump {
        let jport = if jump.port == 0 { 22 } else { jump.port };
        let jhost_port = format!("{}:{}", jump.host, jport);
        io.notice(&format!("Connecting through jump host {jhost_port}"));
        let jhandler = Client { io: io.clone(), host_port: jhost_port.clone() };
        let mut js = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config(spec.keepalive), (jump.host.as_str(), jport), jhandler))
            .await
            .map_err(|_| timed_out(&jhost_port))?
            .with_context(|| format!("connecting to jump host {jhost_port}"))?;
        authenticate(
            &mut js,
            io,
            &Login {
                host_port: &jhost_port,
                username: &jump.username,
                device_id: jump.device_id.as_deref(),
                password: None,
                auth_method: &jump.auth_method,
                key_path: &jump.key_path,
            },
        )
        .await
        .context("jump host login")?;
        let tunnel = js
            .channel_open_direct_tcpip(spec.host.as_str(), port as u32, "127.0.0.1", 0)
            .await
            .with_context(|| format!("jump host {jhost_port} could not reach {host_port}"))?;
        let s = tokio::time::timeout(CONNECT_TIMEOUT, client::connect_stream(config(spec.keepalive), tunnel.into_stream(), handler))
            .await
            .map_err(|_| timed_out(&host_port))?
            .with_context(|| format!("connecting to {host_port} through {jhost_port}"))?;
        jump_session = Some(js);
        s
    } else {
        tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config(spec.keepalive), (spec.host.as_str(), port), handler))
            .await
            .map_err(|_| timed_out(&host_port))?
            .with_context(|| format!("connecting to {host_port}"))?
    };

    let user = authenticate(
        &mut session,
        io,
        &Login {
            host_port: &host_port,
            username: &spec.username,
            device_id: spec.device_id.as_deref(),
            password: spec.password.clone(),
            auth_method: &spec.auth_method,
            key_path: &spec.key_path,
        },
    )
    .await?;
    Ok(Connected { session, jump: jump_session, user, host_port })
}

pub async fn run(spec: &ConnectSpec, io: &Io, mut input: mpsc::UnboundedReceiver<Input>) -> Result<()> {
    let Connected { session, jump: jump_session, user, host_port } = connect(spec, io).await?;

    let channel = session.channel_open_session().await?;
    channel
        .request_pty(false, "xterm-256color", spec.cols as u32, spec.rows as u32, 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;
    let via = spec.jump.as_ref().map(|j| format!(" via {}", j.host)).unwrap_or_default();
    io.status("connected", format!("SSH {user}@{host_port}{via}"));

    let (mut reader, writer) = channel.split();
    loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) => io.output(&data),
                Some(ChannelMsg::ExtendedData { data, .. }) => io.output(&data),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            cmd = input.recv() => match cmd {
                Some(Input::Data(bytes)) => writer.data(&bytes[..]).await?,
                Some(Input::Resize(cols, rows)) => {
                    let _ = writer.window_change(cols as u32, rows as u32, 0, 0).await;
                }
                Some(Input::Break) => io.notice("Break is only available on serial and Telnet sessions."),
                Some(Input::Close) | None => break,
            },
        }
    }
    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
    if let Some(js) = jump_session {
        let _ = js.disconnect(Disconnect::ByApplication, "", "en").await;
    }
    Ok(())
}
