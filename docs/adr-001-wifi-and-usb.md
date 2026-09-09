# ADR-001: Home Wi-Fi for viewing, USB for development

**Status:** Accepted
**Date:** 2026-09-09
**Deciders:** Project owner

## Context

Switching the development Mac to the camera's access point removes its home
internet connection. The development agent must retain internet access while
checking actual camera output. A phone-only preview does not meet that need.

## Decision

Use Wi-Fi station mode for everyday browser viewing and the existing USB
Serial/JTAG console for provisioning, status, and individual JPEG captures.
Both transports use the same camera capture function. There is no automatic
switch of the Mac's network and no automatic firmware flashing.

## Options Considered

| Option | Complexity | Cost | Development access | Decision |
| --- | --- | --- | --- | --- |
| Camera access point | Low | No added hardware | Interrupts Mac Wi-Fi internet | Replace as default |
| Home Wi-Fi station | Moderate | Existing router | Mac keeps internet | Everyday viewing |
| USB serial captures | Moderate | Existing USB cable | Works without a router | Development verification |
| USB UVC webcam | Higher | Existing USB cable | Requires a different USB stack | Deferred |

## Trade-off Analysis

USB uses the existing console VFS with bounded ASCII lines rather than adding
a webcam device stack. JPEG chunks are hex encoded, doubling wire size; this
is intended for snapshots and verification, not high-frame-rate video.
Each transfer has a request ID, declared length, ordered chunks, and CRC32.
Console logs are ignored by the host. A single VFS write keeps each protocol
line separate from other writes. The default polling console backend has a
bounded wait when the host is absent; integrity checks detect dropped data.

## Consequences

- Wi-Fi credentials are entered locally with hidden password input, transferred
  over USB, and stored as one application NVS blob. They are not compiled into
  firmware, passed as command-line arguments, printed, or saved on the Mac.
- NVS is not encrypted in this development firmware. The HTTP camera has no
  application authentication and is intended for a trusted home network.
- Wi-Fi uses 2.4 GHz with a WPA2-compatible personal network and an 8–63-byte
  password. Open, enterprise, and WPA3-only networks are outside this version.
- No credentials or an unavailable router leaves USB operational. Connection
  attempts run separately from the USB worker and retry every 15 seconds.
- HTTP and USB briefly serialize camera access. Each copies and releases the
  camera buffer before sending to a potentially slow host.
- CRC32 and JPEG markers prove transfer integrity, not a decoded image or a
  fresh scene. Hardware validation must also decode/inspect frames and test
  reboot persistence and Wi-Fi loss while USB remains usable.
- The previous access point is no longer started. Recovery/provisioning uses USB.

## Action Items

- [x] Implement station credentials, reconnect attempts, and status.
- [x] Implement USB capture and host integrity checks.
- [x] Add malformed/corrupted transfer and serial transport tests.
- [x] Flash after explicit owner approval and verify real USB JPEG captures.
- [x] Provision home Wi-Fi and verify persistence across a board reset.
- [x] Connect the antenna and pass the bounded concurrent HTTP/USB check; see
  [hardware validation](hardware-validation-2026-09-09.md).

## References

- [ESP-IDF USB Serial/JTAG console](https://docs.espressif.com/projects/esp-idf/en/v5.5.3/esp32s3/api-guides/usb-serial-jtag-console.html)
- [Seeed XIAO ESP32-S3 Wi-Fi](https://wiki.seeedstudio.com/xiao_esp32s3_wifi_usage/)
