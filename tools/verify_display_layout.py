#!/usr/bin/env python3
"""Exercise orientation/crop/source retention on hardware, then restore user state."""
import argparse
import binascii
import json
from pathlib import Path
import struct
import subprocess
import time
import urllib.error
import urllib.request
import zlib
from camera import Camera, Serial, resolve_port


def png(raw, width, height, path):
    scan = bytearray()
    for y in range(height):
        scan.append(0)
        for x in range(width):
            c = int.from_bytes(raw[(y*width+x)*2:(y*width+x+1)*2], 'big')
            scan.extend((((c >> 11) & 31)*255//31, ((c >> 5) & 63)*255//63, (c & 31)*255//31))
    def chunk(tag, data):
        return struct.pack('>I', len(data))+tag+data+struct.pack('>I', binascii.crc32(tag+data))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))+chunk(b'IDAT', zlib.compress(scan))+chunk(b'IEND', b''))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    port = resolve_port(None)
    serial = Serial(port)
    try: before = Camera(serial).status()
    finally: serial.close()
    base = 'http://'+before['wifi']['ip']
    report = {'passed': False, 'before': before, 'checks': [], 'frames': []}

    def request(path, body=None, value=None, headers=None):
        if value is not None: body = json.dumps(value).encode()
        headers = dict(headers or {})
        if body is not None: headers['Content-Type'] = 'application/json' if value is not None else 'application/octet-stream'
        with urllib.request.urlopen(urllib.request.Request(base+path, data=body, headers=headers), timeout=25) as r:
            return r.read(), r.headers

    def status(): return json.loads(request('/status')[0])
    def layout(value): return json.loads(request('/display/settings', value=value)[0])
    def pixel(b,w,x,y): return int.from_bytes(b[(y*w+x)*2:(y*w+x+1)*2], 'big')
    def snapshot(name, expected_dimensions=None):
        b,h = request('/display/frame')
        w,hh = int(h['X-Image-Width']), int(h['X-Image-Height'])
        assert len(b) == w*hh*2 == 153600
        if expected_dimensions is not None:
            assert (w,hh) == expected_dimensions, 'Display orientation changed during the check'
        png(b,w,hh,args.output_dir/(name+'.png'))
        s = status()
        assert s['display']['frame_crc32'] == f'{binascii.crc32(b):08x}'
        report['frames'].append({'name':name,'width':w,'height':hh,'crc':s['display']['frame_crc32'],'wire_crc':s['display']['wire_crc32']})
        return b,w,hh,s

    def wait_for(predicate, label):
        print('Waiting: '+label, flush=True)
        deadline = time.monotonic()+120
        while time.monotonic()<deadline:
            try:
                s=status()
                if predicate(s): return s
            except (OSError, ValueError): pass
            time.sleep(2)
        raise TimeoutError(label)

    saved, saved_headers = request('/display/saved')
    original_layout = before['display']['layout']
    original_mode = before['display']['mode']
    restore_headers = None if (saved_headers['X-Image-Width'],saved_headers['X-Image-Height'])==('240','320') else {'X-Image-Width':saved_headers['X-Image-Width'],'X-Image-Height':saved_headers['X-Image-Height']}
    (args.output_dir/'saved-before.rgb565').write_bytes(saved)
    source = None
    try:
        request('/display/restore', b'')
        assert request('/display/saved')[0] == saved
        report['checks'].append('Existing saved photo remains readable after the firmware migration')
        source = bytearray()
        for y in range(180):
            for x in range(400):
                color = 0xffe0 if y<20 else 0x07ff if y>=160 else (0xf800,0x07e0,0x001f,0xffff)[x//100]
                source.extend(color.to_bytes(2,'big'))
        request('/display/image', source, headers={'X-Image-Width':'400','X-Image-Height':'180'})
        original_source = request('/display/saved')
        assert original_source[0] == source and original_source[1]['X-Image-Width']=='400'
        layout({'rotation':0,'fit':'contain','focus_x':50,'focus_y':50})
        b,w,h,s = snapshot('portrait-fit')
        assert (w,h)==(240,320) and pixel(b,w,0,0)==0
        assert pixel(b,w,0,160)==0xf800 and pixel(b,w,239,160)==0xffff
        layout({'fit':'cover','focus_x':0})
        left,w,h,s = snapshot('portrait-crop-left')
        assert pixel(left,w,0,160)==0xf800
        layout({'focus_x':100})
        right,w,h,s = snapshot('portrait-crop-right')
        assert left!=right and pixel(right,w,0,160)==0x001f and pixel(right,w,239,160)==0xffff
        for rotation in [90,270]:
            layout({'rotation':rotation,'fit':'contain'})
            b,w,h,s = snapshot('landscape-'+str(rotation))
            assert (w,h)==(320,240) and pixel(b,w,0,0)==0
            assert pixel(b,w,0,120)==0xf800 and pixel(b,w,319,120)==0xffff
            rows = [[b[(y*w+x)*2:(y*w+x+1)*2] for x in range(w)] for y in range(h)]
            physical = b''.join(rows[239-x][y] if rotation==90 else rows[x][319-y] for y in range(320) for x in range(240))
            assert s['display']['wire_crc32']==f'{binascii.crc32(physical):08x}'
        assert request('/display/saved')[0] == source
        report['checks'].append('Portrait fit/crop and horizontal focus have known pixels; both physical rotations match independent CRC calculations; source unchanged')

        previous=status()['display']
        for invalid in [{'rotation':180},{'focus_y':101},{'focus_x':-1},{'fit':'stretch'},{'unknown':True}]:
            try: request('/display/settings',value=invalid); raise AssertionError('Invalid settings accepted')
            except urllib.error.HTTPError as e: assert e.code==400
            current=status()['display']
            assert current['layout']==previous['layout'] and current['frames_written']==previous['frames_written']
        report['checks'].append('Invalid layout patches reject before drawing or changing settings')
        target={'rotation':90,'fit':'cover','focus_x':25,'focus_y':75}
        layout(target)
        expected=request('/display/frame')[0]
        uptime=status()['uptime_seconds']
        subprocess.run(['espflash','reset','--port',port,'--non-interactive','--skip-update-check'],check=True)
        s=wait_for(lambda s:s['uptime_seconds']<uptime and s['display']['mode']=='image','layout and source after reset')
        assert s['display']['layout']==target
        assert request('/display/frame')[0]==expected and request('/display/saved')[0]==source
        report['checks'].append('Landscape/crop/focus, source dimensions and displayed pixels survive reset')

        # Restore the user's manual photo before checking the album renderer.
        request('/display/image',saved,headers=restore_headers)
        if before['album']['configured']:
            layout({'rotation':0,'fit':'contain','focus_x':50,'focus_y':50})
            request('/display/album',b'')
            wait_for(lambda s:s['album']['current']>0 and s['album']['last_error'] is None,'album source decoded')
            snapshot('album-portrait-fit', (240,320))
            layout({'fit':'cover'})
            snapshot('album-portrait-crop', (240,320))
            layout({'rotation':90,'fit':'contain'})
            snapshot('album-landscape-fit', (320,240))
            layout({'fit':'cover'})
            snapshot('album-landscape-crop', (320,240))
            assert request('/display/saved')[0]==saved
            report['checks'].append('Real album frame reflows immediately across portrait/landscape and fit/crop; manual photo preserved')
        serial=Serial(port)
        try: (args.output_dir/'camera.jpg').write_bytes(Camera(serial).capture())
        finally: serial.close()
        report['checks'].append('USB camera capture remains available during display/album use')
        report['passed']=True
    except Exception as e:
        report['error']=repr(e)
        if isinstance(e,urllib.error.HTTPError): report['response']=e.read().decode(errors='replace')
        raise
    finally:
        try:
            current_saved=request('/display/saved')[0]
            if current_saved == source:
                request('/display/image',saved,headers=restore_headers)
            elif current_saved != saved:
                raise RuntimeError('Another photo was selected during the test; retaining it without restoring old state')
            layout(original_layout)
            request('/display/'+('restore' if original_mode=='image' else original_mode),b'')
            report['restored']=status()
        except Exception as e: report['restore_error']=str(e);report['passed']=False
        (args.output_dir/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    if not report['passed']: raise RuntimeError('Failed to restore original state')
    print('Display layout checks passed.',flush=True)


if __name__=='__main__': main()
