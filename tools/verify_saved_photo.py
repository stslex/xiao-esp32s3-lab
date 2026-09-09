#!/usr/bin/env python3
"""Verify saved-photo retention and autonomous album playback on the connected board."""
import argparse
import binascii
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request
from camera import Camera, Serial, resolve_port


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', required=True)
    parser.add_argument('--image', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    port = resolve_port(None)
    serial = Serial(port)
    try: initial = Camera(serial).status()
    finally: serial.close()
    base = 'http://' + initial['wifi']['ip']
    saved = args.image.read_bytes()
    assert len(saved) == 153600
    report = {'passed': False, 'checks': [], 'before': initial}

    def request(path, body=None, value=None):
        if value is not None: body = json.dumps(value).encode()
        headers = {'Content-Type':'application/json' if value is not None else 'application/octet-stream'} if body is not None else {}
        with urllib.request.urlopen(urllib.request.Request(base+path,data=body,headers=headers),timeout=25) as response:
            return response.read()

    def state(): return json.loads(request('/status'))

    def wait_for(predicate, label, timeout=150):
        print('Waiting: '+label, flush=True)
        deadline = time.monotonic()+timeout
        last = None
        while time.monotonic()<deadline:
            try:
                last = state()
                if predicate(last): return last
            except (OSError, ValueError): pass
            time.sleep(2)
        raise TimeoutError(label+': '+json.dumps(last and last.get('album')))

    def reset():
        subprocess.run(['espflash','reset','--port',port,'--non-interactive','--skip-update-check'],check=True)

    try:
        request('/display/image',saved)
        assert request('/display/saved') == saved
        for mode in ['github','test']:
            request('/display/'+mode,b'')
            assert request('/display/saved') == saved
            request('/display/restore',b'')
            assert request('/display/frame') == saved
        report['checks'].append('Photo retained across GitHub/test modes and restored byte-for-byte')
        try:
            request('/display/image',b'bad')
            raise AssertionError('Invalid upload accepted')
        except urllib.error.HTTPError as e: assert e.code == 400
        assert request('/display/saved') == saved
        before_reset = state()
        reset()
        restored = wait_for(lambda s:s['uptime_seconds']<before_reset['uptime_seconds'] and s['display']['mode']=='image','photo restored after reset')
        assert request('/display/frame') == saved and request('/display/saved') == saved
        report['image_after_reset'] = restored
        report['checks'].append('Image bytes and selected image mode survive hardware reset')

        request('/album/settings',value={'url':args.url,'interval_seconds':15})
        first = wait_for(lambda s:s['album']['current']>0 and not s['album']['last_error'],'first autonomous album photo')
        (args.output_dir/'album-first.rgb565').write_bytes(request('/display/frame'))
        started = time.monotonic()
        second = wait_for(lambda s:s['album']['current']!=first['album']['current'] and not s['album']['last_error'],'automatic scheduled next photo')
        report['scheduled_change_seconds'] = round(time.monotonic()-started,2)
        (args.output_dir/'album-second.rgb565').write_bytes(request('/display/frame'))
        assert first['display']['frame_crc32'] != second['display']['frame_crc32']
        assert request('/display/saved') == saved
        report['first'], report['second'] = first, second
        report['checks'].append('Two distinct album frames delivered by board timer without a host uploader')
        request('/album/next',b'')
        third = wait_for(lambda s:s['album']['current']!=second['album']['current'] and not s['album']['last_error'],'Next photo button API')
        report['third'] = third
        request('/display/restore',b'')
        time.sleep(18)
        assert state()['display']['mode']=='image' and request('/display/frame')==saved
        report['checks'].append('Saved photo remains intact; album does not overwrite another selected mode')

        request('/album/settings',value={'url':args.url,'interval_seconds':60})
        before_reset=state()
        reset()
        final=wait_for(lambda s:s['uptime_seconds']<before_reset['uptime_seconds'] and s['display']['mode']=='album' and s['album']['current']>0 and not s['album']['last_error'],'album configuration and playback after reset')
        assert final['album']['interval_seconds']==60
        assert request('/display/saved')==saved
        assert final['camera_ready'] and final['display']['ready']
        (args.output_dir/'album-final.rgb565').write_bytes(request('/display/frame'))
        serial=Serial(port)
        try:
            camera=Camera(serial)
            jpeg=camera.capture()
            report['final_usb_status']=camera.status()
        finally: serial.close()
        (args.output_dir/'camera-final.jpg').write_bytes(jpeg)
        report['final']=final
        report['saved_crc32']=f'{binascii.crc32(saved):08x}'
        report['checks'].append('Album URL, 60-second interval and album mode survive reset; USB camera works')
        report['passed']=True
        print('Saved photo and album checks passed.',flush=True)
    except Exception as error:
        report['error']=str(error)
        if isinstance(error,urllib.error.HTTPError): report['response']=error.read().decode(errors='replace')
        raise
    finally:
        (args.output_dir/'report.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__': main()
