use crate::{
    album_data, album_playback::Playback, display::SharedDisplay, image_layout::Image, SharedStatus,
};
use anyhow::{bail, Context, Result};
use embedded_svc::http::client::Client;
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

pub type SharedAlbum = Arc<Mutex<Album>>;
#[derive(Clone)]
struct Config {
    url: String,
    interval: u64,
}
pub struct Album {
    store: Option<EspDefaultNvs>,
    config: Option<Config>,
    generation: u64,
    playback: Playback,
    loading: bool,
    downloads_started: u64,
    cancelled_downloads: u64,
    count: usize,
    partial: bool,
    error: Option<String>,
    updated: Option<Instant>,
}
impl Album {
    pub fn new(store: Option<EspDefaultNvs>) -> SharedAlbum {
        let mut buffer = [0u8; 1536];
        let config = store
            .as_ref()
            .and_then(|s| s.get_str("config", &mut buffer).ok().flatten())
            .and_then(|s| serde_json::from_str(s).ok())
            .and_then(|v| Self::parse(&v).ok());
        Arc::new(Mutex::new(Self {
            store,
            generation: 0,
            playback: Playback::new(config.as_ref().map(|c| c.interval).unwrap_or(60), 0),
            loading: false,
            downloads_started: 0,
            cancelled_downloads: 0,
            count: 0,
            partial: false,
            error: None,
            updated: None,
            config,
        }))
    }
    fn parse(value: &Value) -> Result<Config> {
        let url = value["url"].as_str().context("album_url_required")?.trim();
        if !album_data::album_url(url) {
            bail!("expected_public_google_photos_album_url");
        }
        let interval = value["interval_seconds"]
            .as_u64()
            .context("album_interval_required")?;
        if !(15..=86400).contains(&interval) {
            bail!("album_interval_must_be_15_to_86400_seconds");
        }
        Ok(Config {
            url: url.into(),
            interval,
        })
    }
    pub fn configure(&mut self, value: &Value, clock: u64) -> Result<()> {
        let cfg = Self::parse(value)?;
        self.store
            .as_ref()
            .context("album_storage_unavailable")?
            .set_str(
                "config",
                &json!({"url":cfg.url,"interval_seconds":cfg.interval}).to_string(),
            )?;
        if self
            .config
            .as_ref()
            .is_none_or(|previous| previous.url != cfg.url)
        {
            self.generation += 1;
            self.playback = Playback::new(cfg.interval, clock);
            self.count = 0;
            self.partial = false;
            self.updated = None;
        } else {
            self.playback.interval(cfg.interval, clock);
        }
        self.config = Some(cfg);
        self.error = None;
        Ok(())
    }
    pub fn step(&mut self, direction: i32, clock: u64) -> Result<()> {
        if self.count == 0 {
            bail!("album_not_loaded");
        }
        self.playback.step(direction, self.count, clock);
        Ok(())
    }
    pub fn pause(&mut self, paused: bool, clock: u64) {
        self.playback.pause(paused, clock);
    }
    pub fn json(&self) -> Value {
        json!({"configured":self.config.is_some(), "url":self.config.as_ref().map(|c| &c.url),
            "interval_seconds":self.config.as_ref().map(|c| c.interval).unwrap_or(60), "photos":self.count,
            "current":self.playback.current.map(|i|i+1).unwrap_or(0),
            "paused":self.playback.paused,"remaining_ms":self.playback.remaining_ms,
            "loading":self.loading,"downloads_started":self.downloads_started,"cancelled_downloads":self.cancelled_downloads,"partial_album":self.partial,"last_error":self.error,
            "age_seconds":self.updated.map(|t| t.elapsed().as_secs()), "source":"public_shared_page"})
    }
}

