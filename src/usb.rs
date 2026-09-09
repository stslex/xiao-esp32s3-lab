use crate::{
    camera, network,
    protocol::{self, Command, LineReader},
    SharedStatus,
};
use anyhow::{bail, Result};
use esp_idf_svc::hal::delay::FreeRtos;
use std::{
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

pub fn start(
    status: SharedStatus,
    network: SyncSender<network::Request>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("usb-camera".into())
        .stack_size(12_288)
        .spawn(move || {
            let mut reader = LineReader::default();
            let mut buffer = [0u8; 64];
            loop {
                let count = unsafe {
                    esp_idf_svc::sys::camera::xiao_usb_read(buffer.as_mut_ptr(), buffer.len())
                };
                for &byte in &buffer[..count.max(0) as usize] {
                    if let Some(line) = reader.push(byte) {
                        if let Ok((id, command)) = protocol::parse(&line) {
                            if let Err(error) = handle(id, command, &status, &network) {
                                let _ = send(id, &format!("ERR {error}"));
                            }
                        }
                    }
                }
                // ESP-IDF usleep busy-waits below one scheduler tick. Block the
                // task explicitly so idle tasks can feed the watchdog.
                FreeRtos::delay_ms(10);
            }
        })
}

fn send(id: u32, body: &str) -> Result<()> {
    // Start with a newline to recover from any partial boot/log line.
    let line = format!("\n{} {id} {body}\n", protocol::PREFIX);
    let written = unsafe { esp_idf_svc::sys::camera::xiao_usb_write(line.as_ptr(), line.len()) };
    if written != line.len() as i32 {
        bail!("usb_write_failed");
    }
    Ok(())
}

fn handle(
    id: u32,
    command: Command,
    status: &SharedStatus,
    network: &SyncSender<network::Request>,
) -> Result<()> {
    match command {
        Command::Status => {
            let body = status.lock().unwrap().json().to_string();
            send(id, &format!("STATUS {body}"))?;
        }
        Command::Capture => {
            let frame = camera::snapshot()?;
            if frame.len() > protocol::MAX_FRAME {
                bail!("frame_exceeds_usb_limit");
            }
            send(
                id,
                &format!("BEGIN {} {:08x}", frame.len(), protocol::crc32(&frame)),
            )?;
            for (index, chunk) in frame.chunks(protocol::CHUNK_SIZE).enumerate() {
                send(
                    id,
                    &format!(
                        "DATA {} {}",
                        index * protocol::CHUNK_SIZE,
                        protocol::encode_hex(chunk)
                    ),
                )?;
                if index % 8 == 7 {
                    FreeRtos::delay_ms(10);
                }
            }
            send(id, "END")?;
        }
        Command::WifiSet(credentials) => {
            change_wifi(id, network::Change::Set(credentials), network)?
        }
        Command::WifiForget => change_wifi(id, network::Change::Forget, network)?,
        Command::GithubSet(token) => {
            let github = status.lock().unwrap().github.clone();
            github.lock().unwrap().configure(Some(token))?;
            send(id, "OK")?;
        }
        Command::GithubForget => {
            let github = status.lock().unwrap().github.clone();
            github.lock().unwrap().configure(None)?;
            send(id, "OK")?;
        }
        Command::DisplayTest | Command::DisplayGithub => {
            let (display, github) = {
                let state = status.lock().unwrap();
                (state.display.clone(), state.github.clone())
            };
            let (frame, mode) = if matches!(command, Command::DisplayTest) {
                (crate::framebuffer::Framebuffer::diagnostic().0, "test")
            } else {
                (github.lock().unwrap().frame(), "github")
            };
            display.lock().unwrap().show(frame, mode)?;
            send(id, "OK")?;
        }
    }
    Ok(())
}

fn change_wifi(
    id: u32,
    change: network::Change,
    network: &SyncSender<network::Request>,
) -> Result<()> {
    let (reply, receiver) = mpsc::sync_channel(1);
    network
        .try_send(network::Request { change, reply })
        .map_err(|_| anyhow::anyhow!("wifi_busy"))?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| anyhow::anyhow!("wifi_update_timeout_check_status"))?
        .map_err(anyhow::Error::msg)?;
    send(id, "OK")
}
