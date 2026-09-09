mod album;
mod album_data;
mod album_playback;
mod camera;
mod camera_config;
mod display;
mod display_config;
mod framebuffer;
mod github;
mod github_data;
mod image_layout;
mod network;
mod photo_flash;
mod photo_record;
mod protocol;
mod usb;

use anyhow::{Context, Result};
use embedded_svc::{http::Method, io::Write};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::peripherals::Peripherals,
    http::server::{Configuration as HttpConfiguration, EspHttpServer},
    nvs::{EspDefaultNvs, EspDefaultNvsPartition},
    wifi::EspWifi,
};
use serde_json::{json, Value};
use std::{
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

const INDEX_HTML: &str = include_str!("index.html");
type SharedStatus = Arc<Mutex<Status>>;

struct Status {
    started: Instant,
    http_ready: bool,
    http_error: Option<String>,
    wifi: Value,
    display: display::SharedDisplay,
    github: github::SharedGithub,
    album: album::SharedAlbum,
}

impl Status {
    fn json(&self) -> Value {
        let camera = camera::status();
        let display = self.display.lock().unwrap().json();
        let github = self.github.lock().unwrap().json();
        let album = self.album.lock().unwrap().json();
        json!({
            "firmware": env!("CARGO_PKG_VERSION"), "protocol": protocol::PREFIX,
            "uptime_seconds": self.started.elapsed().as_secs(),
            "camera_ready": camera["ready"], "camera_error": camera["error"],
            "http_ready": self.http_ready, "http_error": self.http_error,
            "frame_size": camera["settings"]["framesize"], "camera": camera, "wifi": self.wifi,
            "display": display, "github": github, "album": album,
        })
    }
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    if let Err(error) = camera::initialize() {
        log::warn!("Camera startup: {error}");
    }
    let partition = EspDefaultNvsPartition::take().ok();
    let display_store = partition
        .clone()
        .and_then(|p| EspDefaultNvs::new(p, "display", true).ok());
    let display = display::Display::initialize(display_store);
    if let Some(partition) = partition.clone() {
        match EspDefaultNvs::new(partition, "cam_controls", true) {
            Ok(store) => {
                if let Err(error) = camera::attach_store(store) {
                    log::warn!("Camera settings: {error}");
                }
            }
            Err(error) => log::warn!("Camera settings storage: {error}"),
        }
    }
    let github_store = partition
        .clone()
        .and_then(|partition| EspDefaultNvs::new(partition, "github", true).ok());
    let github = github::Github::new(github_store);
    let album_store = partition
        .clone()
        .and_then(|p| EspDefaultNvs::new(p, "album", true).ok());
    let album = album::Album::new(album_store);
    if display.lock().unwrap().start_github() && github.lock().unwrap().json()["configured"] == true
    {
        let frame = github.lock().unwrap().frame();
        let _ = display.lock().unwrap().show(frame, "github");
    }
    let status = Arc::new(Mutex::new(Status {
        started: Instant::now(),
        http_ready: false,
        http_error: None,
        wifi: json!({"mode": "station", "configured": false, "connected": false, "ip": null}),
        display: display.clone(),
        github: github.clone(),
        album: album.clone(),
    }));
    let (sender, requests) = mpsc::sync_channel(1);
    let _usb = usb::start(status.clone(), sender)?;
    let mut network = match create_network(status.clone(), partition) {
        Ok(network) => Some(network),
        Err(error) => {
            status.lock().unwrap().wifi["last_error"] =
                json!(format!("Wi-Fi unavailable: {error}"));
            None
        }
    };
    // SNTP provides wall-clock time for TLS certificate validity checks.
    let _sntp = esp_idf_svc::sntp::EspSntp::new_default()
        .map_err(|error| {
            log::warn!("Clock synchronization unavailable: {error}");
            error
        })
        .ok();
    let _github = github::start(status.clone(), github, display.clone())?;
    let _album = album::start(status.clone(), album, display)?;
    // HTTP startup failure must not take USB diagnostics down with it.
    let _server = match start_http_server(status.clone()) {
        Ok(server) => {
            status.lock().unwrap().http_ready = true;
            Some(server)
        }
        Err(error) => {
            status.lock().unwrap().http_error = Some(error.to_string());
            None
        }
    };
    log::info!("USB camera service ready. Configure home Wi-Fi using tools/camera.py wifi-setup.");

    loop {
        if let Ok(request) = requests.recv_timeout(Duration::from_millis(500)) {
            if let Some(network) = network.as_mut() {
                network.handle(request);
            } else {
                let _ = request.reply.send(Err("wifi_unavailable"));
            }
        }
        if let Some(network) = network.as_mut() {
            network.poll();
        }
        camera::poll_idle();
    }
}

fn create_network(
    status: SharedStatus,
    partition: Option<EspDefaultNvsPartition>,
) -> Result<network::Network> {
    let peripherals = Peripherals::take().context("peripherals already taken")?;
    let event_loop = EspSystemEventLoop::take()?;
    let partition = partition.context("NVS unavailable")?;
    let store = EspDefaultNvs::new(partition, "camera", true)?;
    // Keep driver settings in RAM; the application owns the persisted credential blob.
    let wifi = EspWifi::new(peripherals.modem, event_loop, None)?;
    esp_idf_svc::sys::esp!(unsafe {
        esp_idf_svc::sys::esp_wifi_set_storage(esp_idf_svc::sys::wifi_storage_t_WIFI_STORAGE_RAM)
    })?;
    Ok(network::Network::new(wifi, store, status))
}

fn start_http_server(status: SharedStatus) -> Result<EspHttpServer<'static>> {
    let mut server = EspHttpServer::new(&HttpConfiguration {
        stack_size: 12_288,
        session_timeout: Duration::from_secs(10),
        ..Default::default()
    })?;
    server.fn_handler("/", Method::Get, |request| {
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(INDEX_HTML.as_bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;
    server.fn_handler("/capture", Method::Get, move |request| {
        // Old pages used an unbounded ?t= polling loop. Require an explicit live
        // preview session for query-based requests; plain /capture stays a snapshot.
        let preview = request.uri().split_once('?').map(|(_, query)| {
            query
                .split('&')
                .find_map(|part| {
                    part.strip_prefix("preview=")
                        .and_then(|v| v.parse::<u64>().ok())
                })
                .unwrap_or(0)
        });
        let frame = match preview {
            Some(session) => camera::preview_snapshot(session),
            None => camera::snapshot(),
        };
        match frame {
            Ok(frame) => {
                let length = frame.len().to_string();
                request
                    .into_response(
                        200,
                        Some("OK"),
                        &[
                            ("Content-Type", "image/jpeg"),
                            ("Content-Length", &length),
                            ("Content-Disposition", "inline; filename=capture.jpg"),
                            ("Cache-Control", "no-store"),
                        ],
                    )?
                    .write_all(&frame)?;
            }
            Err(error) => {
                request
                    .into_status_response(if error.to_string() == "camera_preview_paused" {
                        409
                    } else {
                        503
                    })?
                    .write_all(error.to_string().as_bytes())?;
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    server.fn_handler("/camera/preview/start", Method::Post, |request| {
        let session = camera::start_preview();
        request
            .into_response(200, Some("OK"), &[("Content-Type", "application/json")])?
            .write_all(json!({"session":session}).to_string().as_bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;
    server.fn_handler("/camera/preview/stop", Method::Post, |request| {
        camera::pause_preview();
        request
            .into_ok_response()?
            .write_all(b"Camera preview paused")?;
        Ok::<(), anyhow::Error>(())
    })?;
    server.fn_handler("/camera/settings", Method::Get, |request| {
        let body = camera::status().to_string();
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(body.as_bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;
    server.fn_handler("/camera/settings", Method::Post, |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok());
        let Some(length) = length.filter(|length| (1..=512).contains(length)) else {
            request
                .into_response(400, Some("Bad Request"), &[("Connection", "close")])?
                .write_all(b"Expected a JSON object up to 512 bytes")?;
            return Ok::<(), anyhow::Error>(());
        };
        let mut bytes = vec![0; length];
        let mut offset = 0;
        while offset < length {
            let count = request.read(&mut bytes[offset..])?;
            if count == 0 {
                anyhow::bail!("truncated_camera_settings");
            }
            offset += count;
        }
        let parsed = serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| "invalid_json")
            .and_then(|value| camera::settings().patch(&value));
        match parsed {
            Err(error) => {
                request
                    .into_status_response(400)?
                    .write_all(error.as_bytes())?;
            }
            Ok(next) => match camera::configure(next) {
                Ok(value) => {
                    request
                        .into_response(200, Some("OK"), &[("Content-Type", "application/json")])?
                        .write_all(value.to_string().as_bytes())?;
                }
                Err(error) => {
                    request
                        .into_status_response(503)?
                        .write_all(error.to_string().as_bytes())?;
                }
            },
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let display = status.lock().unwrap().display.clone();
    let github = status.lock().unwrap().github.clone();
    let album = status.lock().unwrap().album.clone();
    let layout_display = display.clone();
    server.fn_handler("/display/settings", Method::Post, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|s| s.parse::<usize>().ok());
        let Some(length) = length.filter(|n| (1..=256).contains(n)) else {
            request
                .into_response(400, Some("Bad Request"), &[("Connection", "close")])?
                .write_all(b"Expected display settings JSON up to 256 bytes")?;
            return Ok::<(), anyhow::Error>(());
        };
        let mut body = vec![0u8; length];
        let mut offset = 0;
        while offset < length {
            let n = request.read(&mut body[offset..])?;
            if n == 0 {
                anyhow::bail!("truncated_display_settings");
            }
            offset += n;
        }
        let value = serde_json::from_slice::<Value>(&body);
        match value {
            Err(_) => request
                .into_status_response(400)?
                .write_all(b"invalid_json")?,
            Ok(value) => {
                // Validate the full patch before touching the display or NVS.
                match image_layout::Layout::default().patch(&value) {
                    Err(error) => request
                        .into_status_response(400)?
                        .write_all(error.as_bytes())?,
                    Ok(_) => match layout_display.lock().unwrap().configure_layout(&value) {
                        Ok(settings) => request
                            .into_response(
                                200,
                                Some("OK"),
                                &[("Content-Type", "application/json")],
                            )?
                            .write_all(settings.to_string().as_bytes())?,
                        Err(error) => request
                            .into_status_response(503)?
                            .write_all(error.to_string().as_bytes())?,
                    },
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let album_config = album.clone();
    let album_display = display.clone();
    server.fn_handler("/album/settings", Method::Post, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|v| v.parse::<usize>().ok());
        let Some(length) = length.filter(|v| (1..=1536).contains(v)) else {
            request
                .into_response(400, Some("Bad Request"), &[("Connection", "close")])?
                .write_all(b"Expected album settings JSON up to 1536 bytes")?;
            return Ok::<(), anyhow::Error>(());
        };
        let mut body = vec![0u8; length];
        let mut offset = 0;
        while offset < length {
            let count = request.read(&mut body[offset..])?;
            if count == 0 {
                anyhow::bail!("truncated_album_settings");
            }
            offset += count;
        }
        let result = serde_json::from_slice::<Value>(&body)
            .map_err(anyhow::Error::from)
            .and_then(|value| {
                let mut album = album_config.lock().unwrap();
                let clock = album_display.lock().unwrap().album_clock();
                album.configure(&value, clock)
            });
        match result {
            Ok(()) => {
                let mut panel = album_display.lock().unwrap();
                panel.select_album()?;
                request
                    .into_ok_response()?
                    .write_all(b"Album saved; loading photos")?;
            }
            Err(error) => request
                .into_status_response(400)?
                .write_all(error.to_string().as_bytes())?,
        }
        Ok::<(), anyhow::Error>(())
    })?;
    for (path, direction) in [
        ("/album/next", 1),
        ("/album/previous", -1),
        ("/album/pause", 0),
        ("/album/resume", 2),
    ] {
        let control_album = album.clone();
        let control_display = display.clone();
        server.fn_handler(path, Method::Post, move |request| {
            let mut state = control_album.lock().unwrap();
            let panel = control_display.lock().unwrap();
            let result = if panel.mode() != "album" {
                Err(anyhow::anyhow!("select_album_mode_first"))
            } else if direction == 0 || direction == 2 {
                state.pause(direction == 0, panel.album_clock());
                Ok(())
            } else {
                state.step(direction, panel.album_clock())
            };
            match result {
                Ok(()) => request
                    .into_ok_response()?
                    .write_all(b"Album playback updated")?,
                Err(error) => request
                    .into_status_response(409)?
                    .write_all(error.to_string().as_bytes())?,
            }
            Ok::<(), anyhow::Error>(())
        })?;
    }
    let select_album = album.clone();
    let select_display = display.clone();
    server.fn_handler("/display/album", Method::Post, move |request| {
        if select_album.lock().unwrap().json()["configured"] != true {
            request
                .into_status_response(400)?
                .write_all(b"Configure an album first")?;
        } else {
            let mut panel = select_display.lock().unwrap();
            panel.select_album()?;
            request
                .into_ok_response()?
                .write_all(b"Album mode selected")?;
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let image_display = display.clone();
    server.fn_handler("/display/image", Method::Post, move |mut request| {
        let length = request
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok());
        let dimensions = match (request.header("X-Image-Width"),request.header("X-Image-Height")) {
            (None,None)=>Some((240usize,320usize,true)),
            (Some(w),Some(h))=>w.parse::<usize>().ok().zip(h.parse::<usize>().ok()).map(|(w,h)|(w,h,false)),
            _=>None,
        };
        let dimensions=dimensions.filter(|(w,h,_)| *w>0 && *h>0 && *w<=640 && *h<=640 && w*h*2<=framebuffer::BYTE_LEN && length==Some(w*h*2));
        let Some((width,height,legacy_padding))=dimensions else {
            request
                .into_response(400, Some("Bad Request"), &[("Connection", "close")])?
                .write_all(b"Expected RGB565 pixels matching X-Image-Width/Height; at most 76800 pixels, sides up to 640")?;
            return Ok::<(), anyhow::Error>(());
        };
        let mut frame = vec![0u8; width*height*2];
        let mut offset = 0;
        while offset < frame.len() {
            let count = request.read(&mut frame[offset..])?;
            if count == 0 {
                anyhow::bail!("truncated_image_body");
            }
            offset += count;
        }
        let source=image_layout::Image {pixels:frame,width,height,legacy_padding};
        let result = image_display.lock().unwrap().show_image(source);
        match result {
            Ok(()) => {
                request
                    .into_ok_response()?
                    .write_all(b"Image saved and sent to display")?;
            }
            Err(error) => {
                request
                    .into_status_response(503)?
                    .write_all(error.to_string().as_bytes())?;
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let saved_display = display.clone();
    server.fn_handler("/display/saved", Method::Get, move |request| {
        let saved = saved_display.lock().unwrap().saved_photo();
        if let Some(image) = saved {
            let width = image.width.to_string();
            let height = image.height.to_string();
            request
                .into_response(
                    200,
                    Some("OK"),
                    &[
                        ("Content-Type", "application/octet-stream"),
                        ("Cache-Control", "no-store"),
                        ("X-Image-Width", &width),
                        ("X-Image-Height", &height),
                    ],
                )?
                .write_all(&image.pixels)?;
        } else {
            request
                .into_status_response(404)?
                .write_all(b"No saved photo")?;
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let restore_display = display.clone();
    server.fn_handler("/display/restore", Method::Post, move |request| {
        match restore_display.lock().unwrap().restore_photo() {
            Ok(()) => request
                .into_ok_response()?
                .write_all(b"Saved photo restored")?,
            Err(error) => request
                .into_status_response(503)?
                .write_all(error.to_string().as_bytes())?,
        }
        Ok::<(), anyhow::Error>(())
    })?;
    let preview_display = display.clone();
    server.fn_handler("/display/frame", Method::Get, move |request| {
        let frame = preview_display.lock().unwrap().frame();
        if let Some((frame, width, height)) = frame {
            let width = width.to_string();
            let height = height.to_string();
            request
                .into_response(
                    200,
                    Some("OK"),
                    &[
                        ("Content-Type", "application/octet-stream"),
                        ("Cache-Control", "no-store"),
                        ("X-Image-Width", &width),
                        ("X-Image-Height", &height),
                    ],
                )?
                .write_all(&frame)?;
        } else {
            request
                .into_status_response(503)?
                .write_all(b"No display frame has been written")?;
        }
        Ok::<(), anyhow::Error>(())
    })?;
    for mode in ["test", "github"] {
        let mode_display = display.clone();
        let mode_github = github.clone();
        server.fn_handler(&format!("/display/{mode}"), Method::Post, move |request| {
            let frame = if mode == "test" {
                framebuffer::Framebuffer::diagnostic().0
            } else {
                mode_github.lock().unwrap().frame()
            };
            let result = mode_display.lock().unwrap().show(frame, mode);
            match result {
                Ok(()) => {
                    request
                        .into_ok_response()?
                        .write_all(b"Display mode changed")?;
                }
                Err(error) => {
                    request
                        .into_status_response(503)?
                        .write_all(error.to_string().as_bytes())?;
                }
            }
            Ok::<(), anyhow::Error>(())
        })?;
    }
    server.fn_handler("/status", Method::Get, move |request| {
        let body = status.lock().unwrap().json().to_string();
        request
            .into_response(
                200,
                Some("OK"),
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
            )?
            .write_all(body.as_bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(server)
}
