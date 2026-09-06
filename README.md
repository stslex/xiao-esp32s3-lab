# XIAO ESP32-S3 Sense camera in Rust

Rust/ESP-IDF firmware for the Seeed Studio XIAO ESP32-S3 Sense. It starts its
own Wi-Fi access point and serves JPEG frames from the onboard camera.

## Open the camera

1. Flash the firmware.
2. Join Wi-Fi `XIAO-RUST-CAM` with password `xiao-camera`.
3. Open <http://192.168.71.1>.

The page repeatedly requests `/capture`, giving a simple live preview without
the complexity of an MJPEG stream.

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

`espflash` discovers the connected ESP32-S3 automatically. If several ESP
devices are attached, select the XIAO when prompted.

## Command line

```bash
source .esp/export-esp.sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo build
cargo run
```

`camera_bridge/` is a small C wrapper around Espressif's official
`esp32-camera` ESP-IDF component. Wi-Fi, HTTP, application lifetime, and frame
ownership are handled in Rust.
