#!/usr/bin/env python3
"""USB camera capture and Wi-Fi provisioning; Python standard library, macOS/Linux."""
import argparse
import binascii
import fcntl
import getpass
import glob
import json
import os
from pathlib import Path
import secrets
import select
import sys
import termios
import time
import tty
import warnings

PREFIX = "XIAO1"
MAX_FRAME = 1024 * 1024
MAX_LINE = 2048


class ProtocolError(Exception):
    pass


class Serial:
    def __init__(self, port):
        self.fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        self.original = None
        self.buffer = bytearray()
        try:
            fcntl.ioctl(self.fd, termios.TIOCEXCL)
            self.original = termios.tcgetattr(self.fd)
            tty.setraw(self.fd)
            settings = termios.tcgetattr(self.fd)
            settings[2] |= termios.CLOCAL | termios.CREAD
            settings[2] &= ~termios.HUPCL
            settings[4] = settings[5] = termios.B115200
            termios.tcsetattr(self.fd, termios.TCSANOW, settings)
            termios.tcflush(self.fd, termios.TCIFLUSH)
        except BaseException:
            self.close()
            raise

    def close(self):
        try:
            if self.original is not None:
                # Do not deliberately toggle DTR/RTS or reset the board on close.
                self.original[2] &= ~termios.HUPCL
                termios.tcsetattr(self.fd, termios.TCSANOW, self.original)
        except (OSError, termios.error):
            pass  # An unplugged device cannot have its terminal settings restored.
        finally:
            os.close(self.fd)

    def write(self, data, deadline):
        while data:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([], [self.fd], [], remaining)[1]:
                raise TimeoutError("USB write timed out")
            try:
                count = os.write(self.fd, data)
            except BlockingIOError:
                continue
            if not count:
                raise OSError("USB disconnected")
            data = data[count:]

    def readline(self, deadline):
        while True:
            if b"\n" in self.buffer:
                line, _, rest = self.buffer.partition(b"\n")
                self.buffer = bytearray(rest)
                if len(line) > MAX_LINE:
                    raise ProtocolError("Oversized USB line")
                return bytes(line).rstrip(b"\r")
            if len(self.buffer) > MAX_LINE:
                raise ProtocolError("Oversized USB line")
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.fd], [], [], remaining)[0]:
                raise TimeoutError("No complete USB response; check firmware, cable, and other serial monitors")
            try:
                chunk = os.read(self.fd, 1024)
            except BlockingIOError:
                continue
            if not chunk:
                raise OSError("USB disconnected")
            self.buffer.extend(chunk)


class Frame:
    def __init__(self):
        self.data = bytearray()
        self.length = None
        self.crc = None

    def feed(self, response):
        fields = response.split()
        try:
            if len(fields) == 3 and fields[0] == "BEGIN" and self.length is None:
                self.length, self.crc = int(fields[1]), int(fields[2], 16)
                if not 4 <= self.length <= MAX_FRAME or not 0 <= self.crc <= 0xffffffff:
                    raise ProtocolError("Invalid frame header")
            elif len(fields) == 3 and fields[0] == "DATA" and self.length is not None:
                offset = int(fields[1])
                chunk = bytes.fromhex(fields[2])
                if offset != len(self.data) or not 1 <= len(chunk) <= 256:
                    raise ProtocolError("Missing, repeated, or out-of-order frame data")
                if len(self.data) + len(chunk) > self.length:
                    raise ProtocolError("Frame exceeds declared length")
                self.data.extend(chunk)
            elif fields == ["END"] and self.length is not None:
                if len(self.data) != self.length:
                    raise ProtocolError("Truncated frame")
                if binascii.crc32(self.data) != self.crc:
                    raise ProtocolError("Frame CRC32 mismatch")
                if not self.data.startswith(b"\xff\xd8") or not self.data.endswith(b"\xff\xd9"):
                    raise ProtocolError("Missing JPEG markers")
                return bytes(self.data)
            else:
                raise ProtocolError("Unexpected frame response")
        except ValueError as error:
            raise ProtocolError("Malformed frame response") from error
        return None


