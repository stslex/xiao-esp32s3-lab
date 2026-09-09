import binascii
import json
import os
import pty
import select
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import camera

# A transport fixture, deliberately not a decodable photograph.
JPEG = b"\xff\xd8" + bytes(range(256)) * 3 + b"\xff\xd9"


def frame_lines(image=JPEG):
    return [f"BEGIN {len(image)} {binascii.crc32(image):08x}"] + [
        f"DATA {offset} {image[offset:offset + 256].hex()}"
        for offset in range(0, len(image), 256)
    ] + ["END"]


def receive(lines):
    frame = camera.Frame()
    result = None
    for line in lines:
        result = frame.feed(line)
    return result


class FakeSerial:
    def __init__(self, responses):
        self.responses = responses
        self.command = None
        self.lines = []

    def write(self, data, deadline):
        _, request, self.command = data.decode().strip().split(" ", 2)
        self.lines = [b"I (123) camera: boot log", b"XIAO1 stale END"] + [
            f"XIAO1 {request} {line}".encode() for line in self.responses
        ]

    def readline(self, deadline):
        if not self.lines:
            raise TimeoutError("Incomplete response")
        return self.lines.pop(0)


class ProtocolTests(unittest.TestCase):
    def test_capture_ignores_logs_and_other_request_ids(self):
        self.assertEqual(camera.Camera(FakeSerial(frame_lines())).capture(), JPEG)

    def test_corrupted_payload_rejected_by_crc(self):
        lines = frame_lines()
        lines[1] = lines[1][:-2] + "00"
        with self.assertRaisesRegex(camera.ProtocolError, "CRC32"):
            receive(lines)

    def test_missing_duplicate_and_reordered_chunks_rejected(self):
        original = frame_lines()
        variants = [original[:1] + original[2:], original[:2] + original[1:],
                    [original[0], original[2], original[1], *original[3:]]]
        for lines in variants:
            with self.subTest(lines=lines[:1]), self.assertRaises(camera.ProtocolError):
                receive(lines)

    def test_early_end_and_oversized_frame_rejected(self):
        for lines in [["BEGIN 8 00000000", "END"], [f"BEGIN {camera.MAX_FRAME + 1} 00000000"]]:
            with self.assertRaises(camera.ProtocolError):
                receive(lines)

    def test_incomplete_transfer_times_out_instead_of_returning_image(self):
        with self.assertRaises(TimeoutError):
            camera.Camera(FakeSerial(frame_lines()[:-1])).capture()

    def test_invalid_jpeg_markers_rejected_even_with_valid_crc(self):
        with self.assertRaisesRegex(camera.ProtocolError, "JPEG markers"):
            receive(frame_lines(b"not a jpeg"))

    def test_board_errors_and_wrong_protocol_fail(self):
        with self.assertRaisesRegex(camera.ProtocolError, "camera_not_ready"):
            camera.Camera(FakeSerial(["ERR camera_not_ready"])).capture()
        with self.assertRaisesRegex(camera.ProtocolError, "Unsupported"):
            camera.Camera(FakeSerial(['STATUS {"protocol":"future"}'])).status()

    def test_wifi_credentials_are_encoded_and_not_command_fragments(self):
        serial = FakeSerial(["OK"])
        camera.Camera(serial).configure_wifi("Home сеть", "a b\nc def")
        name, password = serial.command.split()[1:]
        self.assertEqual(bytes.fromhex(name).decode(), "Home сеть")
        self.assertEqual(bytes.fromhex(password).decode(), "a b\nc def")
        self.assertNotIn("a b\nc def", serial.command)

    def test_invalid_credentials_are_not_transmitted(self):
        serial = FakeSerial(["OK"])
        for ssid, password in [("", "12345678"), ("я" * 17, "12345678"), ("Home", "short"), ("Home", "a\0bcdefg")]:
            with self.assertRaises(ValueError):
                camera.Camera(serial).configure_wifi(ssid, password)
        self.assertIsNone(serial.command)

    def test_existing_capture_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture.jpg"
            path.write_bytes(b"existing")
            with self.assertRaises(FileExistsError):
                camera.save_frame(path, JPEG)
            self.assertEqual(path.read_bytes(), b"existing")

    def test_ambiguous_ports_require_explicit_selection(self):
        with patch("camera.glob.glob", return_value=["/dev/a", "/dev/b"]):
            with self.assertRaisesRegex(ValueError, "use --port"):
                camera.resolve_port(None)

    def test_serial_transport_over_real_pseudoterminal(self):
        master, slave = pty.openpty()
        serial = camera.Serial(os.ttyname(slave))
        errors = []

        def board():
            try:
                command = b""
                deadline = time.monotonic() + 3
                while not command.strip().endswith(b"CAPTURE"):
                    remaining = deadline - time.monotonic()
                    if remaining <= 0 or not select.select([master], [], [], remaining)[0]:
                        raise TimeoutError("No host command")
                    command += os.read(master, 1024)
                _, request, _ = command.decode().strip().split()
                payload = b"Boot log without a newline"
                for line in frame_lines():
                    payload += f"\r\nXIAO1 {request} {line}\r\n".encode()
                for offset in range(0, len(payload), 17):
                    os.write(master, payload[offset:offset + 17])
            except BaseException as error:
                errors.append(error)

        worker = threading.Thread(target=board)
        worker.start()
        try:
            self.assertEqual(camera.Camera(serial, timeout=3).capture(), JPEG)
        finally:
            worker.join(timeout=4)
            serial.close()
            os.close(master)
            os.close(slave)
        self.assertFalse(worker.is_alive())
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
