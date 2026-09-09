#!/usr/bin/env python3
"""Verify TFT transport with a physical board; restores GitHub mode on completion."""
import argparse
import binascii
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import time
import urllib.error
import urllib.request

from camera import Camera, Serial, resolve_port


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    serial = Serial(resolve_port(args.port))
    camera = Camera(serial)
    report = {"passed": False, "checks": [], "limitation": "SPI completion and buffer integrity are not physical pixel readback."}
    base = None

    def request(path, body=None):
        headers = {"Content-Type": "application/octet-stream"} if body is not None else {}
        req = urllib.request.Request(base + path, data=body, headers=headers)
        with urllib.request.urlopen(req, timeout=15) as response:
            return response.read()

    try:
        report["before"] = camera.status()
        address = report["before"]["wifi"]["ip"]
        if not address:
            raise RuntimeError("Board has no Wi-Fi address")
        base = f"http://{address}"
        if next(camera.responses("DISPLAY_TEST")) != "OK":
            raise RuntimeError("No test-picture acknowledgement")
        diagnostic = request("/display/frame")
        assert len(diagnostic) == 153600
        assert f"{binascii.crc32(diagnostic):08x}" == camera.status()["display"]["frame_crc32"]
        (args.output_dir / "test.rgb565").write_bytes(diagnostic)
        report["checks"].append("USB test mode; full framebuffer length and CRC")

        # A deliberately different exact-size image detects ignored or stale uploads.
        pixel_row = b"".join(color.to_bytes(2, "big") * 60 for color in (0xf800, 0x07e0, 0x001f, 0xffff))
        image = pixel_row * 320
        started = time.monotonic()
        with ThreadPoolExecutor(max_workers=2) as pool:
            uploaded = pool.submit(request, "/display/image", image)
            jpeg = camera.capture()
            uploaded.result()
        report["upload_seconds_with_usb_camera"] = round(time.monotonic() - started, 3)
        (args.output_dir / "usb-during-upload.jpg").write_bytes(jpeg)
        received = request("/display/frame")
        assert received == image, "Uploaded frame mismatch"
        state = camera.status()
        assert state["display"]["mode"] == "image"
        assert state["display"]["frame_crc32"] == f"{binascii.crc32(image):08x}"
        report["checks"].append("HTTP image upload byte equality and CRC, concurrent USB JPEG")

        previous = state["display"]["frames_written"]
        try:
            request("/display/image", b"bad")
            raise AssertionError("Invalid image accepted")
        except urllib.error.HTTPError as error:
            assert error.code == 400
        assert camera.status()["display"]["frames_written"] == previous
        assert request("/display/frame") == image
        report["checks"].append("Wrong-size image rejected with 400; frame unchanged")

        jpeg = request("/capture")
        assert jpeg.startswith(b"\xff\xd8") and jpeg.endswith(b"\xff\xd9")
        (args.output_dir / "http-after-upload.jpg").write_bytes(jpeg)
        request("/display/github", b"")
        report["after"] = camera.status()
        assert report["after"]["display"]["mode"] == "github"
        assert report["after"]["display"]["ready"]
        assert report["after"]["camera_ready"]
        assert report["after"]["uptime_seconds"] >= report["before"]["uptime_seconds"]
        report["checks"].append("HTTP camera and GitHub mode restored without reboot")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        if base:
            try:
                request("/display/github", b"")
            except Exception as error:
                report["restore_error"] = str(error)
                report["passed"] = False
        serial.close()
        (args.output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
