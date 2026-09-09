#!/usr/bin/env python3
"""Exercise resolution and image controls on a connected board, then restore settings."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import struct
import time
import urllib.error
import urllib.request

from camera import Camera, Serial, resolve_port

SIZES = [("QVGA",320,240),("VGA",640,480),("SVGA",800,600),("XGA",1024,768),
         ("SXGA",1280,1024),("UXGA",1600,1200),("QXGA",2048,1536)]


def jpeg_info(data):
    assert data.startswith(b"\xff\xd8") and data.endswith(b"\xff\xd9")
    offset, size, tables = 2, None, bytearray()
    while offset + 4 <= len(data):
        assert data[offset] == 255
        while data[offset] == 255:
            offset += 1
        marker = data[offset]
        offset += 1
        if marker in (0xda, 0xd9):
            break
        length = struct.unpack_from(">H", data, offset)[0]
        assert length >= 2 and offset + length <= len(data)
        if marker in (0xc0, 0xc1, 0xc2):
            height, width = struct.unpack_from(">HH", data, offset + 3)
            size = (width, height)
        if marker == 0xdb:
            tables.extend(data[offset+2:offset+length])
        offset += length
    assert size and tables, "Missing JPEG size or quantization tables"
    return size, hashlib.sha256(tables).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    serial = Serial(resolve_port(args.port))
    camera = Camera(serial)
    report = {"passed":False,"frames":[],"checks":[]}
    base, original = None, None

    def request(path, value=None):
        body = json.dumps(value).encode() if value is not None else None
        req = urllib.request.Request(base+path,data=body,headers={"Content-Type":"application/json"} if body else {})
        with urllib.request.urlopen(req,timeout=30) as response:
            return response.read()

    def configure(value):
        return json.loads(request("/camera/settings",value))

    def capture(name, expected, usb=False):
        started = time.monotonic()
        data = camera.capture() if usb else request("/capture")
        size, dqt = jpeg_info(data)
        assert size == expected, (name,size,expected)
        assert len(data) <= 1024*1024
        (args.output_dir/f"{name}.jpg").write_bytes(data)
        info = {"name":name,"width":size[0],"height":size[1],"bytes":len(data),
                "seconds":round(time.monotonic()-started,3),"dqt_sha256":dqt,"transport":"usb" if usb else "http"}
        report["frames"].append(info)
        print(json.dumps(info),flush=True)
        return info

    try:
        report["before"] = camera.status()
        base = f"http://{report['before']['wifi']['ip']}"
        original = report["before"]["camera"]["settings"].copy()
        original.pop("width"); original.pop("height")
        for name,width,height in SIZES:
            current = configure({"framesize":name,"quality":12})
            assert (current["width"],current["height"]) == (width,height)
            capture(name,(width,height))
        report["checks"].append("All seven resolutions match actual JPEG SOF dimensions")

        configure({"framesize":"QXGA","quality":10})
        high = capture("QXGA-quality10",(2048,1536))
        configure({"quality":30})
        low = capture("QXGA-quality30",(2048,1536))
        assert high["dqt_sha256"] != low["dqt_sha256"], "Quality did not change JPEG quantization tables"
        report["checks"].append("JPEG quality changes the sensor's actual quantization tables")
        configure({"quality":10})
        with ThreadPoolExecutor(max_workers=1) as pool:
            http = pool.submit(capture,"QXGA-concurrent-http",(2048,1536))
            capture("QXGA-concurrent-usb",(2048,1536),usb=True)
            http.result()
        report["checks"].append("Concurrent maximum-resolution HTTP and USB capture")

        current = configure({"brightness":2,"contrast":-2,"saturation":1,"hmirror":True,"vflip":False})
        assert all(current[key] == value for key,value in {"brightness":2,"contrast":-2,"saturation":1,"hmirror":True,"vflip":False}.items())
        capture("controls-applied",(2048,1536))
        before_invalid = json.loads(request("/camera/settings"))["settings"]
        for invalid in [{"quality":8},{"quality":41},{"framesize":"BAD"},{"brightness":3},{"hmirror":1},{"unexpected":True}]:
            try:
                configure(invalid)
                raise AssertionError("Invalid settings accepted")
            except urllib.error.HTTPError as error:
                assert error.code == 400
            assert json.loads(request("/camera/settings"))["settings"] == before_invalid
        report["checks"].append("Image controls accepted; invalid batches return 400 and preserve prior settings")
        report["after"] = camera.status()
        assert report["after"]["uptime_seconds"] >= report["before"]["uptime_seconds"]
        assert report["after"]["display"]["ready"] and report["after"]["github"]["configured"]
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        if isinstance(error, urllib.error.HTTPError):
            report["error_response"] = error.read().decode(errors="replace")
        raise
    finally:
        if original:
            try:
                report["restored"] = configure(original)
            except Exception as error:
                report["restore_error"] = str(error)
                report["passed"] = False
        serial.close()
        (args.output_dir/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    if not report["passed"]:
        raise RuntimeError("Camera validation failed; see report")
    print("Camera controls checks passed.",flush=True)


if __name__ == "__main__":
    main()
