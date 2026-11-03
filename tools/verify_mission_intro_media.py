"""Observe Basic/Intro natural playback and task return in the ordinary client.

Uses normal campaign menu input, captured client pixels and WASAPI loopback.
Requires Windows, FFmpeg, numpy and PyAudioWPatch for local verification only.
No Escape, engine capture adapter, private playback call or state injection.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from verify_mission_movie_audio import capture, decode
from verify_mission_results import Font
from verify_mission_save_browser import BrowserWindow


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class IntroWindow(BrowserWindow):
    def __init__(self, pid):
        super().__init__(pid)
        if self.u.GetForegroundWindow() == self.handle:
            return
        # Windows can deny SetForegroundWindow. An ordinary title-bar click
        # activates the owned window, but only if that exact window is on top.
        rect = w.RECT()
        self.u.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
        self.u.GetWindowRect(self.handle, c.byref(rect))
        point = w.POINT((rect.left + rect.right) // 2, rect.top + 10)
        self.u.WindowFromPoint.argtypes, self.u.WindowFromPoint.restype = [w.POINT], w.HWND
        covering = self.u.WindowFromPoint(point)
        if covering != self.handle:
            title = c.create_unicode_buffer(256)
            self.u.GetWindowTextW(covering, title, len(title))
            self.u.PostMessageW(self.handle, 0x10, 0, 0)
            raise RuntimeError(f'owned title bar is obscured by {title.value!r}; cannot activate the client')
        self.u.SetCursorPos(point.x, point.y)
        self.u.mouse_event(2, 0, 0, 0, 0)
        time.sleep(0.1)
        self.u.mouse_event(4, 0, 0, 0, 0)
        time.sleep(0.3)
        self.check()


def observe(np, font, references, raw):
    pixels = np.frombuffer(raw, dtype=np.uint8).reshape(736, 1000, 4)
    # The ordinary fullscreen movie fits 140x110 to 937x736 at x=31.
    # Sample pixel centres in the upper half, clear of the subtitle band.
    xs = 31 + ((np.arange(140) + 0.5) * 937 / 140).astype(int)
    ys = ((np.arange(55) + 0.5) * 736 / 110).astype(int)
    video = pixels[ys[:, None], xs, :3][:, :, ::-1].astype(np.float32).ravel()
    video -= video.mean()
    correlations = references @ video / max(float(np.linalg.norm(video)), 1e-8)
    index = int(np.argmax(correlations))
    cyan = (pixels[:, :, 0] > 150) & (pixels[:, :, 1] > 80) & (pixels[:, :, 2] < 80)
    yellow = (pixels[:, :, 2] > 200) & (pixels[:, :, 1] > 95) & (pixels[:, :, 0] < 220)
    # Retain the same unclipped right-hand glyph patch throughout all four cues.
    subtitle = (pixels[640:715, 750:815, :3].min(axis=2) >= 250)
    return {
        'reference_frame': index,
        'video_correlation': float(correlations[index]),
        'subtitle_white_pixels': int(subtitle.sum()),
        'subtitle_mask_sha256': hashlib.sha256(subtitle.tobytes()).hexdigest(),
        'countdown_title': font.find(yellow[670:705, :832], 'SDK intro countdown'),
        'finished_message': font.find(cyan[:110, :810], 'SDK intro finished'),
        'running_message': font.find(cyan[:110, :810], 'SDK intro running'),
    }


def run_window(np, references, client, fixture, output, control):
    output.mkdir(parents=True)
    font = Font(np, fixture / 'game.fnt')
    settings = fixture / 'settings/VERA.INI'
    original = settings.read_bytes() if settings.exists() else None
    settings.parent.mkdir(exist_ok=True)
    settings.write_text('[Intro]\nPlay=no\n[VERA]\nFullScreen=no\nSkipStartupLogo=yes\nLockCursor=no\n', encoding='utf-8')
    child, window, samples = None, None, []
    try:
        with (output / 'client.log').open('wb') as log:
            child = subprocess.Popen([str(client), '-WIN'], cwd=fixture, stdout=log, stderr=log,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            time.sleep(8)
            window = IntroWindow(child.pid)
            window.capture(output / 'menu.png')
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / 'single-player.png')
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / 'campaign.png')
            window.click(395, 535)
            started = time.perf_counter()
            # Do not send any further input while the intro ends and task runs.
            while time.perf_counter() - started < 14:
                name = f'frame-{len(samples):03d}.png'
                observed = time.perf_counter()
                raw = window.capture(output / name)
                samples.append({'time': observed - started, 'path': name,
                                'bgra_sha256': hashlib.sha256(raw).hexdigest(),
                                'png_sha256': sha(output / name), **observe(np, font, references, raw)})
                time.sleep(0.1)
    finally:
        if window and child and child.poll() is None:
            window.u.PostMessageW(window.handle, 0x10, 0, 0)
        if child:
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=10)
        if original is None:
            settings.unlink(missing_ok=True)
        else:
            settings.write_bytes(original)
        (output / 'cleanup.json').write_text(json.dumps({
            'client_exit_code': child.returncode if child else None,
            'settings_restored': (settings.read_bytes() if settings.exists() else None) == original,
        }, indent=2), encoding='utf-8')
        (output / 'frames.json').write_text(json.dumps(samples, indent=2), encoding='utf-8')
        source_log = fixture / 'logs/ra2.log'
        if source_log.exists():
            (output / 'runtime.log').write_bytes(source_log.read_bytes())

    movie = [i for i, s in enumerate(samples) if s['video_correlation'] >= 0.9]
    game = [i for i, s in enumerate(samples) if s['countdown_title']]
    subtitles = []
    for i in movie:
        row = samples[i]
        if row['subtitle_white_pixels'] > 1000 and (not subtitles or subtitles[-1]['sha256'] != row['subtitle_mask_sha256']):
            subtitles.append({'frame': i, 'time': row['time'], 'sha256': row['subtitle_mask_sha256']})
    gap = max(b['time'] - a['time'] for a, b in zip(samples, samples[1:]))
    checks = {
        'clean_exit': child.returncode == 0,
        'settings_restored': (settings.read_bytes() if settings.exists() else None) == original,
        'sampling_gap_under_half_second': gap < 0.5,
        'task_countdown_title_visible': bool(game),
        'task_first_and_subsequent_rule_messages_visible': any(s['finished_message'] for s in samples) and any(s['running_message'] for s in samples),
        'movie_presence_matches_intro_control': bool(movie) != control,
        'subtitle_presence_matches_intro_control': bool(subtitles) != control,
    }
    if not control:
        duration = samples[movie[-1]]['time'] - samples[movie[0]]['time'] if movie else 0
        checks.update({
            'three_seconds_of_independently_matched_movie_pixels': 2.5 <= duration <= 3.4,
            'multiple_decoded_frames_visible': len({samples[i]['reference_frame'] for i in movie}) >= 10,
            'late_movie_frames_visible_before_natural_return': bool(movie) and max(samples[i]['reference_frame'] for i in movie) >= 40,
            'four_distinct_ordered_subtitle_masks': len(subtitles) == 4 and len({s['sha256'] for s in subtitles}) == 4,
            'intro_finishes_before_task_countdown_appears': bool(movie) and bool(game) and movie[-1] < game[0],
            'task_still_runs_after_intro': bool(movie) and any(s['running_message'] and s['countdown_title'] for s in samples[movie[-1] + 1:]),
        })
    return {'passed': all(checks.values()), 'checks': checks, 'control': control,
            'observation_origin_perf_counter': started, 'movie_frames': movie,
            'subtitle_masks': subtitles, 'first_game_frame': game[0] if game else None,
            'max_sampling_gap_seconds': gap,
            'inputs': {str(p): sha(p) for p in [client, fixture / 'mission.ts', fixture / 'sdk.ts',
                fixture / 'SOV01UMD.MAP', fixture / 'install/SOV01UMD.MAP', fixture / 'subtitlemd.txt',
                fixture / 'game.fnt', fixture / 'config.toml']},
            'scope': 'Ordinary campaign Intro, independent RGB reference frames, four known subtitle masks and GAME.FNT task messages. No playback skip or engine diagnostics. Subtitle mask order is not general OCR; no original-game pixel equality.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['client', 'fixture', 'control-fixture', 'ffmpeg', 'python-deps', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'win32' or args.output.exists():
        parser.error('Windows and a new output directory required')
    sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    import pyaudiowpatch as pa

    sources = [args.fixture.resolve(), args.control_fixture.resolve()]
    for source, control in zip(sources, [False, True]):
        text = (source / 'mission.ts').read_text(encoding='utf-8')
        build = json.loads((source / 'intro-build.json').read_text())
        runtime = json.loads((source / 'intro-runtime.json').read_text())
        if not build['passed'] or build['control'] != control or ('intro:m.movie(Movie.A01_P01e)' in text) == control:
            parser.error('expected intro-media and intro-media-control sources')
        if not runtime['passed']:
            parser.error('task runtime checks failed')
        if sha(source / 'SOV01UMD.MAP') != sha(source / 'install/SOV01UMD.MAP'):
            parser.error('installed map differs')
    bik, ffmpeg = sources[0] / 'A01_P01e.bik', args.ffmpeg.resolve()
    if sha(bik) != sha(sources[1] / bik.name):
        parser.error('reference movies differ')
    video = subprocess.run([str(ffmpeg), '-v', 'error', '-nostdin', '-i', str(bik), '-map', '0:v:0',
                            '-pix_fmt', 'rgb24', '-f', 'rawvideo', 'pipe:1'], capture_output=True, check=True).stdout
    frames = np.frombuffer(video, dtype=np.uint8).reshape(45, 110, 140, 3)[:, :55].reshape(45, -1).astype(np.float32)
    frames -= frames.mean(axis=1, keepdims=True)
    references = frames / np.maximum(np.linalg.norm(frames, axis=1, keepdims=True), 1e-8)
    pcm = decode(ffmpeg, ['-i', str(bik), '-map', '0:a:0'])
    reference = np.frombuffer(pcm, dtype='<f4')
    args.output.mkdir(parents=True)
    (args.output / 'reference.rgb').write_bytes(video)
    (args.output / 'reference.f32').write_bytes(pcm)
    os.environ['RUST_LOG'] = 'info'
    runs = []
    error = None
    for source, name, control in zip(sources, ['intro', 'control'], [False, True]):
        def runner(client, fixture, output, control):
            return run_window(np, references, client, fixture, output, control)
        try:
            result = capture(np, pa, args.client.resolve(), source, args.output / name, control, ffmpeg, reference, window_runner=runner)
        except Exception as exc:
            cleanup = args.output / name / 'cleanup.json'
            error = {'run': name, 'type': type(exc).__name__, 'message': str(exc),
                     'cleanup': json.loads(cleanup.read_text()) if cleanup.exists() else None}
            print(json.dumps({'error': error}), flush=True)
            break
        runs.append(result)
        print(json.dumps({'run': name, 'passed': result['passed'],
                          'failed': [k for k, v in result['checks'].items() if not v],
                          'window_failed': [k for k, v in result['window']['checks'].items() if not v]}), flush=True)
    report = {'passed': len(runs) == 2 and all(r['passed'] for r in runs),
              'error': error, 'runs': dict(zip(['intro', 'control'], runs)),
              'fixture_checks': {str(source): {'build': json.loads((source / 'intro-build.json').read_text()),
                                              'runtime': json.loads((source / 'intro-runtime.json').read_text())['checks']}
                                 for source in sources},
              'fingerprints': {str(p.resolve()): sha(p) for p in [Path(__file__), args.client, ffmpeg, bik,
                  Path(__file__).with_name('verify_mission_movie_audio.py'), Path(__file__).with_name('verify_mission_movie_window.py'),
                  Path(__file__).with_name('verify_mission_save_browser.py'), Path(__file__).with_name('verify_mission_results.py'),
                  args.output / 'reference.rgb', args.output / 'reference.f32']},
              'scope': 'Basic/Intro natural playback through the ordinary campaign menu, independent RGB decode and actual WASAPI soundtrack fingerprints, plus no-Intro control and subsequent TS task execution. Strict PCM waveform match is recorded separately, not a pass condition; no native fidelity, save/load playback or skip-input claim.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