class Camera:
    def __init__(self, serial, timeout=90):
        self.serial = serial
        self.timeout = timeout

    def responses(self, command):
        request_id = secrets.randbits(32)
        deadline = time.monotonic() + self.timeout
        # A newline also clears any incomplete command from an earlier session.
        self.serial.write(f"\n{PREFIX} {request_id} {command}\n".encode("ascii"), deadline)
        expected = f"{PREFIX} {request_id} ".encode("ascii")
        while True:
            line = self.serial.readline(deadline)
            if not line.startswith(expected):
                continue  # Boot logs and stale requests are not protocol responses.
            try:
                response = line[len(expected):].decode("ascii")
            except UnicodeDecodeError as error:
                raise ProtocolError("Non-ASCII protocol response") from error
            if response.startswith("ERR "):
                raise ProtocolError(response[4:])
            yield response

    def status(self):
        response = next(self.responses("STATUS"))
        if not response.startswith("STATUS "):
            raise ProtocolError("Expected board status")
        try:
            result = json.loads(response[7:])
        except ValueError as error:
            raise ProtocolError("Malformed board status") from error
        if not isinstance(result, dict) or result.get("protocol") != PREFIX:
            raise ProtocolError("Unsupported firmware protocol")
        return result

    def capture(self):
        frame = Frame()
        for response in self.responses("CAPTURE"):
            image = frame.feed(response)
            if image is not None:
                return image

    def configure_wifi(self, ssid, password):
        ssid_bytes, password_bytes = ssid.encode("utf-8"), password.encode("utf-8")
        if not 1 <= len(ssid_bytes) <= 32 or b"\0" in ssid_bytes:
            raise ValueError("SSID must be 1–32 UTF-8 bytes without NUL")
        if not 8 <= len(password_bytes) <= 63 or b"\0" in password_bytes:
            raise ValueError("WPA2 password must be 8–63 UTF-8 bytes without NUL")
        response = next(self.responses(f"WIFI_SET {ssid_bytes.hex()} {password_bytes.hex()}"))
        if response != "OK":
            raise ProtocolError("Wi-Fi settings were not acknowledged")

    def forget_wifi(self):
        if next(self.responses("WIFI_FORGET")) != "OK":
            raise ProtocolError("Wi-Fi removal was not acknowledged")


def resolve_port(explicit):
    if explicit:
        return explicit
    ports = sorted(set(glob.glob("/dev/cu.usbmodem*") + glob.glob("/dev/ttyACM*")))
    if len(ports) != 1:
        raise ValueError(f"Expected one USB serial device; use --port. Found: {ports}")
    return ports[0]