fn download<T>(
    url: &str,
    image: bool,
    valid: &impl Fn() -> bool,
    mut consume: impl FnMut(&[u8]) -> Result<Option<T>>,
) -> Result<T> {
    let mut url = url.to_owned();
    for _ in 0..4 {
        if !valid() {
            bail!("album_cancelled");
        }
        if !(if image {
            album_data::image_url(&url)
        } else {
            album_data::album_url(&url)
        }) {
            bail!("album_redirect_not_allowed");
        }
        let connection = EspHttpConnection::new(&Configuration {
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            timeout: Some(Duration::from_secs(15)),
            buffer_size: Some(4096),
            follow_redirects_policy: FollowRedirectsPolicy::FollowNone,
            ..Default::default()
        })?;
        let mut client = Client::wrap(connection);
        if !valid() {
            bail!("album_cancelled");
        }
        let mut response = client
            .request(
                embedded_svc::http::Method::Get,
                &url,
                &[
                    ("User-Agent", "Mozilla/5.0"),
                    ("Accept-Encoding", "identity"),
                ],
            )?
            .submit()?;
        if matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            url = response
                .header("Location")
                .context("album_redirect_missing")?
                .to_owned();
            continue;
        }
        if response.status() != 200 {
            bail!("album_http_{}", response.status());
        }
        let mut chunk = [0u8; 4096];
        let mut total = 0;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if !valid() {
                bail!("album_cancelled");
            }
            if Instant::now() > deadline {
                bail!("album_download_timeout");
            }
            let n = response.read(&mut chunk)?;
            if !valid() {
                bail!("album_cancelled");
            }
            total += n;
            if total > if image { 256 * 1024 } else { 4 * 1024 * 1024 } {
                bail!("album_response_too_large");
            }
            if let Some(value) = consume(&chunk[..n])? {
                return Ok(value);
            }
            if n == 0 {
                bail!("album_empty_or_page_format_changed");
            }
            FreeRtos::delay_ms(10);
        }
    }
    bail!("album_too_many_redirects")
}

fn fetch_album(url: &str, valid: &impl Fn() -> bool) -> Result<(Vec<String>, bool)> {
    let mut parser = album_data::Document::default();
    let value = download(url, false, valid, |chunk| {
        parser.feed(chunk).map_err(anyhow::Error::msg)
    })?;
    album_data::media(&value).map_err(anyhow::Error::msg)
}
fn fetch_photo(url: &str, valid: &impl Fn() -> bool) -> Result<Image> {
    let mut bytes = Vec::new();
    let jpeg = download(&format!("{url}=w480-h480-rj"), true, valid, |chunk| {
        if chunk.is_empty() {
            return Ok(Some(std::mem::take(&mut bytes)));
        }
        bytes.try_reserve(chunk.len())?;
        bytes.extend_from_slice(chunk);
        Ok(None)
    })?;
    if !valid() {
        bail!("album_cancelled");
    }
    let mut frame = vec![0; 480 * 480 * 2];
    let (mut width, mut height) = (0u16, 0u16);
    let result = unsafe {
        esp_idf_svc::sys::camera::xiao_photo_decode(
            jpeg.as_ptr(),
            jpeg.len(),
            frame.as_mut_ptr(),
            frame.len(),
            &mut width,
            &mut height,
        )
    };
    if result != 0 {
        bail!("album_jpeg_decode_failed_{result}");
    }
    frame.truncate(width as usize * height as usize * 2);
    Image::new(frame, width as usize, height as usize).map_err(anyhow::Error::msg)
}

