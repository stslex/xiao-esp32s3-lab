# XIAO ESP32-S3 Sense camera in Rust

Rust/ESP-IDF firmware for the Seeed Studio XIAO ESP32-S3 Sense. View the camera
over your home Wi-Fi and capture JPEGs over USB for development without changing
the Mac's network or losing internet access.

## Open the camera

After flashing, keep the board connected by USB and run:

```bash
python3 tools/camera.py wifi-setup
```

On macOS, you can also double-click `tools/wifi-setup.command` to open the
local setup prompts in Terminal.

Enter the name of your **2.4 GHz, WPA2-compatible personal network** and its
password at the local prompts. Password input is hidden. Credentials are stored
on the board and survive a restart; they are not put in source files, command
arguments, or a local configuration file. This development firmware stores NVS
without encryption and exposes HTTP to devices on the same trusted network.

The tool prints the camera URL after connection. Open that URL from a device on
the same home network. The router assigns the IP, so it can change; retrieve it
again with `python3 tools/camera.py status`.

Version 0.2.0 no longer starts `XIAO-RUST-CAM`. If Wi-Fi is unconfigured or the
router is unavailable, USB remains available. The board retries connection every
15 seconds. It can be powered from a USB power supply for normal Wi-Fi use.
Keep the supplied external antenna connected. `status` reports `wifi.rssi_dbm`
when connected; a very weak signal can allow small status responses while JPEG
transfers time out. Wi-Fi modem power saving is disabled for image delivery.

The camera starts idle. Press **Start preview** for live camera updates or
**Use camera picture** for a fresh single photo. Opening the page does not start
capture. Preview stops when choosing another display mode or hiding the page.

## Playback controls and idle camera (0.7.0)

The album has **Previous photo**, **Next photo**, **Pause slideshow** and
**Resume slideshow** controls. Previous/Next wrap at the ends and work while
paused. Pause keeps the current photo on the TFT and freezes its countdown.
Selecting GitHub, the saved photo or the test screen suspends album downloads
and the countdown. Returning to **Album slideshow** immediately restores the
cached photo and continues its remaining time. An explicit pause remains paused
when returning. Changing the interval for the same album keeps its position.
Playback position and the decoded current slide are retained in RAM during the
running session; restarting the board begins the album again.

Album work checks cancellation between network reads and before decoding or
drawing. A request already blocked in network I/O may take up to its 15-second
socket timeout to stop; its result cannot overwrite the held photo after pause
or a mode switch. No subsequent download starts while suspended. The cached
photo and album list remain in RAM to make returning immediate.

The camera driver stops after two seconds without capture requests. Deinitializing
releases its task, DMA resources and frame buffers; the XCLK pin is held low.
A new HTTP or USB snapshot wakes it and reapplies saved controls. Wakeup adds
latency to the first capture. This is resource suspension, not a switched-off
camera power supply. The board, Wi-Fi and lightweight status service stay active.

Live browser preview requires `POST /camera/preview/start`, which returns a
session number for `GET /capture?preview=NUMBER`. Successful preview captures
renew a five-second lease. `POST /camera/preview/stop`, a display mode change or
an expired lease invalidates it. Plain `GET /capture` and USB snapshots remain
explicit one-shot operations. Old `?t=...` preview requests return 409 until the
browser page is refreshed, preventing stale tabs from continuously waking the
camera. Another explicitly active client can still request camera snapshots.

`/status` exposes camera `active`, `snapshots`, `wakeups`, `suspensions` and free
PSRAM, plus album `paused`, `remaining_ms`, `loading`, `downloads_started` and
`cancelled_downloads`. These help distinguish a held screen from continuing
background capture/download work. Hardware validation is available through:

```bash
python3 tools/verify_album_playback.py --url http://BOARD_IP --output-dir captures/playback-check
```

## Orientation and photo cropping (0.6.0)

