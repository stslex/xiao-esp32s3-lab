# Hardware validation: home Wi-Fi and USB camera

## Executed

- Owner approved flashing version 0.2.0 on 2026-09-09.
- Device identified as ESP32-S3 revision 0.2 with 8 MB flash.
- Boot detected the OV3660 camera and 8 MB PSRAM; PSRAM memory test passed.
- Initial USB-validated ELF SHA256:
  `e98bde8daffc18ebe3cdde7abe8d14cfa344199f0effe4f5f3cfe3a3be25fc3e`.
- USB status reported firmware `0.2.0`, protocol `XIAO1`, camera ready, HTTP
  ready, and no camera/HTTP/Wi-Fi initialization errors.
- With Wi-Fi unconfigured, USB verification received three JPEGs at board
  uptime 31–32 seconds. Lengths: 2,879 / 3,662 / 3,606 bytes. CRC32 values:
  `f451c802` / `81d1f73a` / `851023d6`. Request/ordering/length/CRC32/JPEG-marker
  checks passed. Per-frame times: 0.272 / 0.285 / 0.290 seconds.
- macOS identified all three frames as 320×240. The third frame was decoded
  and visually inspected. Different checksums alone are not proof of scene motion.
- Host checks: four Rust protocol tests and twelve Python tests passed,
  including corrupt/incomplete frames and a real pseudo-terminal transport test.
- The Mac's Wi-Fi connection was not changed for flashing or USB capture.

Raw JPEGs and the machine-readable USB report are kept locally under the
ignored `captures/usb-initial/` directory, not included in the repository.

## Watchdog issue reproduced and corrected

The first flashed build, ELF SHA256
`0c5711a3b6666e01c583d79937dd4019c6febe912d7c69d09cc07a53513e35dd`,
triggered task-watchdog warnings naming an idle task. Its backtrace pointed to
the USB worker's `std::thread::sleep` call through `usleep` and `ets_delay_us`.

ESP-IDF is configured at 100 Hz (one tick is 10 ms). Its `usleep` implementation
busy-waits for delays below one tick, so the 2 ms polling pause did not let idle
tasks run. The fix uses `FreeRtos::delay_ms(10)` for polling and periodically
during long frame transfers. After rebuilding and reflashing, startup reached
the USB/HTTP services at about 1.4 seconds, and no watchdog warning appeared in
the subsequent boot-monitor observation before the first capture check.

This is a bounded observation, not a long-duration stability claim.

## Remaining hardware checks

- Check reconnection after a network outage without interrupting the Mac's
  internet connection or a shared router.

## Home Wi-Fi and final build

- The owner entered home-network settings through the local Terminal setup
  helper. USB status confirmed a station connection; HTTP `/status` was read.
- A browser displayed a real image and reached `Camera online · frame 12`.
- Sustained concurrent verification did not pass: USB returned all ten frames,
  but HTTP JPEG transfers timed out. An isolated HTTP request also timed out,
  so this was not limited to simultaneous camera access. The device logged
  `httpd_sock_err: error in send : 11` and `ESP_ERR_HTTPD_RESP_SEND`.
- The first HTTP test also incorrectly required `Content-Length`. The service
  uses chunked encoding; the corrected test accepts either valid framing. The
  subsequent timeouts above were independent of that test correction.
- Added station RSSI to USB/HTTP status and disabled Wi-Fi modem power saving
  for this externally powered camera. Final flashed ELF SHA256:
  `cd63f7b7646b4231dff8659c66e59fed164d00ac5ee96f48207024d508335583`.
- Disabling power saving did not resolve the HTTP timeouts. Measured RSSI was
  approximately -88 to -92 dBm; a ping sample lost one of four packets. Weak
  RF/antenna connectivity is a leading hypothesis, not a proven sole cause.
- A normal board reset preserved credentials and rejoined the home network.
  Post-reset USB verification passed three more frames, 5,299 / 6,903 / 6,902
  bytes, in 0.305 / 0.327 / 0.320 seconds. CRC32: `ea64ac04` / `6b7dc12b` /
  `4b6de656`. All were identified as 320×240; the third was decoded and viewed.

Local reports are in `captures/dual-transport*/` and
`captures/usb-after-reset/`. Failed reports are retained. The antenna follow-up
below resolves the weak-signal blocker for the bounded HTTP workload.

## Antenna follow-up: passed

The owner reported connecting the missing antenna. No firmware change or
reflash was made between the failing final-build test and this repeat.

- RSSI improved from approximately -88 to -92 dBm before the antenna check to
  -69 dBm initially and -56 dBm at the end of the concurrent test.
- Four ping requests all succeeded: 4.500–6.171 ms, average 5.075 ms.
- The same concurrent workload passed all ten HTTP JPEG transfers and all ten
  USB transfers while the browser preview was also open.
- HTTP frame times: 0.028–0.104 seconds, average 0.039 seconds.
- USB frame times: 0.313–0.335 seconds, average 0.320 seconds.
- No watchdog or panic message was found in the serial log collected by the
  test. Status remained camera-ready, HTTP-ready, and Wi-Fi-connected.
- The final HTTP and USB samples were identified as 320×240. The browser
  rendered an image and its visible frame counter advanced from 124 to 176.
- The full successful report and twenty JPEGs are local in the ignored
  `captures/dual-transport-antenna/` directory.

These results support the antenna connection as the cause of the earlier
radio-link failure. This is a successful bounded concurrency test, not a
long-duration or router-outage reliability claim.
