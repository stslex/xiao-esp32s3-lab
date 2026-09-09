use crate::{
    display_config,
    framebuffer::{Framebuffer, BYTE_LEN},
    image_layout::{Image, Layout},
    photo_flash::Flash,
    photo_record::PhotoStore,
    protocol,
};
use anyhow::{bail, Context, Result};
use esp_idf_svc::nvs::EspDefaultNvs;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub type SharedDisplay = Arc<Mutex<Display>>;

pub struct Display {
    ready: bool,
    error: Option<String>,
    mode: &'static str,
    frames_written: u64,
    crc32: Option<String>,
    frame: Option<Vec<u8>>,
    source: Option<Image>,
    album_source: Option<Image>,
    mode_revision: u64,
    mode_started: Instant,
    album_elapsed: Duration,
    saved_photo: Option<Image>,
    layout: Layout,
    wire_crc32: Option<String>,
    photo_store: PhotoStore<Flash>,
    settings: Option<EspDefaultNvs>,
    preferred_mode: Option<String>,
    storage_error: Option<String>,
}

impl Display {
    pub fn initialize(settings: Option<EspDefaultNvs>) -> SharedDisplay {
        let mut photo_store = PhotoStore::new(Flash);
        let loaded = photo_store.load();
        let storage_error = loaded.as_ref().err().map(|s| s.to_string());
        let mut buffer = [0u8; 16];
        let preferred_mode = settings.as_ref().and_then(|s| {
            s.get_str("mode", &mut buffer)
                .ok()
                .flatten()
                .map(str::to_owned)
        });
        let mut layout_buffer = [0u8; 256];
        let layout = settings
            .as_ref()
            .and_then(|s| s.get_str("layout", &mut layout_buffer).ok().flatten())
            .and_then(|s| serde_json::from_str(s).ok())
            .and_then(|v| Layout::default().patch(&v).ok())
            .unwrap_or_default();
        let mut display = Self {
            ready: false,
            error: None,
            mode: "test",
            frames_written: 0,
            crc32: None,
            frame: None,
            source: None,
            album_source: None,
            mode_revision: 0,
            mode_started: Instant::now(),
            album_elapsed: Duration::ZERO,
            layout,
            wire_crc32: None,
            saved_photo: loaded.ok().flatten(),
            photo_store,
            settings,
            preferred_mode,
            storage_error,
        };
        match display_config::PINS {
            None => display.error = Some("wiring_not_configured".into()),
            Some(pins) if !pins.valid() => {
                display.error = Some("invalid_or_conflicting_pins".into())
            }
            Some(pins) => {
                let result = unsafe {
                    esp_idf_svc::sys::camera::xiao_display_init(
                        pins.cs, pins.dc, pins.reset, pins.mosi, pins.clock,
                    )
                };
                display.ready = result == 0;
                if result != 0 {
                    display.error = Some(format!("display_init_failed_{result}"));
                }
                if display.ready {
                    if display.preferred_mode.as_deref() == Some("image")
                        && display.saved_photo.is_some()
                    {
                        let frame = display.saved_photo.clone().unwrap();
                        let _ = display.draw(frame, "image");
                    } else {
                        let mode = if display.preferred_mode.as_deref() == Some("album") {
                            "album"
                        } else {
                            "test"
                        };
                        let _ = display
                            .draw(Image::portrait(Framebuffer::diagnostic().0).unwrap(), mode);
                    }
                }
            }
        }
        Arc::new(Mutex::new(display))
    }

    pub fn json(&self) -> Value {
        let (width, height) = self.layout.dimensions();
        json!({"configured": display_config::PINS.is_some(), "ready": self.ready,
            "error": self.error, "mode": self.mode, "width": width, "height": height, "layout":self.layout.json(),
            "frames_written": self.frames_written, "frame_crc32": self.crc32,
            "wire_crc32":self.wire_crc32,
            "saved_photo_available": self.saved_photo.is_some(), "storage_error": self.storage_error,
            "verification": "SPI completion only; no panel pixel readback"})
    }

