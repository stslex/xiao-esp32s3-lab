use crate::{
    display::SharedDisplay,
    github_data::{waiting_frame, Snapshot},
    SharedStatus,
};
use anyhow::{bail, Context, Result};
use embedded_svc::{http::client::Client, io::Write};
use esp_idf_svc::{
    hal::delay::FreeRtos,
    http::client::{Configuration, EspHttpConnection, FollowRedirectsPolicy},
    nvs::EspDefaultNvs,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const USERNAME: &str = "stslex";
const REFRESH_SECONDS: u64 = 900;
const MAX_RESPONSE: usize = 64 * 1024;
pub type SharedGithub = Arc<Mutex<Github>>;

pub struct Github {
    store: Option<EspDefaultNvs>,
    token: Option<String>,
    generation: u64,
    data: Option<Snapshot>,
    fetched: Option<Instant>,
    error: Option<String>,
}

impl Github {
    pub fn new(store: Option<EspDefaultNvs>) -> SharedGithub {
        let mut state = Self {
            store,
            token: None,
            generation: 0,
            data: None,
            fetched: None,
            error: None,
        };
        let mut buffer = [0u8; 128];
        if let Some(store) = &state.store {
            match store.get_str("token", &mut buffer) {
                Ok(Some(token)) => state.token = Some(token.to_owned()),
                Ok(None) => {}
                Err(_) => state.error = Some("github_token_storage_read_failed".into()),
            }
        }
        Arc::new(Mutex::new(state))
    }

    pub fn configure(&mut self, token: Option<String>) -> Result<()> {
        let store = self
            .store
            .as_ref()
            .context("github_token_storage_unavailable")?;
        if let Some(token) = &token {
            store.set_str("token", token)?;
        } else {
            store.remove("token")?;
        }
        self.token = token;
        self.generation += 1;
        self.data = None;
        self.fetched = None;
        self.error = None;
        Ok(())
    }

    pub fn json(&self) -> Value {
        json!({"username": USERNAME, "configured": self.token.is_some(), "refresh_seconds": REFRESH_SECONDS,
            "age_seconds": self.fetched.map(|time| time.elapsed().as_secs()),
            "last_error": self.error, "data": self.data.as_ref().map(Snapshot::json)})
    }

    pub fn frame(&self) -> Vec<u8> {
        match &self.data {
            Some(data) => data.frame(
                self.fetched
                    .map(|time| time.elapsed().as_secs() / 60)
                    .unwrap_or(0),
                self.error.is_some(),
            ),
            None => waiting_frame(self.token.is_some()),
        }
    }
}

pub fn start(
    status: SharedStatus,
    github: SharedGithub,
    display: SharedDisplay,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("github-stats".into())
        .stack_size(20_480)
        .spawn(move || {
            let mut next_fetch = Instant::now();
            let mut next_draw = Instant::now();
            let mut generation = 0;
            let mut was_ready = false;
            loop {
                let connected = status.lock().unwrap().wifi["connected"] == true;
                let clock_ready = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    > 1_735_689_600;
                if connected && clock_ready && !was_ready {
                    next_fetch = Instant::now();
                }
                was_ready = connected && clock_ready;
                let token = {
                    let state = github.lock().unwrap();
                    if state.generation != generation {
                        generation = state.generation;
                        next_fetch = Instant::now();
                        next_draw = Instant::now();
                    }
                    if connected && clock_ready && Instant::now() >= next_fetch {
                        state.token.clone()
                    } else {
                        None
                    }
                };
                if let Some(token) = token {
                    let mut retry_seconds = REFRESH_SECONDS;
                    let result = fetch(&token, &mut retry_seconds);
                    drop(token);
                    let mut state = github.lock().unwrap();
                    // Ignore a response from credentials that were replaced or removed in flight.
                    if state.generation == generation {
                        match result {
                            Ok(data) => {
                                state.data = Some(data);
                                state.fetched = Some(Instant::now());
                                state.error = None;
                            }
                            Err(error) => {
                                state.error = Some(error.to_string());
                                log::warn!("GitHub update failed: {error}");
                            }
                        }
                    }
                    next_fetch = Instant::now() + Duration::from_secs(retry_seconds);
                    next_draw = Instant::now();
                }
                {
                    let mut state = github.lock().unwrap();
                    if state.token.is_none() {
                        state.error = None;
                    } else if !connected {
                        state.error = Some("wifi_disconnected".into());
                    } else if !clock_ready {
                        state.error = Some("waiting_for_clock_sync".into());
                    }
                }
                if Instant::now() >= next_draw {
                    let frame = github.lock().unwrap().frame();
                    let mut panel = display.lock().unwrap();
                    if panel.mode() == "github" {
                        let _ = panel.show(frame, "github");
                    }
                    next_draw = Instant::now() + Duration::from_secs(60);
                }
                FreeRtos::delay_ms(1000);
            }
        })
}

fn fetch(token: &str, retry_seconds: &mut u64) -> Result<Snapshot> {
    let connection = EspHttpConnection::new(&Configuration {
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(Duration::from_secs(15)),
        buffer_size: Some(2048),
        // Never forward the Authorization header to a redirect destination.
        follow_redirects_policy: FollowRedirectsPolicy::FollowNone,
        ..Default::default()
    })?;
    let mut client = Client::wrap(connection);
    let body = json!({"query": include_str!("github.graphql"), "variables": {"login": USERNAME}})
        .to_string();
    let authorization = format!("Bearer {token}");
    let length = body.len().to_string();
    let headers = [
        ("User-Agent", "xiao-esp32s3-lab"),
        ("Accept", "application/json"),
        ("Accept-Encoding", "identity"),
        ("Content-Type", "application/json"),
        ("Content-Length", &length),
        ("Authorization", &authorization),
    ];
    let mut request = client.post("https://api.github.com/graphql", &headers)?;
    request.write_all(body.as_bytes())?;
    let mut response = request.submit()?;
    if response.status() == 403 || response.status() == 429 {
        *retry_seconds = 3600;
        if let Some(seconds) = response
            .header("Retry-After")
            .and_then(|value| value.parse::<u64>().ok())
        {
            *retry_seconds = (*retry_seconds).max(seconds.min(86_400));
        }
    }
    if response.status() != 200 {
        bail!("github_http_{}", response.status());
    }
    if response
        .header("Content-Length")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_RESPONSE)
    {
        bail!("github_response_too_large");
    }
    let mut body = Vec::new();
    let mut chunk = [0u8; 2048];
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if Instant::now() >= deadline {
            bail!("github_body_timeout");
        }
        let count = response.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if body.len() + count > MAX_RESPONSE {
            bail!("github_response_too_large");
        }
        body.try_reserve(count)
            .context("github_response_allocation_failed")?;
        body.extend_from_slice(&chunk[..count]);
        FreeRtos::delay_ms(10);
    }
    Snapshot::parse(&body).map_err(anyhow::Error::msg)
}
