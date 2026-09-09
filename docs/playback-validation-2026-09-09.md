# Playback and camera suspension validation — 2026-09-09

Firmware **0.7.0** was built with `sh tools/build.sh --offline` and flashed on
the attached XIAO ESP32-S3 Sense using the existing partition table. Application
size: **1,777,648 bytes** of 7,274,496 available. Saved Wi-Fi/GitHub credentials,
camera/display controls and manual photo storage were retained.

ELF SHA-256:

```text
89675873ce7927d57b6986a82bfb135695bada0c706c02aa14b5ffa8d9eaa94b
```

## Implementation

`album_playback.rs` separates the displayed index from a pending navigation
target. The timer consumes only time spent in album display mode while playing.
Explicit pause retains the remaining time; manual navigation works while paused.
Adjacent navigation wraps, and rapid commands update the pending target.

The display retains the decoded album source separately from the manual photo.
Selecting album mode redraws that source immediately without advancing or
downloading it. Mode revisions and playback revisions invalidate stale download
results, including a quick exit and return to the same mode. Download cancellation
is checked between reads and before decoding/committing a photo. Retries preserve
the current photo rather than silently advancing after a failure.

Camera previews now require an explicit expiring session. Display mode changes
invalidate the session; legacy browser polling cannot start capture. A periodic
idle check calls `esp_camera_deinit()` after two seconds without requests, then
disconnects XCLK from GPIO10 and holds it low. A subsequent snapshot reinitializes
the camera and applies the saved settings. The bundled driver frees its capture
task, queues, DMA resources and buffers on deinitialization; see the
[Espressif camera driver](https://github.com/espressif/esp32-camera/blob/master/driver/cam_hal.c).
This is not a hardware power switch for the camera module.

## Executed evidence

The host suite passed **19 tests**, including deterministic checks of remaining
time, hidden-mode time, backward/forward wraparound, rapid navigation, pause
cancellation and interval changes retaining position. JavaScript syntax,
Rust formatting, Python compilation and firmware compilation passed.

`tools/verify_album_playback.py` completed **9 hardware checks**. Evidence lives
in ignored `captures/playback-070/report.json` and its JPEG files. No personal
photos, shared-album URLs, tokens or device reports are checked in.

- Three idle → capture → idle camera cycles produced valid JPEGs at the saved
  **2048×1536, JPEG setting 10** configuration. Each suspension released
  **1,048,860 bytes of PSRAM** relative to the active-camera sample.
- Selecting a different display mode invalidated a running preview session.
  Subsequent requests with that session and legacy `?t=...` requests returned
  409. The capture counter stayed unchanged afterwards.
- An abandoned preview session expired; requesting it after six seconds failed
  without waking the camera.
- USB capture woke the idle camera and it suspended again afterwards.
- With a temporary 15-second interval, an 18-second explicit pause held photo 1,
  its exact RGB565 frame and its 14,951 ms remaining time. Album download count
  stayed at 2, frame-write count did not change, camera capture count stayed at
  6 and the camera remained inactive.
- Previous from photo 1 selected photo 79; Next returned to photo 1 while staying
  paused. The restored frame matched byte for byte.
- After resuming for three seconds, selecting GitHub froze photo 1 with
  **11,045 ms** left. During 18 seconds away, both remaining time and the download
  count (4) stayed unchanged. Returning restored the identical photo without
  downloading it; a sample showed 11,025 ms left. It did not advance immediately
  and later advanced when the remaining interval elapsed.
- Pausing during an observed in-flight photo download increased the cancellation
  counter and retained the exact previously displayed frame and index.
- The manual saved photo's SHA-256 remained unchanged throughout.

A separate short check, recorded in ignored
`captures/playback-070/mode-cancellation.json`, also passed: an explicitly paused
album stayed paused after a mode round trip with no new download; a rapid
test → album round trip during a download cancelled the stale request without
changing the current frame or starting a replacement download.

The original GitHub display mode, 60-second interval and camera controls were
restored. The refreshed browser UI was inspected at a narrow viewport: Previous,
Pause/Resume and Next fit the layout; controls correctly showed the album as
suspended in GitHub mode, and the camera showed Start preview with no automatic
capture on page load.

## Limits

Playback position, countdown and decoded source are retained in RAM across mode
switches, not power loss. A board restart begins album playback again. A paused
album retains its cached pixels and list in RAM to enable immediate restoration;
the worker still performs lightweight state checks, but does not fetch/decode
new photos. A network call already blocked at cancellation must return or reach
its timeout before resources are released.

Another explicitly active HTTP/USB client can request snapshots and wake the
camera. Measurements establish freed memory and stopped capture/download work;
no electrical power measurements or physical LCD pixel readback were performed.
Google Photos support still depends on its public shared-page format.