def save_frame(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as output:
        output.write(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", help="Auto-detected when exactly one USB serial device is attached")
    parser.add_argument("--timeout", type=float, default=90, help="Per-request timeout in seconds")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("status", help="Read camera readiness, firmware version, and Wi-Fi IP")
    capture = sub.add_parser("capture", help="Save one JPEG; refuse to overwrite an existing file")
    capture.add_argument("--output", type=Path, default=None)
    verify = sub.add_parser("verify", help="Capture several frames and write an integrity report")
    verify.add_argument("--count", type=int, default=3)
    verify.add_argument("--output-dir", type=Path, default=None)
    sub.add_parser("wifi-setup", help="Prompt locally for Wi-Fi name and hidden password")
    sub.add_parser("wifi-forget", help="Remove saved Wi-Fi settings and stop the radio")
    sub.add_parser("display-test", help="Show the TFT color test picture")
    sub.add_parser("display-github", help="Show GitHub activity on the TFT")
    sub.add_parser("github-setup", help="Store a separate read-only token using hidden local input")
    sub.add_parser("github-forget", help="Remove the board's GitHub token")
    args = parser.parse_args()
    if args.timeout <= 0 or (args.command == "verify" and not 1 <= args.count <= 100):
        parser.error("Use a positive timeout and a frame count from 1 to 100")
    port = resolve_port(args.port)
    serial = Serial(port)
    try:
        camera = Camera(serial, args.timeout)
        stamp = time.strftime("%Y%m%d-%H%M%S") + f"-{time.time_ns() % 1_000_000:06d}"
        if args.command == "status":
            print(json.dumps(camera.status(), indent=2))
        elif args.command in ("display-test", "display-github", "github-forget"):
            command = {"display-test": "DISPLAY_TEST", "display-github": "DISPLAY_GITHUB", "github-forget": "GITHUB_FORGET"}[args.command]
            if next(camera.responses(command)) != "OK":
                raise ProtocolError("Expected acknowledgement")
            print(json.dumps(camera.status(), indent=2))
        elif args.command == "github-setup":
            camera.status()
            print("Use a separate fine-grained token with public repositories only and no write permissions.")
            print("The token is stored in unencrypted NVS on this development board. Do not paste your main gh token.")
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                token = getpass.getpass("Fine-grained GitHub token (hidden): ").strip()
            if not token.startswith("github_pat_") or not 50 <= len(token) <= 100 or not all(c.isascii() and (c.isalnum() or c == "_") for c in token):
                raise ValueError("Expected a fine-grained github_pat_ token (50–100 ASCII characters)")
            if next(camera.responses("GITHUB_SET " + token.encode("ascii").hex())) != "OK":
                raise ProtocolError("Expected GitHub setup acknowledgement")
            del token
            if next(camera.responses("DISPLAY_GITHUB")) != "OK":
                raise ProtocolError("Expected display acknowledgement")
            print("Token saved. The board will fetch GitHub activity directly over HTTPS.", flush=True)
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                state = camera.status().get("github", {})
                if state.get("data"):
                    print(json.dumps(state, indent=2))
                    break
                time.sleep(2)
            else:
                raise TimeoutError("Token saved, but no activity received yet. Check USB status for clock, Wi-Fi or token errors.")
        elif args.command == "capture":
            path = args.output or Path("captures") / f"{stamp}.jpg"
            image = camera.capture()
            save_frame(path, image)
            print(f"Saved {path.resolve()} ({len(image)} bytes, CRC32 {binascii.crc32(image):08x})")
        elif args.command == "verify":
            status = camera.status()
            if not status.get("camera_ready"):
                raise ProtocolError("Camera initialization failed; inspect status")
            directory = args.output_dir or Path("captures") / stamp
            directory.mkdir(parents=True, exist_ok=False)
            report = {"transport": "usb", "status_before": status, "frames": [], "passed": False,
                      "checks": ["length", "ordered chunks", "CRC32", "JPEG markers"],
                      "limitation": "Integrity checks do not decode JPEG pixels or prove scene freshness."}
            try:
                for index in range(args.count):
                    started = time.monotonic()
                    image = camera.capture()
                    path = directory / f"frame-{index + 1:03d}.jpg"
                    save_frame(path, image)
                    report["frames"].append({"file": path.name, "bytes": len(image),
                        "crc32": f"{binascii.crc32(image):08x}", "seconds": round(time.monotonic() - started, 3)})
                report["status_after"] = camera.status()
                report["passed"] = True
            except Exception as error:
                report["error"] = str(error)
                raise
            finally:
                (directory / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(f"USB integrity checks passed: {len(report['frames'])} frames. {directory.resolve()}")
        elif args.command == "wifi-setup":
            # Fail closed if getpass cannot suppress echo; never accept a password argument.
            camera.status()
            ssid = input("Home Wi-Fi name (2.4 GHz): ")
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                password = getpass.getpass("Wi-Fi password (hidden): ")
            camera.configure_wifi(ssid, password)
            del password
            print("Settings saved on the board. Waiting up to 45 seconds for an address…", flush=True)
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                status = camera.status()
                if status.get("wifi", {}).get("connected") and status["wifi"].get("ip"):
                    print(f"Camera: http://{status['wifi']['ip']}")
                    break
                time.sleep(1)
            else:
                raise TimeoutError("Settings saved, but no Wi-Fi address yet. Check the 2.4 GHz network and password; USB capture remains available.")
        elif args.command == "wifi-forget":
            camera.forget_wifi()
            print("Saved Wi-Fi settings removed. USB capture remains available.")
    finally:
        serial.close()


if __name__ == "__main__":
    try:
        main()
    except (OSError, termios.error, ValueError, ProtocolError, getpass.GetPassWarning) as error:
        print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(130)
