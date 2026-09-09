# Saved photo and autonomous album validation — 2026-09-09

## Delivered firmware

- Firmware 0.5.0 on the XIAO ESP32-S3 Sense and confirmed ST7789V wiring.
- ELF SHA256: `6b97a00abe59cf0bc635eacb546ed6166597c311a5a2a356f6ee1262f3697847`.
- Application size: 1,745,168 bytes, within the 7,274,496-byte factory partition.
- The boot log confirmed unchanged NVS/PHY offsets and the new photo partition
  at `0x700000`, length `0x100000`. No credential partition was erased.
- `espflash.toml` preserves this table for ordinary project flashes.
- `tools/build.sh` wakes the ESP-IDF build script through its watched binding
  header so native C changes are rebuilt. The initial candidate used a stale
  native object and could not find the photo partition; the corrected binary
  passed the checks below. The initial failure is retained in local evidence.

## Executed hardware checks

`tools/verify_saved_photo.py` passed on the corrected flashed binary. The supplied
shared link and personal images are retained only in ignored local `captures/`
evidence, not in project source or this report.

1. Uploaded the owner's previous 153,600-byte RGB565 image. `GET /display/saved`
   returned exactly the same bytes.
2. Selected GitHub and test modes in turn. The saved image stayed unchanged;
   `POST /display/restore` restored it byte-for-byte each time.
3. A wrong-sized upload returned HTTP 400 and left the saved image unchanged.
4. Reset the board while image mode was selected. Both selected mode and image
   bytes survived. The returned display frame and saved-photo endpoint matched
   the original image exactly after reset.
5. Configured the supplied public album with a temporary 15-second interval.
   The board discovered 78 photos, with `partial_album: false`, and downloaded,
   decoded and displayed the first photo using its own HTTPS client.
6. Without any host photo upload, the next scheduled frame arrived after 18.62
   seconds of observation. The two display frames had different CRCs. Their
   RGB565 buffers were decoded and visually inspected; both showed distinct
   album photos, correct colors and letterboxing.
7. The Next photo endpoint advanced again. Returning to Saved photo and waiting
   18 seconds kept image mode and the original pixels intact; the album worker
   did not overwrite that selected mode.
8. Saved a 60-second interval, selected album mode and reset again. The URL,
   interval and selected mode survived. The board obtained a fresh album list
   and displayed a photo after reconnecting, without host provisioning.
9. The manual photo still matched byte-for-byte after album playback and both
   resets. A USB camera snapshot also completed, including protocol integrity
   checks. Camera, TFT and Wi-Fi remained available.
10. The updated page was opened and visually inspected at a narrow viewport:
    Saved photo, Album slideshow, album URL/interval and Next photo were usable.
    Status subsequently showed `Playing · photo 2 of 78 · every 60s`, confirming
    a scheduled change at the final interval as well. GitHub counts remained
    visible and the camera preview continued at 800×600.

## Executed host checks

- Fourteen Rust tests passed, including all prior shared parser/rendering and
  USB protocol tests.
- The flash-record test interrupted a replacement payload write, reconstructed
  the store as after reboot, and recovered the previous image. It also corrupted
  the newest payload and recovered the older valid slot by CRC.
- The album parser test splits a document at every byte boundary and confirms
  that off-domain and injected URLs are rejected.
- Twelve Python tests, JavaScript syntax, Python syntax, Rust formatting and
  `git diff --check` passed. The firmware built without warnings.

## Boundaries

- The public Google Photos page is an undocumented source, not the official
  Ambient API. Page-layout changes, revoked sharing or unavailable image URLs
  can stop updates. Errors retain the last displayed frame and retry later.
- This album's 78 entries were discovered; only the first few images were
  downloaded and inspected during the bounded test. Large/paginated albums are
  limited to their first page and at most 200 entries, and report that limit.
- Photos are downloaded as small JPEGs over HTTPS with certificate validation.
  Redirects are restricted to Google Photos / Google image hosts. HTML is
  streamed with a 4 MiB limit, extracted JSON is limited to 256 KiB, and JPEGs
  to 128 KiB. Decoder output is explicitly bounded before allocation and decode.
- Flash interruption was simulated on the host; power was not physically cut
  during a write. No claim is made about every possible hardware failure.
- Validation inspected SPI completion and the stored/transmitted software
  buffers. It did not read physical panel pixels or inspect a new owner photo.
- Slide images are kept in RAM and never replace or repeatedly rewrite the
  manual saved photo. A restart reloads the configured album from the network;
  this is not a complete offline album cache.
