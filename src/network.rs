use crate::{protocol::Credentials, SharedStatus};
use anyhow::{Context, Result};
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_svc::{nvs::EspDefaultNvs, wifi::EspWifi};
use serde_json::json;
use std::{
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};

pub enum Change {
    Set(Credentials),
    Forget,
}
pub struct Request {
    pub change: Change,
    pub reply: SyncSender<Result<(), &'static str>>,
}

pub struct Network {
    wifi: EspWifi<'static>,
    store: EspDefaultNvs,
    configured: bool,
    last_attempt: Instant,
    status: SharedStatus,
    last_error: Option<String>,
    last_ip: Option<String>,
}

impl Network {
    pub fn new(wifi: EspWifi<'static>, store: EspDefaultNvs, status: SharedStatus) -> Self {
        let mut network = Self {
            wifi,
            store,
            configured: false,
            last_attempt: Instant::now(),
            status,
            last_error: None,
            last_ip: None,
        };
        if let Err(error) = network.load() {
            network.last_error = Some(format!("Wi-Fi setup failed: {error}"));
        }
        network.poll();
        network
    }

    fn load(&mut self) -> Result<()> {
        let mut buffer = [0u8; 256];
        if let Some(value) = self.store.get_blob("credentials", &mut buffer)? {
            let credentials = Credentials::from_stored(std::str::from_utf8(value)?)
                .map_err(anyhow::Error::msg)?;
            self.configure(&credentials)?;
        }
        Ok(())
    }

    fn configure(&mut self, credentials: &Credentials) -> Result<()> {
        self.configured = false;
        if self.wifi.is_started()? {
            self.wifi.stop()?;
        }
        self.wifi
            .set_configuration(&Configuration::Client(ClientConfiguration {
                ssid: credentials
                    .ssid
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid SSID"))?,
                password: credentials
                    .password
                    .as_str()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid password length"))?,
                auth_method: AuthMethod::WPA2Personal,
                ..Default::default()
            }))?;
        self.wifi.start()?;
        // This camera is externally powered; prioritize continuous image delivery.
        esp_idf_svc::sys::esp!(unsafe {
            esp_idf_svc::sys::esp_wifi_set_ps(esp_idf_svc::sys::wifi_ps_type_t_WIFI_PS_NONE)
        })?;
        self.configured = true;
        self.last_attempt = Instant::now();
        self.wifi.connect().context("start connection")?;
        Ok(())
    }

    pub fn handle(&mut self, request: Request) {
        let response = match self.apply(request.change) {
            Ok(()) => {
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                // Never include credentials or the command in errors or logs.
                self.last_error = Some(format!("Wi-Fi update failed: {error}"));
                Err("wifi_update_failed_check_status")
            }
        };
        self.poll();
        let _ = request.reply.send(response);
    }

    fn apply(&mut self, change: Change) -> Result<()> {
        match change {
            Change::Set(credentials) => {
                self.store
                    .set_blob("credentials", credentials.stored().as_bytes())?;
                self.configure(&credentials)?;
            }
            Change::Forget => {
                self.store.remove("credentials")?;
                if self.wifi.is_started()? {
                    self.wifi.stop()?;
                }
                self.configured = false;
            }
        }
        Ok(())
    }

    pub fn poll(&mut self) {
        let up = self.configured && self.wifi.is_up().unwrap_or(false);
        if self.configured && !up && self.last_attempt.elapsed() >= Duration::from_secs(15) {
            self.last_attempt = Instant::now();
            let _ = self.wifi.disconnect();
            if let Err(error) = self.wifi.connect() {
                self.last_error = Some(format!("Wi-Fi reconnect: {error}"));
            }
        }
        let ip = if up {
            self.last_error = None;
            self.wifi
                .sta_netif()
                .get_ip_info()
                .ok()
                .map(|info| info.ip.to_string())
        } else {
            None
        };
        if ip != self.last_ip {
            if let Some(ref address) = ip {
                log::info!("Camera URL: http://{address}");
            }
            self.last_ip = ip.clone();
        }
        let rssi = if up {
            let mut record = esp_idf_svc::sys::wifi_ap_record_t::default();
            let result = unsafe { esp_idf_svc::sys::esp_wifi_sta_get_ap_info(&mut record) };
            (result == 0).then_some(record.rssi)
        } else {
            None
        };
        self.status.lock().unwrap().wifi = json!({
            "mode": "station", "configured": self.configured,
            "connected": up, "ip": ip, "last_error": self.last_error,
            "rssi_dbm": rssi,
        });
    }
}
