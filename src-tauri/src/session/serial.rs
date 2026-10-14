//! Serial console sessions (USB console cables, RJ45 rollover adapters).

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use super::{ConnectSpec, Input, Io};

pub async fn run(spec: &ConnectSpec, io: &Io, mut input: mpsc::UnboundedReceiver<Input>) -> Result<()> {
    let baud = if spec.baud == 0 { 9600 } else { spec.baud };
    let port = serialport::new(&spec.serial_port, baud)
        .timeout(Duration::from_millis(100))
        .open()
        .with_context(|| format!("opening {}", spec.serial_port))?;
    let mut writer = port.try_clone().context("serial port")?;
    io.status("connected", format!("Serial {} {} baud", spec.serial_port, baud));

    // reading blocks, so it gets its own thread
    let stop = Arc::new(AtomicBool::new(false));
    let (gone_tx, mut gone_rx) = mpsc::unbounded_channel::<String>();
    let reader_io = io.clone();
    let reader_stop = stop.clone();
    let mut reader = port;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while !reader_stop.load(Ordering::Relaxed) {
            match reader.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => reader_io.output(&buf[..n]),
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
                Some(Input::Close) | None => break Ok(()),
            },
            Some(err) = gone_rx.recv() => {
                break Err(anyhow::anyhow!("serial port lost ({err}), was the cable unplugged?"));
            }
        }
    };
    stop.store(true, Ordering::Relaxed);
    result
}