pub fn start(
    status: SharedStatus,
    album: SharedAlbum,
    display: SharedDisplay,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("photo-album".into())
        .stack_size(24_576)
        .spawn(move || {
            let mut generation = u64::MAX;
            let mut urls: Vec<String> = Vec::new();
            let mut refresh = Instant::now();
            let mut retry_clock = 0;
            let mut retry_revision = 0;
            let mut observed_mode = None;
            loop {
                FreeRtos::delay_ms(250);
                let (cfg, ticket, mode_revision, clock) = {
                    let mut state = album.lock().unwrap();
                    let panel = display.lock().unwrap();
                    let clock = panel.album_clock();
                    state.playback.tick(clock);
                    if observed_mode.is_some_and(|revision| revision != panel.mode_revision()) {
                        state.playback.pending = None;
                        state.playback.revision += 1;
                    }
                    observed_mode = Some(panel.mode_revision());
                    if panel.mode() != "album" {
                        continue;
                    }
                    let Some(cfg) = state.config.clone() else {
                        continue;
                    };
                    if generation != state.generation {
                        generation = state.generation;
                        urls.clear();
                        refresh = Instant::now();
                        retry_clock = 0;
                    }
                    if retry_revision != state.playback.revision {
                        retry_revision = state.playback.revision;
                        retry_clock = 0;
                    }
                    if clock < retry_clock {
                        continue;
                    }
                    if state.playback.paused && state.playback.pending.is_none() {
                        continue;
                    }
                    if !urls.is_empty() && state.playback.target(clock, urls.len()).is_none() {
                        continue;
                    }
                    (cfg, state.playback.revision, panel.mode_revision(), clock)
                };
                let connected = status.lock().unwrap().wifi["connected"] == true;
                let wall_clock = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    > 1_735_689_600;
                if !connected || !wall_clock {
                    album.lock().unwrap().error = Some(
                        if connected {
                            "waiting_for_clock_sync"
                        } else {
                            "wifi_disconnected"
                        }
                        .into(),
                    );
                    continue;
                }
                let valid = || {
                    let state = album.lock().unwrap();
                    let panel = display.lock().unwrap();
                    state.generation == generation
                        && state.playback.revision == ticket
                        && panel.mode() == "album"
                        && panel.mode_revision() == mode_revision
                };
                album.lock().unwrap().loading = true;
                let result = (|| -> Result<(usize, Image)> {
                    if Instant::now() >= refresh || urls.is_empty() {
                        album.lock().unwrap().downloads_started += 1;
                        let (new_urls, partial) = fetch_album(&cfg.url, &valid)?;
                        if !valid() {
                            bail!("album_cancelled");
                        }
                        let mut state = album.lock().unwrap();
                        // Keep the same photo if the hourly refresh changes album ordering.
                        if let Some(old) = state.playback.current.and_then(|i| urls.get(i)) {
                            if let Some(i) = new_urls.iter().position(|url| url == old) {
                                state.playback.current = Some(i);
                            }
                        }
                        urls = new_urls;
                        state.count = urls.len();
                        state.partial = partial;
                        refresh = Instant::now() + Duration::from_secs(3600);
                    }
                    if !valid() {
                        bail!("album_cancelled");
                    }
                    let index = {
                        let mut state = album.lock().unwrap();
                        let panel = display.lock().unwrap();
                        state
                            .playback
                            .target(panel.album_clock(), urls.len())
                            .context("album_cancelled")?
                    };
                    album.lock().unwrap().downloads_started += 1;
                    let image = fetch_photo(&urls[index], &valid)?;
                    Ok((index, image))
                })();
                let mut state = album.lock().unwrap();
                let mut panel = display.lock().unwrap();
                state.loading = false;
                if generation != state.generation
                    || ticket != state.playback.revision
                    || panel.mode() != "album"
                    || panel.mode_revision() != mode_revision
                {
                    state.cancelled_downloads += 1;
                    continue;
                }
                match result
                    .and_then(|(index, image)| panel.show_source(image, "album").map(|_| index))
                {
                    Ok(index) => {
                        state.playback.displayed(index, panel.album_clock());
                        state.updated = Some(Instant::now());
                        state.error = None;
                        retry_clock = 0;
                    }
                    Err(error) => {
                        state.error = Some(error.to_string());
                        retry_clock = clock + 60_000;
                        refresh = Instant::now();
                    }
                }
            }
        })
}