In **TFT display**, select **Screen orientation**: portrait (240×320), or
landscape (320×240) with the physical screen turned left or right. Select
**Fit whole photo** to preserve the entire image with black margins, or
**Fill screen · crop edges** to fill the screen without stretching. The two
**Crop position** sliders choose which part of an oversized image remains visible.
Press **Apply display settings** to redraw the current photo immediately.

These settings apply to saved photos and album slides and survive a restart.
New uploads retain their aspect ratio before framing, so crop changes never
discard the saved source pixels. The browser reduces uploads to at most 76,800
pixels with sides up to 640 pixels; the original full-resolution file stays on
the sending device. Old 240×320 saved photos remain readable; symmetric black
letterbox margins from older uploads are ignored when reframing. GitHub and the
test screen rotate with the display and always fit completely, preserving text.

`POST /display/settings` accepts a partial JSON object up to 256 bytes:

```json
{"rotation":90,"fit":"cover","focus_x":50,"focus_y":50}
```

Rotation is `0`, `90` (turn the physical screen left), or `270` (turn it right).
Fit is `contain` or `cover`; focus coordinates range from 0 to 100.
Unknown fields and invalid values return 400 without changing settings.
See `docs/display-layout-validation-2026-09-09.md` for validation evidence.

## Saved photo and Google Photos slideshow

**Send image** and **Use camera picture** save the prepared source image on
the board. **Saved photo** restores it after using GitHub, the test picture or an
album. The saved photo and selected display mode survive a restart. Re-sending
the same image does not rewrite flash; changing modes does not rewrite photo
data. Album slides never replace the saved manual photo.

Paste a public Google Photos album link into **Google Photos album** and press
**Save and start slideshow**. The link must open without signing in to Google.
The board downloads and decodes the images itself over verified HTTPS; the Mac
and browser can be disconnected. The default interval is 60 seconds, adjustable
from 15 seconds to one day. **Previous photo** and **Next photo** request an
adjacent slide; **Pause slideshow** holds the current photo. Select another
display mode to suspend playback; **Album slideshow** returns to the held slide.

