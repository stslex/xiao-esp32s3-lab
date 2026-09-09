use crate::camera_config::{jpeg_dimensions, Settings, MAX_JPEG, RESOLUTIONS};
use anyhow::{bail, Context, Result};
use esp_idf_svc::{hal::delay::FreeRtos, nvs::EspDefaultNvs};
use serde_json::{json, Value};
use std::{
    ffi::c_void,
    slice,
    sync::Mutex,
    time::{Duration, Instant},
};

static CAMERA: Mutex<State> = Mutex::new(State {
    settings: Settings::DEFAULT,
    ready: false,
    error: None,
    store: None,
    active: false,
    last_used: None,
    preview_session: 0,
    preview_until: None,
    snapshots: 0,
    wakeups: 0,
    suspensions: 0,
});

struct State {
    settings: Settings,
    ready: bool,
    error: Option<String>,
    store: Option<EspDefaultNvs>,
    active: bool,
    last_used: Option<Instant>,
    preview_session: u64,
    preview_until: Option<Instant>,
    snapshots: u64,
    wakeups: u64,
    suspensions: u64,
}

pub fn initialize() -> Result<()> {
    let mut state = CAMERA.lock().unwrap();
    let result = unsafe { esp_idf_svc::sys::camera::xiao_camera_init() };
    state.ready = result == 0;
    state.active = result == 0;
    state.last_used = Some(Instant::now());
    if result != 0 {
        state.error = Some(format!("camera_initialization_failed_0x{result:x}"));
        bail!("camera initialization failed: 0x{result:x}");
    }
    Ok(())
}

pub fn attach_store(store: EspDefaultNvs) -> Result<()> {
    let mut state = CAMERA.lock().unwrap();
    let mut buffer = [0u8; 512];
    let saved = store.get_str("settings", &mut buffer)?.map(str::to_owned);
    state.store = Some(store);
    if let Some(saved) = saved {
        let value: Value = serde_json::from_str(&saved)?;
        let next = Settings::DEFAULT
            .patch(&value)
            .map_err(anyhow::Error::msg)?;
        change(&mut state, next, false)?;
    }
    Ok(())
}

pub fn status() -> Value {
    let state = CAMERA.lock().unwrap();
    json!({"ready": state.ready, "error": state.error, "sensor": "OV3660",
        "settings": state.settings.json(), "persistence_available": state.store.is_some(),
        "max_jpeg_bytes": MAX_JPEG, "active": state.active,
        "preview_enabled":state.preview_until.is_some_and(|until| Instant::now()<until),
        "snapshots":state.snapshots,"wakeups":state.wakeups,"suspensions":state.suspensions,
        "idle_timeout_seconds":2,
        "free_psram_bytes":unsafe { esp_idf_svc::sys::heap_caps_get_free_size(esp_idf_svc::sys::MALLOC_CAP_SPIRAM) }})
}

pub fn start_preview() -> u64 {
    let mut state = CAMERA.lock().unwrap();
    state.preview_session += 1;
    state.preview_until = Some(Instant::now() + Duration::from_secs(5));
    state.preview_session
}
pub fn pause_preview() {
    let mut state = CAMERA.lock().unwrap();
    state.preview_session += 1;
    state.preview_until = None;
}
pub fn poll_idle() {
    // Never delay network housekeeping behind a camera request.
    let Ok(mut state) = CAMERA.try_lock() else {
        return;
    };
    if state.active
        && state
            .last_used
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(2))
    {
        let result = unsafe { esp_idf_svc::sys::camera::xiao_camera_suspend() };
        if result == 0 {
            state.active = false;
            state.suspensions += 1;
        } else {
            state.error = Some(format!("camera_suspend_failed_{result}"));
        }
    }
}
fn wake(state: &mut State) -> Result<()> {
    if state.active {
        return Ok(());
    }
    let result = unsafe { esp_idf_svc::sys::camera::xiao_camera_init() };
    if result != 0 {
        bail!("camera_wakeup_failed_{result}");
    }
    state.active = true;
    state.wakeups += 1;
    state.last_used = Some(Instant::now());
    if let Err(error) = apply_checked(state.settings) {
        let result = unsafe { esp_idf_svc::sys::camera::xiao_camera_suspend() };
        if result == 0 {
            state.active = false;
        }
        return Err(error);
    }
    Ok(())
}

pub fn settings() -> Settings {
    CAMERA.lock().unwrap().settings
}

pub fn configure(next: Settings) -> Result<Value> {
    let mut state = CAMERA.lock().unwrap();
    change(&mut state, next, true)?;
    Ok(state.settings.json())
}