    pub fn show(&mut self, frame: Vec<u8>, mode: &'static str) -> Result<()> {
        self.show_source(Image::portrait(frame).map_err(anyhow::Error::msg)?, mode)
    }

    pub fn show_source(&mut self, source: Image, mode: &'static str) -> Result<()> {
        if self.preferred_mode.as_deref() != Some(mode) {
            self.settings
                .as_ref()
                .context("display_mode_storage_unavailable")?
                .set_str("mode", mode)?;
            self.preferred_mode = Some(mode.into());
        }
        self.draw(source, mode)
    }

    fn draw(&mut self, source: Image, mode: &'static str) -> Result<()> {
        if !self.ready {
            bail!("{}", self.error.as_deref().unwrap_or("display_not_ready"));
        }
        let frame = self
            .layout
            .render(&source, mode == "image" || mode == "album");
        let physical = self.layout.physical(&frame);
        let result =
            unsafe { esp_idf_svc::sys::camera::xiao_display_draw(physical.as_ptr(), BYTE_LEN) };
        if result != 0 {
            self.ready = false;
            self.error = Some(format!("display_write_failed_{result}"));
            bail!("display_write_failed_{result}");
        }
        self.crc32 = Some(format!("{:08x}", protocol::crc32(&frame)));
        self.wire_crc32 = Some(format!("{:08x}", protocol::crc32(&physical)));
        self.frames_written += 1;
        if self.mode != mode {
            if self.mode == "album" {
                self.album_elapsed += self.mode_started.elapsed();
            }
            self.mode_started = Instant::now();
            self.mode_revision += 1;
            crate::camera::pause_preview();
        }
        self.mode = mode;
        self.frame = Some(frame);
        if mode == "album" {
            self.album_source = Some(source.clone());
        }
        self.source = Some(source);
        Ok(())
    }

    pub fn show_image(&mut self, image: Image) -> Result<()> {
        if self.saved_photo.as_ref() != Some(&image) {
            if let Err(error) = self.photo_store.save(&image) {
                self.storage_error = Some(error.into());
                bail!("{error}");
            }
            self.saved_photo = Some(image.clone());
            self.storage_error = None;
        }
        self.show_source(image, "image")
    }

    pub fn restore_photo(&mut self) -> Result<()> {
        self.show_source(self.saved_photo.clone().context("no_saved_photo")?, "image")
    }

    pub fn saved_photo(&self) -> Option<Image> {
        self.saved_photo.clone()
    }

    pub fn select_album(&mut self) -> Result<()> {
        let source = self
            .album_source
            .clone()
            .unwrap_or_else(|| Image::portrait(Framebuffer::diagnostic().0).unwrap());
        self.show_source(source, "album")
    }

    pub fn album_clock(&self) -> u64 {
        let elapsed = self.album_elapsed
            + if self.mode == "album" {
                self.mode_started.elapsed()
            } else {
                Duration::ZERO
            };
        elapsed.as_millis() as u64
    }
    pub fn mode_revision(&self) -> u64 {
        self.mode_revision
    }

    pub fn configure_layout(&mut self, value: &Value) -> Result<Value> {
        let next = self.layout.patch(value).map_err(anyhow::Error::msg)?;
        if next != self.layout {
            self.settings
                .as_ref()
                .context("display_layout_storage_unavailable")?
                .set_str("layout", &next.json().to_string())?;
            self.layout = next;
            if let Some(source) = self.source.clone() {
                self.draw(source, self.mode)?;
            }
        }
        Ok(self.layout.json())
    }

    pub fn start_github(&self) -> bool {
        self.preferred_mode
            .as_deref()
            .is_none_or(|mode| mode == "github")
    }

    pub fn mode(&self) -> &'static str {
        self.mode
    }
    pub fn frame(&self) -> Option<(Vec<u8>, usize, usize)> {
        self.frame.clone().map(|frame| {
            let (w, h) = self.layout.dimensions();
            (frame, w, h)
        })
    }
}
