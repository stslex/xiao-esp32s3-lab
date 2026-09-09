# TFT and autonomous GitHub validation — 2026-09-09

## Firmware and hardware

- XIAO ESP32-S3 Sense, OV3660, 8 MB flash / 8 MB PSRAM.
- GMT020-02-7P ST7789V 240×320, portrait RGB565, SPI2 at 10 MHz.
- Owner confirmed CS=D0/GPIO1, DC=D1/GPIO2, RST=D3/GPIO4,
  SDA=D10/GPIO9, SCL=D8/GPIO7, VCC=3V3, GND=GND.
- Existing user authorization covers this flash and subsequent fixes.
- Firmware version 0.3.0; final ELF SHA256:
  `f0ba6e66abc5bdc97ac65a3639f70e1ec5831fb61e5406f0f7e6a7f4ef2a2082`.
- Local evidence is in ignored `captures/`; it is not part of the public source.

## Executed checks

1. `cargo build --offline` passed using the installed ESP-IDF 5.5.3 toolchain.
   Firmware has HTTPS CA bundle verification and does not follow authenticated
   HTTP redirects. Final app size was 1,684,480 bytes.
2. Initial display frame completed SPI transfer. USB reported ready, no display
   error, one frame, CRC32 `24a04699`. Retrieved 153600 RGB565 bytes were rendered
   locally and visually inspected: text, gradient, and red/green/blue bars.
   This is software-buffer inspection, not observation of the panel.
3. The owner entered a separate fine-grained GitHub token using the hidden local
   USB prompt. The agent did not read, print or export the token. The board
   fetched the GraphQL query directly with its own HTTPS client.
4. GitHub returned 2016 commit contributions, 366 opened pull requests and 2960
   total contributions for the returned interval
   `2025-09-06T21:00:00Z` through `2026-09-09T20:59:59Z` (53 calendar weeks).
   These counts matched a separate `gh api graphql` read on the Mac. The display
   uses the API's contribution semantics and exact returned dates.
5. Final flash/reset retained Wi-Fi and GitHub credentials. At uptime 11 seconds,
   status reported GitHub data age 3 seconds, mode `github`, display/camera/HTTP
   ready, and no GitHub error. No host-side stats uploader was running.
6. `python3 tools/verify_display.py --output-dir captures/display-final` passed:
   USB test mode; full-frame CRC; an exact-size, different image uploaded over
   HTTP and retrieved byte-for-byte; USB camera capture during upload; wrong-size
   image rejected with HTTP 400 without changing the frame; HTTP camera still
   available; GitHub restored with monotonic uptime and no reboot.
   Upload plus concurrent USB capture took 0.613 seconds in this bounded check.
7. Both saved camera frames were identified as 320×240 baseline JPEGs. The USB
   receiver also checked chunk order, declared length, CRC32 and JPEG markers.
8. The new web page was opened and visually inspected with a live camera preview,
   readable GitHub counts/calendar, and enabled mode buttons. The web preview is
   labeled as the last sent frame.
9. Host checks passed: five protocol tests (including token framing), one pin
   conflict test, one RGB565 order/clipping test, four host-check tests (including
   the shared framebuffer test), twelve Python camera tests, formatting and
   `git diff --check`. GraphQL negative checks reject partial errors, missing
   counts, duplicate weekdays and an oversized calendar.
10. The owner confirmed that the image is visible and supplied a photo of the
    physical TFT showing the diagnostic frame. The photo confirms readable text,
    correct portrait orientation, the full gradient and red/green/blue bars in
    the expected order. This closes the physical test-picture visibility check.

## Limits and pending observations

- The owner's photo shows the diagnostic picture. The GitHub layout was inspected
  in the software preview, not in that physical photo. This seven-pin connection
  has no panel pixel readback; SPI completion and software CRC alone cannot prove
  physical visibility of each subsequent frame.
- Automatic first fetch and fetch after reset were executed. The 15-minute
  scheduled refresh interval was verified in code, not by waiting an entire
  interval during this check.
- Router outage, token expiration, TLS failure and DMA timeout were not induced
  on the live board. Their error paths were inspected; no long-duration stability
  claim is made.
- Uploaded images and fetched contribution data are in RAM. Token and Wi-Fi
  credentials persist in unencrypted NVS. Reboot selects GitHub when a token is
  configured and obtains a fresh snapshot.