fn change(state: &mut State, next: Settings, persist: bool) -> Result<()> {
    if !state.ready {
        bail!("camera_not_ready");
    }
    if state.settings == next {
        return Ok(());
    }
    let previous = state.settings;
    let result = wake(state).and_then(|_| apply_checked(next)).and_then(|_| {
        if persist {
            state
                .store
                .as_ref()
                .context("camera_settings_storage_unavailable")?
                .set_str("settings", &next.stored())?;
        }
        Ok(())
    });
    state.last_used = Some(Instant::now());
    match result {
        Ok(()) => {
            state.settings = next;
            state.error = None;
            Ok(())
        }
        Err(error) => {
            // A failed SCCB write may leave a partially changed sensor. Restore the
            // entire previous configuration and verify a frame before accepting reads.
            state.ready = apply_checked(previous).is_ok();
            state.error = Some(if state.ready {
                format!("settings_rejected_restored: {error}")
            } else {
                "camera_recovery_failed_restart_required".into()
            });
            bail!("{}", state.error.as_deref().unwrap());
        }
    }
}

fn apply_checked(settings: Settings) -> Result<()> {
    let result = unsafe {
        esp_idf_svc::sys::camera::xiao_camera_apply(
            settings.resolution as i32,
            settings.quality,
            settings.brightness,
            settings.contrast,
            settings.saturation,
            settings.hmirror as i32,
            settings.vflip as i32,
        )
    };
    if result != 0 {
        bail!("camera_setting_write_failed_0x{result:x}");
    }
    // There may already be one queued/in-flight frame from the old controls.
    let old = unsafe { esp_idf_svc::sys::camera::xiao_camera_capture() };
    if !old.is_null() {
        drop(CameraFrame(old));
    }
    FreeRtos::delay_ms(20);
    let _verified = capture_matching(settings)?;
    Ok(())
}

pub fn snapshot() -> Result<Vec<u8>> {
    snapshot_for(None)
}
pub fn preview_snapshot(session: u64) -> Result<Vec<u8>> {
    snapshot_for(Some(session))
}
fn snapshot_for(preview: Option<u64>) -> Result<Vec<u8>> {
    let mut state = CAMERA
        .lock()
        .map_err(|_| anyhow::anyhow!("camera lock poisoned"))?;
    if !state.ready {
        bail!("camera_not_ready");
    }
    if let Some(session) = preview {
        if state.preview_session != session
            || !state
                .preview_until
                .is_some_and(|until| Instant::now() < until)
        {
            bail!("camera_preview_paused");
        }
    }
    let result = wake(&mut state).and_then(|_| capture_matching(state.settings));
    state.last_used = Some(Instant::now());
    if result.is_ok() {
        state.snapshots += 1;
        if preview.is_some() {
            state.preview_until = Some(Instant::now() + Duration::from_secs(5));
        }
    }
    state.error = result.as_ref().err().map(|error| error.to_string());
    result
}

fn capture_matching(settings: Settings) -> Result<Vec<u8>> {
    let (_, width, height) = RESOLUTIONS[settings.resolution];
    for _ in 0..3 {
        let raw = unsafe { esp_idf_svc::sys::camera::xiao_camera_capture() };
        if raw.is_null() {
            bail!("camera_capture_timeout_try_lower_resolution_or_quality");
        }
        let frame = CameraFrame(raw);
        let length = unsafe { esp_idf_svc::sys::camera::xiao_camera_frame_length(frame.0) };
        let data = unsafe { esp_idf_svc::sys::camera::xiao_camera_frame_data(frame.0) };
        if data.is_null() || !(4..=MAX_JPEG).contains(&length) {
            bail!("invalid_camera_frame");
        }
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        // Read the JPEG itself; the driver's metadata uses the latest configured
        // size even when the queued image was captured with the previous size.
        if bytes.ends_with(&[0xff, 0xd9]) && jpeg_dimensions(bytes) == Some((width, height)) {
            let mut copy = Vec::new();
            copy.try_reserve_exact(length)?;
            copy.extend_from_slice(bytes);
            return Ok(copy);
        }
        drop(frame);
        FreeRtos::delay_ms(10);
    }
    bail!("camera_returned_wrong_resolution_or_invalid_jpeg")
}

struct CameraFrame(*mut c_void);
impl Drop for CameraFrame {
    fn drop(&mut self) {
        unsafe { esp_idf_svc::sys::camera::xiao_camera_release(self.0) };
    }
}
