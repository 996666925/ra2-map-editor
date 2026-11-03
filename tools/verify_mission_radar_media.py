"""Observe TS radar movie pixels and WASAPI soundtrack in a normal campaign window.

Uses ordinary menu clicks and the existing movie audio observer. Requires
Windows, FFmpeg, numpy and PyAudioWPatch; all are verification dependencies.
Keeps PNGs and raw radar crops. No engine diagnostics, Escape or state writes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from verify_mission_movie_audio import capture, decode
from verify_mission_movie_window import Window
from verify_mission_results import Font


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observe(np, font, references, raw):
    pixels = np.frombuffer(raw, dtype=np.uint8).reshape(736, 1000, 4)
    cyan = (pixels[:, :, 0] > 150) & (pixels[:, :, 1] > 80) & (pixels[:, :, 2] < 80)
    # Fixed 1000x736 sidebar: 168-wide radar surface at (832,48), with the
    # original 140x110 Bink centered horizontally. Compare RGB, not alpha.
    video = pixels[48:158, 846:986, :3][:, :, ::-1].astype(np.float32)
    subtitle = (pixels[:, :, 2] > 200) & (pixels[:, :, 1] > 95) & (pixels[:, :, 0] < 220)
    sample = video.ravel() - video.mean()
    norm = np.linalg.norm(sample)
    correlations = references @ sample / max(float(norm), 1e-8)
    index = int(np.argmax(correlations))
    radar = pixels[28:180, 832:1000].tobytes()
    return {
        'radar_sha256': hashlib.sha256(radar).hexdigest(),
        'video_reference_frame': index,
        'video_correlation': float(correlations[index]),
        'cues': [letter for letter in 'ABCD' if font.find(subtitle[:160, :810], 'SDK movie cue ' + letter)],
        'before_message': font.find(cyan[:160, :810], 'SDK radar start'),
        'after_message': font.find(cyan[:160, :810], 'SDK radar finished'),
        'radar': radar,
    }


def run_window(np, references, client, fixture, output, control, pause_simulation):
    output.mkdir(parents=True)
    font = Font(np, fixture / 'game.fnt')
    settings = fixture / 'settings/VERA.INI'
    original = settings.read_bytes() if settings.exists() else None
    text = original.decode('utf-8') if original is not None else '[Intro]\nPlay=no\n[VERA]\nFullScreen=no\nSkipStartupLogo=yes\nLockCursor=no\n'
    text = text.replace('SkipStartupLogo=no', 'SkipStartupLogo=yes').replace('LockCursor=yes', 'LockCursor=no')
    settings.parent.mkdir(exist_ok=True)
    settings.write_text(text, encoding='utf-8')
    child, window, samples = None, None, []
    try:
        with (output / 'client.log').open('wb') as log:
            child = subprocess.Popen([str(client), '-WIN'], cwd=fixture, stdout=log, stderr=log,
                                     creationflags=subprocess.CREATE_NO_WINDOW)
            time.sleep(8)
            window = Window(child.pid)
            window.capture(output / 'menu.png')
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / 'single-player.png')
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / 'campaign.png')
            window.click(395, 535)
            started = time.perf_counter()
            while time.perf_counter() - started < 22:
                name = f'frame-{len(samples):03d}'
                observed_at = time.perf_counter()
                raw = window.capture(output / (name + '.png'))
                facts = observe(np, font, references, raw)
                (output / (name + '-radar.bgra')).write_bytes(facts.pop('radar'))
                samples.append({'time': observed_at - started, 'path': name + '.png', **facts})
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
        (output / 'frames.json').write_text(json.dumps(samples, indent=2), encoding='utf-8')
        source_log = fixture / 'logs/ra2.log'
        if source_log.exists():
            (output / 'runtime.log').write_bytes(source_log.read_bytes())

    movie = [i for i, s in enumerate(samples) if s['video_correlation'] >= 0.9]
    cue_times = {letter: next((s['time'] for s in samples if letter in s['cues']), None) for letter in 'ABCD'}
    gap = max(b['time'] - a['time'] for a, b in zip(samples, samples[1:]))
    after = next((s['time'] for s in samples if s['after_message']), None)
    checks = {
        'clean_exit': child.returncode == 0,
        'settings_restored': (settings.read_bytes() if settings.exists() else None) == original,
        'sampling_gap_under_half_second': gap < 0.5,
        'initial_authored_message': any(s['before_message'] for s in samples),
        'subsequent_authored_message': after is not None,
        'movie_presence_matches_control': bool(movie) != control,
        'subtitle_presence_matches_control': any(t is not None for t in cue_times.values()) != control,
    }
    if not control:
        duration = samples[movie[-1]]['time'] - samples[movie[0]]['time'] if movie else 0
        checks.update({
            'three_seconds_of_actual_movie_pixels': 2.5 <= duration <= 3.4,
            'multiple_decoded_frames_visible': len({samples[i]['video_reference_frame'] for i in movie}) >= 8,
            'all_four_subtitles_in_order': all(t is not None for t in cue_times.values()) and all(a < b for a, b in zip(cue_times.values(), list(cue_times.values())[1:])),
            'message_timing_matches_pause_mode': bool(movie) and after is not None and (after > samples[movie[-1]]['time']) == pause_simulation,
            'subsequent_message_visible_after_movie': bool(movie) and any(s['after_message'] for s in samples[movie[-1] + 1:]),
        })
    return {'passed': all(checks.values()), 'checks': checks, 'control': control, 'pause_simulation': pause_simulation,
            'observation_origin_perf_counter': started, 'movie_frames': movie,
            'cue_times': cue_times, 'after_message_seconds': after,
            'max_sampling_gap_seconds': gap, 'client_sha256': sha(client),
            'inputs': {name: sha(fixture / name) for name in ('mission.ts', 'sdk.ts', 'SOV01UMD.MAP', 'subtitlemd.txt', 'game.fnt', 'config.toml')},
            'scope': 'Ordinary foreground campaign window; known GAME.FNT phrases and RGB correlation with independently decoded 140x110 Bink frames; no engine diagnostics or native pixel parity.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('client', 'paused', 'unpaused', 'control', 'ffmpeg', 'python-deps', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != 'win32' or args.output.exists():
        parser.error('Windows and a new output directory are required')
    sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    import pyaudiowpatch as pa

    sources = [args.paused.resolve(), args.unpaused.resolve(), args.control.resolve()]
    for source, expected in zip(sources, ('true', 'false', None)):
        text = (source / 'mission.ts').read_text(encoding='utf-8')
        expression = 'action.playRadarMovie(m.movie(Movie.A01_P01e),'
        if (expression in text) != (expected is not None) or expected is not None and expression + expected + ')' not in text:
            parser.error('expected radar-paused, radar-unpaused and radar-control sources')
        if not json.loads((source / 'radar-runtime-report.json').read_text())['passed']:
            parser.error('ordinary-frame radar request audit failed')
        if sha(source / 'SOV01UMD.MAP') != sha(source / 'install/SOV01UMD.MAP'):
            parser.error('installed map differs from authored map')
    bik = sources[0] / 'A01_P01e.bik'
    if any(sha(source / 'A01_P01e.bik') != sha(bik) for source in sources):
        parser.error('reference movies differ')
    ffmpeg = args.ffmpeg.resolve()
    video = subprocess.run([str(ffmpeg), '-v', 'error', '-nostdin', '-i', str(bik), '-map', '0:v:0',
                            '-pix_fmt', 'rgb24', '-f', 'rawvideo', 'pipe:1'], capture_output=True, check=True).stdout
    frames = np.frombuffer(video, dtype=np.uint8).reshape(45, 110 * 140 * 3).astype(np.float32)
    frames -= frames.mean(axis=1, keepdims=True)
    references = frames / np.maximum(np.linalg.norm(frames, axis=1, keepdims=True), 1e-8)
    pcm = decode(ffmpeg, ['-i', str(bik), '-map', '0:a:0'])
    reference = np.frombuffer(pcm, dtype='<f4')
    args.output.mkdir(parents=True)
    (args.output / 'reference.rgb').write_bytes(video)
    (args.output / 'reference.f32').write_bytes(pcm)
    os.environ['RUST_LOG'] = 'info'
    runs = []
    for name, source in zip(('paused', 'unpaused', 'control'), sources):
        def runner(client, fixture, output, control):
            return run_window(np, references, client, fixture, output, control, name == 'paused')
        result = capture(np, pa, args.client.resolve(), source, args.output / name, name == 'control', ffmpeg, reference, window_runner=runner)
        runs.append(result)
        print(json.dumps({'run': name, 'passed': result['passed'], 'failed': [k for k, v in result['checks'].items() if not v], 'window_failed': [k for k, v in result['window']['checks'].items() if not v]}), flush=True)
    report = {'passed': all(r['passed'] for r in runs), 'runs': dict(zip(('paused', 'unpaused', 'control'), runs)),
              'fingerprints': {str(p): sha(p) for p in (Path(__file__), Path(__file__).with_name('verify_mission_movie_audio.py'), Path(__file__).with_name('verify_mission_movie_window.py'), Path(__file__).with_name('verify_mission_results.py'), args.client, ffmpeg, bik, args.output / 'reference.rgb', args.output / 'reference.f32')},
              'scope': 'Normal campaign GUI, independent decoded movie RGB and WASAPI time-frequency soundtrack matching for Action100/117 plus no-movie control. Not PCM sample equality, native game pixels, microphone/speakers, menu skipping, nested or save/load playback.'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
