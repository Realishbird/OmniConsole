//! Serial console sessions (USB console cables, RJ45 rollover adapters).

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serialport::{DataBits, FlowControl, Parity, StopBits};
use tokio::sync::mpsc;

use super::{ConnectSpec, Input, Io};

/// "9600 8N1", the way console settings are usually written
pub fn settings_label(spec: &ConnectSpec) -> String {
    let parity = match spec.parity.as_str() {
        "odd" => 'O',
        "even" => 'E',
        _ => 'N',
    };
    let flow = match spec.flow_control.as_str() {
        "software" => ", XON/XOFF",
        "hardware" => ", RTS/CTS",
        _ => "",
    };
    let baud = if spec.baud == 0 { 9600 } else { spec.baud };
    format!("{baud} {}{parity}{}{flow}", spec.data_bits, spec.stop_bits)
}

pub async fn run(spec: &ConnectSpec, io: &Io, mut input: mpsc::UnboundedReceiver<Input>) -> Result<()> {
    let baud = if spec.baud == 0 { 9600 } else { spec.baud };
    let data_bits = match spec.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    };
    let parity = match spec.parity.as_str() {
        "odd" => Parity::Odd,
        "even" => Parity::Even,
        _ => Parity::None,
    };
    let stop_bits = if spec.stop_bits == 2 { StopBits::Two } else { StopBits::One };
    let flow = match spec.flow_control.as_str() {
        "software" => FlowControl::Software,
        "hardware" => FlowControl::Hardware,
        _ => FlowControl::None,
    };
    let port = serialport::new(&spec.serial_port, baud)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("opening {}", spec.serial_port))?;
    let mut writer = port.try_clone().context("serial port")?;
    io.status("connected", format!("Serial {} {}", spec.serial_port, settings_label(spec)));

    // reading blocks, so it gets its own thread
    let stop = Arc::new(AtomicBool::new(false));
    let (gone_tx, mut gone_rx) = mpsc::unbounded_channel::<String>();
    let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut login = spec.auto_login.then(|| super::autologin::AutoLogin::new(&spec.username));
    let mut password = spec.password.clone().or_else(|| spec.device_id.as_deref().and_then(crate::store::password_get));
    let reader_io = io.clone();
    let reader_stop = stop.clone();
    let watch_login = login.is_some();
    let mut reader = port;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while !reader_stop.load(Ordering::Relaxed) {
            match reader.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    reader_io.output(&buf[..n]);
                    if watch_login {
                        let _ = seen_tx.send(buf[..n].to_vec());
                    }
                }
                Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    let _ = gone_tx.send(e.to_string());
                    break;
                }
            }
        }
    });

    let result = loop {
        tokio::select! {
            cmd = input.recv() => match cmd {
                Some(Input::Data(bytes)) => {
                    if let Err(e) = writer.write_all(&bytes) {
                        break Err(e).context("writing to the serial port");
                    }
                }
                Some(Input::Resize(..)) => {}
                Some(Input::Break) => {
                    // a 500 ms break, long enough for ROMMON on every Cisco platform
                    if writer.set_break().is_ok() {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let _ = writer.clear_break();
                        io.notice("Break sent.");
                    }
                }
                Some(Input::Close) | None => break Ok(()),
            },
            Some(err) = gone_rx.recv() => {
                break Err(anyhow::anyhow!("serial port lost ({err}), was the cable unplugged?"));
            }
            Some(data) = seen_rx.recv(), if login.is_some() => {
                if let Some(l) = login.as_mut() {
                    match l.feed(&data, password.as_deref()) {
                        super::autologin::Reply::Send(bytes) => { let _ = writer.write_all(&bytes); }
                        super::autologin::Reply::NeedPassword => {
                            if let Some(p) = io.ask("password", format!("Password for {}", spec.serial_port), None, false).await {
                                password = Some(p.clone());
                                let _ = writer.write_all(&l.password(&p));
                            }
                        }
                        super::autologin::Reply::None => {}
                    }
                }
            }
        }
    };
    stop.store(true, Ordering::Relaxed);
    result
}
