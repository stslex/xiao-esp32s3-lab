# Camera controls validation — 2026-09-09

## Firmware and scope

- XIAO ESP32-S3 Sense with detected OV3660 and 8 MB PSRAM.
- Rust / ESP-IDF 5.5.3, `esp32-camera` 2.1.7; legacy SCCB retained.
- Firmware 0.4.0, application size 1,712,304 bytes.
- Flashed ELF SHA256:
  `f75b4a658a7eda61636a3530c2af149b734d4647f6bf4401986a6f05434b0923`.
  The same hash prefix appeared in the boot log.
- A 1 MiB PSRAM JPEG buffer was confirmed in the boot log. PSRAM DMA is off.
- Home Wi-Fi station mode and USB remain available together. Camera settings
  use a separate NVS namespace from Wi-Fi and GitHub credentials.

## Executed checks

`tools/verify_camera_controls.py --output-dir captures/camera-controls/validated-040`
passed on the final flashed binary. Local JPEGs and JSON evidence are ignored
under `captures/`, not published with the source.

| Resolution | Dimensions from JPEG SOF | JPEG bytes at setting 12 | HTTP capture seconds |
| --- | --- | ---: | ---: |
| QVGA | 320×240 | 5,098 | 0.083 |
| VGA | 640×480 | 17,758 | 0.176 |
| SVGA | 800×600 | 26,978 | 0.111 |
| XGA | 1024×768 | 43,661 | 0.227 |
| SXGA | 1280×1024 | 189,696 | 0.475 |
| UXGA | 1600×1200 | 288,721 | 0.726 |
| QXGA | 2048×1536 | 485,242 | 0.868 |

These are individual observations, not sustained frame-rate measurements.
Scene content, lighting, Wi-Fi and other clients affect size and latency.

1. All seven settings produced the requested dimensions in the actual JPEG
   headers. Checking only driver metadata would miss queued frames captured at
   the old resolution. The firmware discards an old frame and validates a new
   JPEG before saving settings.
2. At QXGA, JPEG setting 10 produced a 560,596-byte image; setting 30 produced
   205,509 bytes. Quantization-table SHA256 changed from
   `1c4daede6d447623dcf534daa17fee1c495497c2ec7435f6374e81492355c5c5`
   to `3c33f047abab5c49d22e130df0346abec8bac7e2fd6ec3e25ab18ebd68925d9e`.
   The table change demonstrates an actual sensor compression change even when
   scene content changes. A captured QXGA JPEG was decoded and visually inspected.
3. Concurrent QXGA/10 requests succeeded over HTTP (533,435 bytes, 4.997 s) and
   USB (551,008 bytes, 6.764 s). USB also validated framing, chunk order, length
   and CRC. Camera ownership is released before either transport sends data.
4. Brightness +2, contrast -2, saturation +1, mirror enabled and vertical flip
   disabled were accepted together and followed by a valid QXGA frame. This
   confirms setter execution and capture, not calibrated color accuracy.
5. JPEG settings 8 and 41, an unknown resolution, brightness +3, integer mirror
   and an unknown key each returned HTTP 400, preserving previous settings.
   Uptime increased from 18 to 73 seconds throughout the test, with no reboot.
6. A separate persistence check saved XGA/10, brightness +1 and mirror enabled,
   then used `espflash reset`. USB reported the same settings after reboot;
   uptime changed from 112 to 6 seconds. A fresh JPEG was 1024×768.
7. Defaults were restored: SVGA/12, neutral image adjustments, no mirror and
   vertical flip enabled. The previous TFT image was uploaded again and its
   153,600 bytes read back from the software buffer matched exactly. Display
   status was ready with CRC32 `07baf689`; this is not physical panel readback.
8. The updated browser page was opened and visually inspected. Resolution and
   quality menus, color sliders, mirror/flip, Apply, Restore defaults, preview
   pause and full-size download were present. A live image was visible and
   Apply/Restore buttons became enabled after status loaded.
9. Host validation passed: seven shared Rust tests, five USB protocol tests,
   twelve Python tests, firmware build, JavaScript syntax, Rust formatting and
   `git diff --check`.
10. A later read-only status check confirmed firmware 0.4.0, connected station
    Wi-Fi, camera/TFT/HTTP ready, and a fresh autonomous GitHub snapshot with no
    error (2016 commits, 366 PRs, 2960 contributions). Settings had subsequently
    changed through the running service to QXGA/10, brightness +1, no mirror and
    no vertical flip, with a different TFT image. These current settings were
    left intact after the test restoration.

## Reproduced limit and validation boundaries

- An earlier candidate allowed JPEG setting 8. At QXGA it returned HTTP 503
  with `settings_rejected_restored: camera_capture_timeout_try_lower_resolution_or_quality`.
  Serial logs reported `NO-SOI` and a capture timeout. Rollback to SVGA/12
  succeeded. No framebuffer overflow was logged, so its cause is not proven
  to be buffer exhaustion. SVGA/8 and QXGA/10 worked in isolation. The released
  firmware therefore accepts settings 10–40 and offers 10 as **Very high**.
- Rejected setting 8 and the successful rollback were tested on the earlier
  candidate. In the final firmware, 8 is rejected before sensor writes.
- The fixed 1 MiB limit is not a guarantee for every scene. Very complex/noisy
  frames can still fail; lower resolution or more compression is available.
- A larger JPEG adds pixels; it does not correct poor lighting, focus or motion.
  Captures during this check included dark, noisy scenes.
- NVS write failure and sensor bus failure were not injected. Their rollback
  paths were inspected. No long-duration or sustained video-rate claim is made.
- Browser controls were inspected visually; API behavior was exercised by the
  hardware script. Every individual browser click and download was not automated.
