#!/usr/bin/env python3
"""Exercise playback suspension and camera demand allocation on attached hardware."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import urllib.error
import urllib.request
from camera import Camera, Serial, resolve_port
from verify_camera_controls import jpeg_info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', help='Board URL; otherwise discover its address over USB')
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    if args.url is None:
        serial = Serial(resolve_port(None))
        try: args.url = 'http://' + Camera(serial).status()['wifi']['ip']
        finally: serial.close()
    args.output_dir.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'checks': [], 'samples': {}}

    def req(path, value=None, post=False):
        data = json.dumps(value).encode() if value is not None else b'' if post else None
        with urllib.request.urlopen(urllib.request.Request(args.url+path, data=data,
                headers={'Content-Type': 'application/json'}), timeout=25) as response:
            return response.read()

    def status(): return json.loads(req('/status'))
    def post(path): return req(path, post=True)
    def sample(label):
        s = status(); report['samples'][label] = s
        return s
    def wait(predicate, label, timeout=90):
        print(label, flush=True)
        end = time.monotonic()+timeout
        while time.monotonic()<end:
            s = status()
            if predicate(s): return s
            time.sleep(.3)
        raise TimeoutError(label)
    def idle(s): return not s['camera']['active']
    def rejected(path):
        try: req(path); raise AssertionError('Paused preview accepted')
        except urllib.error.HTTPError as e: assert e.code == 409
    def check(label): report['checks'].append(label); print('PASS: '+label, flush=True)

    before = sample('before')
    assert before['firmware'] == '0.7.0'
    saved_hash = hashlib.sha256(req('/display/saved')).hexdigest()
    original_album = {k: before['album'][k] for k in ('url','interval_seconds')}
    assert original_album['url']
    original_mode = before['display']['mode']
    original_paused = before['album']['paused']
    try:
        post('/camera/preview/stop')
        post('/display/test')
        wait(idle, 'Camera becomes idle')
        for i in range(3):
            body=req('/capture')
            (args.output_dir/f'wakeup-{i}.jpg').write_bytes(body)
            dimensions,_=jpeg_info(body)
            assert dimensions==(before['camera']['settings']['width'],before['camera']['settings']['height'])
            active=sample(f'camera-active-{i}')
            assert active['camera']['active']
            asleep=wait(idle,'Camera releases buffers after capture')
            report['samples'][f'camera-idle-{i}']=asleep
            assert asleep['camera']['free_psram_bytes']-active['camera']['free_psram_bytes'] > 900_000
        check('Three fresh camera wake/capture/suspend cycles preserve settings and release over 900 KB of PSRAM each')
        session=json.loads(post('/camera/preview/start'))['session']
        req('/capture?preview='+str(session))
        post('/display/github')
        rejected('/capture?preview='+str(session))
        rejected('/capture?t=legacy-page')
        wait(idle,'Camera suspends after display mode change')
        first=sample('camera-paused')
        time.sleep(6)
        assert status()['camera']['snapshots']==first['camera']['snapshots']
        check('Mode change invalidates preview; old browser polling cannot restart capture')
        session=json.loads(post('/camera/preview/start'))['session']
        req('/capture?preview='+str(session))
        time.sleep(6)
        rejected('/capture?preview='+str(session))
        assert idle(status())
        check('An abandoned preview session expires and camera remains idle')
        serial=Serial(resolve_port(None))
        try: (args.output_dir/'usb-wakeup.jpg').write_bytes(Camera(serial).capture())
        finally: serial.close()
        wait(idle,'Camera suspends after USB capture')
        check('USB capture wakes the idle camera and releases resources afterwards')

        req('/album/settings', {**original_album,'interval_seconds':15})
        wait(lambda s:s['album']['current']>0 and not s['album']['loading'] and not s['album']['last_error'],'Album photo displayed')
        post('/album/pause')
        time.sleep(.5)
        held=sample('album-paused'); held_frame=req('/display/frame')
        print('Observe pause beyond a full slide interval',flush=True)
        time.sleep(18)
        after=sample('album-paused-after')
        for key in ['current','downloads_started','remaining_ms']:
            assert after['album'][key]==held['album'][key],key
        assert req('/display/frame')==held_frame and after['display']['frames_written']==held['display']['frames_written']
        assert after['camera']['snapshots']==held['camera']['snapshots'] and idle(after)
        check('Pause holds frame/time with no downloads, redraws or camera activity beyond the slide interval')
        index=held['album']['current'];count=held['album']['photos']
        previous=(index-2)%count+1
        post('/album/previous')
        wait(lambda s:s['album']['current']==previous and not s['album']['loading'],'Previous photo, including wraparound')
        assert status()['album']['paused']
        post('/album/next')
        wait(lambda s:s['album']['current']==index and not s['album']['loading'],'Next returns to original photo')
        assert status()['album']['paused'] and req('/display/frame')==held_frame
        check('Previous/Next work while paused, wrap around, and return identical source pixels')

        post('/album/resume');time.sleep(3)
        current=req('/display/frame')
        post('/display/github');time.sleep(.5)
        hidden=sample('album-hidden')
        print('Observe other mode beyond a full slide interval',flush=True)
        time.sleep(18)
        away=sample('album-hidden-after')
        for key in ['current','downloads_started','remaining_ms']:
            assert away['album'][key]==hidden['album'][key],key
        assert idle(away) and away['camera']['snapshots']==hidden['camera']['snapshots']
        post('/display/album')
        restored=sample('album-restored')
        assert req('/display/frame')==current
        assert restored['album']['current']==index and restored['album']['downloads_started']==hidden['album']['downloads_started']
        remaining=hidden['album']['remaining_ms']
        assert remaining > 5000
        time.sleep(2)
        assert status()['album']['current']==index
        expected=index%count+1
        wait(lambda s:s['album']['current']==expected,'Playback continues when remaining time elapses',45)
        check('Other modes freeze album work/time; return restores same photo before continuing the remaining interval')

        post('/album/next')
        loading=wait(lambda s:s['album']['loading'],'Catch an in-flight photo download',15)
        frozen_index=loading['album']['current'];frozen_frame=req('/display/frame')
        post('/album/pause')
        stopped=wait(lambda s:not s['album']['loading'],'In-flight download cancels after pause',25)
        assert stopped['album']['current']==frozen_index and req('/display/frame')==frozen_frame
        assert stopped['album']['cancelled_downloads']>loading['album']['cancelled_downloads']
        check('Pausing during download discards the pending frame and retains the visible photo')
        assert hashlib.sha256(req('/display/saved')).hexdigest()==saved_hash
        check('Manual saved photo is unchanged by all playback and camera checks')
        report['passed']=True
    except Exception as error:
        report['error']=repr(error)
        raise
    finally:
        try:
            post('/camera/preview/stop')
            req('/album/settings', original_album)
            post('/album/pause' if original_paused else '/album/resume')
            post('/display/'+('restore' if original_mode=='image' else original_mode))
            report['restored']=status()
        except Exception as error:
            report['restore_error']=repr(error);report['passed']=False
        (args.output_dir/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    if not report['passed']:raise RuntimeError('Restore failed')
    print('Playback and resource checks passed.',flush=True)


if __name__=='__main__':main()
