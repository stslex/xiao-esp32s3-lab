# Display orientation and photo cropping validation — 2026-09-09

Firmware: **0.6.0**, XIAO ESP32-S3 Sense with the previously confirmed
ST7789V 240×320 SPI wiring. The firmware was built with `sh tools/build.sh
--offline` and flashed using the existing `partitions.csv`, preserving NVS and
the photo partition. Application size: **1,762,976 bytes** of 7,274,496 available.

ELF SHA-256:

```text
ce112da7382fd5177a43a4c7d4f1d02063f903f5726e5415ea0bd2260482c829
```

## Behavior

- Portrait and both landscape directions use the same native panel driver.
  Rust renders a 240×320 or 320×240 logical image and rotates pixels into the
  fixed 240×320 physical wire buffer.
- Photos support contain and cover framing, with horizontal/vertical crop focus.
  GitHub and diagnostic content always fit completely to preserve their text.
- Manual uploads retain an uncropped RGB565 source with dimensions; layout
  changes only redraw it. New uploads are limited to 76,800 pixels and sides of
  640 pixels. Album JPEGs are bounded to 480×480 and 256 KiB before decoding.
- Layout settings persist in NVS. Photo record version 2 protects source
  dimensions and flags with the header CRC and retains version 1 compatibility.
  The alternating slots, offsets and maximum payload size remain unchanged.

## Executed checks

The host harness passed **16 tests**, including independent color/corner oracles
for cropping and both rotations, invalid settings, the old record format,
variable-aspect source persistence, damaged metadata and interrupted writes.
The existing Python suite passed **12 tests**. Firmware compilation and the
embedded JavaScript syntax check passed.

`tools/verify_display_layout.py` exercised the board using a 400×180 source with
known red/green/blue/white regions and yellow/cyan edges. These checks passed:

- The previously saved manual photo remained readable after the firmware upgrade.
- Portrait contain produced the expected black margins and source colors.
- Cover framing at both horizontal extremes selected the expected source regions.
- Both landscape directions returned 320×240 logical frames. Independently
  rotated Python buffers matched the board's physical wire CRC32 values.
- Layout changes left the source bytes unchanged.
- Invalid rotation, fit, focus and unknown-field patches returned 400 without
  changing layout or drawing a frame.
- A USB reset retained landscape orientation, cover framing, both focus values,
  source dimensions, source bytes and the exact displayed logical frame.

The initial combined run did **not** pass its album portion: observed frame
dimensions changed between layout requests and a saved-image comparison failed.
The origin of those intervening changes was not established. The report is kept
as a failed run, not counted as album proof. The verifier now also asserts album
dimensions and avoids restoring an old photo over a different concurrently
selected photo.

A separate hardware run checked the real album frame in portrait contain/cover,
landscape contain/cover and the opposite landscape direction. All five checks
passed with exact requested layout/dimensions, matching logical CRC32 and an
unchanged saved manual photo after every transition. Captured PNGs were visually
inspected: contain preserved the full frame and cover filled the screen with
proportional cropping. USB camera capture also passed during album playback.
The starting portrait/cover settings were restored; the album remained active.

The refreshed browser UI was inspected: both orientation choices, framing,
crop sliders and Apply were present and enabled, with live album and camera
status. The existing old browser page needed a refresh to expose new controls.

Local evidence is in ignored `captures/display-layout/validated-060/` (the
combined run) and `captures/display-layout/album-060/` (the passing isolated
album run). Images, album links and device status reports are not checked in.

## Limits

The frame API and CRC checks verify rendered pixels and completed SPI writes,
not a readback from the physical panel. No new physical-screen photograph was
provided for this firmware. Earlier physical validation established the panel
driver and color order; the driver remains unchanged here.

Legacy uploads may already contain black letterboxing. Symmetric pure-black
outer margins are ignored during reframing without changing their stored bytes;
that heuristic can also interpret a deliberately black border as padding.
New uploads carry dimensions and do not use that heuristic. The stored source
is a display-sized bitmap, not the original full-resolution photo file.

Google Photos parsing remains dependent on its public shared-page format.
Hardware power-loss injection and SPI fault injection were not performed.