This uses the public shared-album web page, not an official stable API. The
[Google Photos Ambient API](https://developers.google.com/photos/partner-program/overview)
requires partner-program acceptance, while the
[Library API](https://developers.google.com/photos/support/updates) no longer
provides general access to existing shared albums. Google page changes can break
this integration. Hardware checks have exercised albums with 78 and 79 photos. The parser reads
at most the first 200 entries on the first page; larger/paginated albums are
explicitly reported as partial in the UI. Authentication pages and unsupported
formats report an error and keep the current displayed frame.

Album configuration is saved in NVS; slide pixels and image URLs are cached in
RAM. The album page is refreshed hourly during playback, or after a download
failure, with a 60-second retry delay on errors. The board requests JPEGs bounded
to 480×480 and applies the selected orientation and framing. Each next-slide
interval starts after the previous image is displayed; downloads add latency.

Additional endpoints:

- `GET /display/saved`: the uncropped saved manual RGB565 image, with
  `X-Image-Width` and `X-Image-Height` headers, or 404.
- `POST /display/restore`: select the saved photo.
- `POST /album/settings`: JSON `url` and `interval_seconds`; save and start.
- `POST /display/album`: resume the configured slideshow.
- `POST /album/next`: request the next slide.
- `POST /album/previous`: request the previous slide, wrapping at the start.
- `POST /album/pause`: hold the current photo and countdown.
- `POST /album/resume`: continue the countdown. These playback controls require
  album display mode; Previous/Next also require a loaded album.
- `/status`: saved-photo availability, storage errors, album configuration,
  photo count/index, update age and download errors.

`espflash.toml` selects `partitions.csv`. The original NVS and PHY offsets remain
unchanged, the application ends before `0x700000`, and the last 1 MiB of flash is
reserved for photo records. Two alternating slots use payload/header CRCs and
write the new header last. The data partition uses the standard SPIFFS subtype
for tool compatibility, but stores raw records and is never mounted as SPIFFS.
Do not override this table when flashing this firmware. Photo data and album
configuration, like the existing device settings, are stored without encryption.

Hardware checks are documented in `docs/photo-album-validation-2026-09-09.md`.

## Camera controls (0.4.0)

The **Camera** section now controls resolution, JPEG quality, brightness,
contrast, saturation, horizontal mirror and vertical flip. Press **Apply settings**
to change and save them; **Restore defaults** restores 800×600, JPEG setting 12,
neutral color adjustments and the original vertical flip. Camera settings survive
restarts separately from Wi-Fi and GitHub credentials.

Available resolutions are 320×240, 640×480, 800×600, 1024×768, 1280×1024,
1600×1200 and the OV3660's maximum 2048×1536 (3 MP). Higher resolution adds
detail but reduces preview speed. **Pause preview** stops the preview session.
**Save full-size photo** downloads a fresh JPEG at the current camera
resolution. The separate TFT has 240×320 physical pixels, presented as 320×240
in landscape, regardless of camera resolution.

The quality menu uses sensor JPEG settings 10 (very high), 12 (high), 20 (balanced)
and 30 (smaller files). In the sensor API a lower number means less compression.
The firmware accepts integer settings 10–40, brightness/contrast/saturation -2–2,
and booleans for mirror/flip. Automatic exposure and white balance remain enabled.
Setting 8 timed out during 3 MP hardware validation, so this firmware limits the
range to 10–40. Setting 10 passed concurrent 3 MP capture over HTTP and USB.

`GET /camera/settings` returns camera status and settings.
`POST /camera/settings` accepts a JSON object up to 512 bytes, for example:

```json
{"framesize":"QXGA","quality":12,"hmirror":false}
```

Allowed frame names are `QVGA`, `VGA`, `SVGA`, `XGA`, `SXGA`, `UXGA`, `QXGA`.
Unknown keys, unsupported sizes and out-of-range values return 400 before any
sensor writes. Successful changes are checked against a captured JPEG's actual
dimensions before being saved. Sensor/storage failures attempt to restore the
previous configuration; if recovery fails, status requests a restart.

A dedicated 1 MiB PSRAM JPEG buffer supports runtime resolution changes.
HTTP and USB snapshots release the camera buffer before transmission. Extremely
complex/noisy scenes at maximum quality can exceed this bounded buffer: select
more compression or a lower resolution if capture reports a timeout. The USB
host receiver accepts up to 1 MiB and uses a 90-second request deadline by default.

Hardware validation (changes camera settings temporarily and restores them):

```bash
python3 tools/verify_camera_controls.py --output-dir captures/camera-controls-check
```

It checks JPEG dimensions at every supported size, actual quantization table
changes between quality levels, concurrent HTTP/USB capture at 3 MP and rejection
of invalid settings. See `docs/camera-controls-validation-2026-09-09.md`.

## TFT images and GitHub activity (0.3.0)

The connected display is a GoldenMorning **GMT020-02-7P**, ST7789V, 240×320.
Its [manufacturer specification](https://goldenmorninglcd.com/tft-display-module/2-inch-240x320-st7789v-gmt020-02/)
identifies a four-wire SPI interface. The confirmed wiring is:

| Display | XIAO label | GPIO / supply |
| --- | --- | --- |
| CS | D0 | GPIO1 |
| DC | D1 | GPIO2 |
| RST | D3 | GPIO4 |
| SDA (MOSI) | D10 | GPIO9 |
| SCL (clock) | D8 | GPIO7 |
| VCC | 3V3 | 3.3 V |
| GND | GND | Ground |

`src/display_config.rs` holds **raw GPIO numbers**, not the D-label numbers.
Only use this configuration with the corresponding wiring. These pins do not
overlap the Sense camera, USB, flash or PSRAM. The Sense SD card interface is not
initialized; its SPI pins are shared with the display wiring.

Open the board's normal home-network URL. **Test picture** displays text,
a gradient and red/green/blue bars. Choose a JPG/PNG/WebP and press **Send image**,
or press **Use camera picture**. Images use the selected orientation and framing
without changing their aspect ratio. **GitHub stats** selects the activity
dashboard. The last manually selected image is retained in flash. The preview
represents the last successfully transmitted frame in its viewing orientation,
not a readback of physical panel pixels.

The GitHub dashboard shows `stslex` by default (change `USERNAME` in
`src/github.rs` for another user): commit contributions, opened pull requests,
total contributions, and a daily calendar for GitHub's default yearly window.
The exact returned dates are printed below the calendar. These are GitHub
contribution counts, not `git log` totals across every branch. Calendar intensity
uses local buckets of 0, 1–3, 4–9, 10–19 and 20+ contributions per day.

### Autonomous GitHub setup

Create a separate **fine-grained personal access token**, with **Public repositories**
and no additional/write permissions, using
[GitHub's token settings](https://github.com/settings/personal-access-tokens/new).
Only data visible to that token can be counted; restricted/private contributions
follow GitHub's visibility rules. Do not reuse a broad development token.

```bash
python3 tools/camera.py github-setup
```

On macOS, `tools/github-setup.command` opens the same local prompt. Paste the
token into the hidden terminal prompt. The host does not accept it as a command
argument or save it to a file. It is sent over USB and stored in **unencrypted
NVS on this development board**. It is not returned in status, logged, or exposed
through an HTTP configuration endpoint.

The board syncs its clock with SNTP and requests the GraphQL API directly using
HTTPS certificate validation. The Mac can then be disconnected: the board only
needs power and home Wi-Fi. Refresh is every 15 minutes, with a longer delay on
rate limiting. Failures retain the last valid in-memory snapshot and show its age;
the web/USB status reports the error. After reboot, the board restores the selected
display mode and fetches fresh GitHub data in the background. GitHub is the initial
default when no display mode has been saved. On token expiration, repeat `github-setup`.

```bash
python3 tools/camera.py display-test
python3 tools/camera.py display-github
python3 tools/camera.py github-forget
```

`github-forget` deletes the board's stored token; revoke it in GitHub separately
if it should no longer work anywhere. USB diagnostics and camera capture work
independently of GitHub. These HTTP display controls are for the trusted local
network, like the camera endpoints.

The display API accepts `POST /display/image` with big-endian RGB565 pixels and
`X-Image-Width` / `X-Image-Height` headers. The body must match those dimensions,
with at most 76,800 pixels and sides up to 640. Legacy uploads without dimensions
must contain exactly 153600 bytes (240×320). It also accepts `POST /display/test`
and `POST /display/github`. `GET /display/frame` returns the last logical RGB565
frame with dimension headers: 240×320 or 320×240, always 153600 bytes. `/status`
includes layout, mode, SPI completion count, logical and physical wire CRC32,
GitHub counts and snapshot age.
The C bridge uses an internal DMA stripe buffer, waits for completion before
reuse, and stops drawing on an uncertain transfer; Rust serializes access.

Host checks for the same contribution parser and renderer used on the board:

```bash
cargo +stable test --manifest-path tools/host-check/Cargo.toml --target aarch64-apple-darwin
```

Use your host's target triple on other systems. See
`docs/display-validation-2026-09-09.md` for executed checks and remaining limits.

## USB capture and verification

The host tool uses only the Python 3 standard library on macOS/Linux. It selects
the port automatically if exactly one USB serial device is attached. Otherwise
use `python3 tools/camera.py --port /dev/cu.YOUR_DEVICE status`. Close other
serial monitors before running it. It does not flash firmware or intentionally
toggle the device's reset lines.

```bash
python3 tools/camera.py status
python3 tools/camera.py capture
python3 tools/camera.py verify --count 3
```

Captures and JSON reports go into the ignored `captures/` directory. Existing
files are never overwritten. The verifier checks request identity, chunk order,
length, CRC32, and JPEG start/end markers. Decode and inspect the saved JPEGs
as well: transport integrity does not prove valid pixels or a changing scene.
USB sends individual snapshots; it does not register the board as a UVC webcam.

Reconfigure with `wifi-setup`, or remove saved credentials and stop Wi-Fi with:

```bash
python3 tools/camera.py wifi-forget
```

USB setup uses an 8–63-byte password. Open networks, enterprise authentication,
and WPA3-only networks are outside this version.

## USB protocol

Send an ASCII line `XIAO1 <u32-request-id> <command>\n`. Commands are `STATUS`,
`CAPTURE`, `WIFI_SET <utf8-ssid-hex> <utf8-password-hex>`, `WIFI_FORGET`,
`DISPLAY_TEST`, `DISPLAY_GITHUB`, `GITHUB_SET <fine-grained-token-hex>`,
and `GITHUB_FORGET`.
Commands are limited to 256 bytes. Malformed/oversized commands are discarded;
the host validates credentials before sending them. Hex is encoding, not encryption.

Responses start with `XIAO1 <same-id> `:

- `STATUS <json>`: firmware version, camera initialization, uptime, and Wi-Fi IP.
- `OK` or `ERR <reason>`: configuration acknowledgement or execution error.
- `BEGIN <length> <crc32-hex>`, then `DATA <byte-offset> <hex>` chunks, then `END`.

Chunks contain up to 256 bytes. Maximum JPEG size is 1 MiB. Normal log lines
can occur between responses. Each request has a fixed host deadline. Incomplete
or corrupt captures fail instead of producing a success report. `WIFI_SET`'s
`OK` confirms saved settings and starting the connection attempt, not obtaining
an address; `wifi-setup` polls status for the address.

## Validation

Host checks do not require the board or change network configuration:

```bash
rustup run stable rustc --edition=2021 --test src/protocol.rs -o /tmp/xiao-protocol-tests
/tmp/xiao-protocol-tests
python3 -m unittest discover -s tools -p 'test_*.py' -v
```

After an approved flash, verify actual hardware:

1. Run USB `status` and `verify` before configuring Wi-Fi; decode/inspect JPEGs.
2. Run `wifi-setup`, open the reported URL, and check `/status` and `/capture`.
3. Capture over USB while the browser preview is running.
4. Restart the board and confirm it rejoins the saved network.
5. With the router unavailable, confirm USB still works; restore the router and
   confirm reconnection. Do not interrupt a shared router without permission.

The design and outstanding hardware checks are recorded in
[ADR-001](docs/adr-001-wifi-and-usb.md).

## Zed tasks

Install and initialize the toolchain first:

```bash
brew install rustup espflash
cargo install espup ldproxy
mkdir -p .esp
espup install --std --targets esp32s3 --export-file .esp/export-esp.sh
```

Then start Zed with the ESP environment:

```bash
source .esp/export-esp.sh
export PATH="$HOME/.cargo/bin:$PATH"
zed .
```

Use `Cmd+Shift+R` to run:

- `Rust: Build`
- `Rust: Flash + Monitor`
- `Rust: Serial Monitor`
- `Rust: Format`
- `Camera: USB Status`
- `Camera: USB Verify`
- `Camera: Wi-Fi Setup`

`espflash` discovers the connected ESP32-S3 automatically. If several ESP
devices are attached, select the XIAO when prompted.

## Command line

```bash
source .esp/export-esp.sh
export PATH="$HOME/.cargo/bin:$PATH"
sh tools/build.sh
espflash flash --monitor target/xtensa-esp32s3-espidf/debug/xiao-esp32s3-camera
```

`camera_bridge/` is a small C wrapper around Espressif's official
`esp32-camera` ESP-IDF component. Wi-Fi, HTTP, application lifetime, and frame
ownership are handled in Rust.
