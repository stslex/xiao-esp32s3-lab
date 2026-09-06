use std::{convert::TryInto, ffi::c_void, slice, sync::Mutex, thread, time::Duration};

use anyhow::{bail, Context, Result};
use embedded_svc::{
    http::Method,
    io::Write,
    wifi::{self, AccessPointConfiguration, AuthMethod},
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::peripherals::Peripherals,
    http::server::{Configuration as HttpConfiguration, EspHttpServer},
    nvs::EspDefaultNvsPartition,
    wifi::{BlockingWifi, EspWifi},
};
use log::info;

const WIFI_SSID: &str = "XIAO-RUST-CAM";
const WIFI_PASSWORD: &str = "xiao-camera";
const WIFI_CHANNEL: u8 = 6;
const INDEX_HTML: &str = include_str!("index.html");
const HTTP_STACK_SIZE: usize = 12_288;

static CAMERA_LOCK: Mutex<()> = Mutex::new(());

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    initialize_camera()?;

    let peripherals = Peripherals::take().context("peripherals already taken")?;
    let system_event_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, system_event_loop.clone(), Some(nvs))?,
        system_event_loop,
    )?;
    start_access_point(&mut wifi)?;

    let server = start_http_server()?;

    info!("Camera ready: connect to {WIFI_SSID} and open http://192.168.71.1");

    loop {
        // Keeping these values in scope keeps both services alive.
        let _services = (&wifi, &server);
        thread::sleep(Duration::from_secs(60));
    }
}

fn initialize_camera() -> Result<()> {
    let result = unsafe { esp_idf_svc::sys::camera::xiao_camera_init() };
    if result != 0 {
        bail!("camera initialization failed with ESP-IDF error 0x{result:x}");
    }
    info!("Camera initialized (QVGA JPEG, framebuffer in PSRAM)");
    Ok(())
}

fn start_access_point(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    let configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
        ssid: WIFI_SSID.try_into().expect("valid SSID"),
        ssid_hidden: false,
        auth_method: AuthMethod::WPA2Personal,
        password: WIFI_PASSWORD.try_into().expect("valid password"),
        channel: WIFI_CHANNEL,
        ..Default::default()
    });

    wifi.set_configuration(&configuration)?;
    wifi.start()?;
    wifi.wait_netif_up()?;

    info!("Wi-Fi access point started: {WIFI_SSID}");
    Ok(())
}

fn start_http_server() -> Result<EspHttpServer<'static>> {
    let configuration = HttpConfiguration {
        stack_size: HTTP_STACK_SIZE,
        ..Default::default()
    };
    let mut server = EspHttpServer::new(&configuration)?;

    server.fn_handler("/", Method::Get, |request| {
        let headers = [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Cache-Control", "no-store"),
        ];
        request
            .into_response(200, Some("OK"), &headers)?
            .write_all(INDEX_HTML.as_bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;

    server.fn_handler("/capture", Method::Get, |request| {
        let _camera_guard = CAMERA_LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("camera lock poisoned"))?;
        let frame = CameraFrame::capture()?;
        let content_length = frame.len().to_string();
        let headers = [
            ("Content-Type", "image/jpeg"),
            ("Content-Length", content_length.as_str()),
            ("Cache-Control", "no-store, no-cache, must-revalidate"),
            ("Access-Control-Allow-Origin", "*"),
        ];
        request
            .into_response(200, Some("OK"), &headers)?
            .write_all(frame.bytes())?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(server)
}

struct CameraFrame {
    raw: *mut c_void,
}

impl CameraFrame {
    fn capture() -> Result<Self> {
        let raw = unsafe { esp_idf_svc::sys::camera::xiao_camera_capture() };
        if raw.is_null() {
            bail!("camera returned no frame");
        }
        Ok(Self { raw })
    }

    fn len(&self) -> usize {
        unsafe { esp_idf_svc::sys::camera::xiao_camera_frame_length(self.raw) }
    }

    fn bytes(&self) -> &[u8] {
        let data = unsafe { esp_idf_svc::sys::camera::xiao_camera_frame_data(self.raw) };
        unsafe { slice::from_raw_parts(data, self.len()) }
    }
}

impl Drop for CameraFrame {
    fn drop(&mut self) {
        unsafe { esp_idf_svc::sys::camera::xiao_camera_release(self.raw) };
    }
}
