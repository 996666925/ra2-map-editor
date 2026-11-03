"""Exercise a TS campaign through the normal Windows save/load browsers.

Only the owned foreground client receives ordinary mouse/keyboard input.
Snapshots are produced by the browser and read independently by the editor's
save auditor. No engine capture route, private input adapter or state injection.
Requires an unobstructed Windows desktop and a new isolated fixture/output.
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

from verify_mission_movie_window import Window
from verify_mission_results import Font


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class BrowserWindow(Window):
    def click(self, x, y):
        # A page can replace its controls while the pointer remains over the
        # same button. Give the new page an ordinary pointer move before press.
        self.check()
        point = w.POINT(x - 12, y)
        self.u.ClientToScreen(self.handle, c.byref(point))
        self.u.SetCursorPos(point.x, point.y)
        time.sleep(0.15)
        super().click(x, y)

    def key(self, code):
        self.check()
        self.u.keybd_event(code, 0, 0, 0)
        time.sleep(0.04)
        self.u.keybd_event(code, 0, 2, 0)
        time.sleep(0.04)

    def text(self, text):
        # Fixture descriptions use lower-case ASCII and spaces, so these are
        # actual host key presses, without clipboard or WM_CHAR injection.
        for char in text:
            if char != ' ' and not ('a' <= char <= 'z'):
                raise ValueError('description requires lower-case ASCII')
            self.key(0x20 if char == ' ' else ord(char.upper()))


def wait_for(test, timeout, description):
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        result = test()
        if result:
            return result
        time.sleep(0.2)
    raise TimeoutError(description)


def analyze(saves):
    a, b, c, d = [saves[name]['audit'] for name in ('a', 'b', 'c', 'd')]

    def types(row, kind):
        return [obj for obj in row['objects'] if obj['type'] == kind]

    def guard(row):
        teams = [team for team in row['teams'] if len(team['state']['members']) == 1
                 and team['state']['members'][0]['id'] == types(row, 'E1')[0]['id']]
        if len(teams) != 1:
            raise ValueError('expected exactly one original guard team')
        return teams[0]

    def repeat(row):
        runtime = row['triggers']
        index = next(tag['trigger'] for tag in runtime['definitions']['tag_types'] if tag['name'] == 'repeat')
        return next(trigger for trigger in runtime['triggers'] if trigger and trigger['type_index'] == index)

    checks = {
        'four_browser_descriptions': [row['description'] for row in (a, b, c, d)]
            == ['browser a', 'browser b', 'browser c', 'browser d'],
        'saved_and_loaded_frames_prove_rollback': 30 <= a['tick'] < b['tick']
            and a['tick'] <= c['tick'] < b['tick'] and c['tick'] - a['tick'] <= 60,
        'ordinary_countdown_keeps_original_anchor_and_caption': all(
            row['countdown']['frames'] == 9000 - row['tick']
            and row['countdown']['title'] == a['countdown']['title'] for row in (a, b, c, d)),
        'original_guard_identity_health_and_owner_preserved': all(
            types(row, 'E1') == types(a, 'E1') for row in (b, c, d)),
        'same_guard_team_and_script_after_load': all(
            guard(row)['id'] == guard(a)['id'] and guard(row)['type'] == guard(a)['type']
            and guard(row)['script'] == guard(a)['script'] for row in (b, c, d)),
        'guard_active_before_and_immediately_after_load': all(
            guard(row)['state']['cursor'] == 0 and 0 not in row['set_globals'] for row in (a, c)),
        'saved_guard_timer_anchor_and_duration_preserved': guard(a)['state']['guard_timer']
            == guard(c)['state']['guard_timer'] == {'duration': 300, 'start_frame': 2},
        'guard_completes_after_restored_timer': guard(d)['state']['cursor'] == 2
            and 0 in d['set_globals'],
        'repeated_reinforcement_follows_saved_frame_clock': all(
            len(types(row, 'ENGINEER')) == min(15, max(0, (row['tick'] - 1) // 30))
            for row in (a, b, c, d)),
        'saved_repeat_timer_anchor_and_duration_preserved': repeat(a)['timer'] == repeat(c)['timer']
            and repeat(a)['timer']['duration'] == 30,
        'saved_reinforcement_identities_preserved_after_load': all(
            obj in types(c, 'ENGINEER') and obj in types(d, 'ENGINEER') for obj in types(a, 'ENGINEER')),
        'final_stop_rule_executed_and_no_extra_waves': d['tick'] > 451
            and 1 in d['set_globals'] and len(types(d, 'ENGINEER')) == 15
            and repeat(d)['enabled'] is False and repeat(d)['timer'] == {'duration': 30, 'start_frame': 450},
        'all_observed_objects_alive': all(obj['alive'] and obj['health'] > 0
            for row in (a, b, c, d) for obj in row['objects']),
    }
    return checks


def run(args):
    sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    fixture, output = args.fixture.resolve(), args.output.resolve()
    font = Font(np, fixture / 'game.fnt')
    output.mkdir(parents=True, exist_ok=False)
    checks, saves, inputs, captures = {}, {}, [], []
    report = {'passed': False, 'checks': checks, 'saves': saves, 'inputs': inputs,
              'captures': captures, 'scope': 'Normal visible campaign save/load browser input, '
              'four real production saves, listed SDK frame/countdown/team/reinforcement state '
              'and retained GPU screenshots. No exact load-boundary snapshot, full-state equality, '
              'network loading, native SAV or original-game pixel parity claim.'}
    input_paths = {name: path for name, path in (
        ('verifier', Path(__file__)), ('window_helper', Path(__file__).with_name('verify_mission_movie_window.py')),
        ('font_helper', Path(__file__).with_name('verify_mission_results.py')),
        ('client', args.client), ('auditor', args.auditor), ('mission', fixture / 'mission.ts'),
        ('campaign', fixture / 'campaign.ts'), ('sdk', fixture / 'sdk.ts'),
        ('types', fixture / 'map.generated.ts'), ('map', fixture / 'install/first.map'),
        ('font', fixture / 'game.fnt'), ('config', fixture / 'config.toml'),
        ('fixture_report', fixture / 'fixture.json'), ('publication_manifest', fixture / 'package/manifest.json'))}
    input_hashes = {name: sha(path) for name, path in input_paths.items()}
    child, window, forced = None, None, False
    settings = fixture / 'settings/VERA.INI'
    original = settings.read_bytes()
    started = time.perf_counter()
    try:
        environment = os.environ.copy()
        environment['RUST_LOG'] = 'info'
        with (output / 'client.log').open('wb') as log:
            child = subprocess.Popen([str(args.client.resolve()), '-WIN'], cwd=fixture,
                                     env=environment, stdout=log, stderr=log)
            time.sleep(8)
            window = BrowserWindow(child.pid)

            def capture(name):
                path = output / (name + '.png')
                raw = window.capture(path)
                pixels = np.frombuffer(raw, dtype=np.uint8).reshape(736, 1000, 4)
                yellow = (pixels[:, :, 0] < 65) & (pixels[:, :, 1] > 175) & (pixels[:, :, 2] > 175)
                cyan = (pixels[:, :, 0] > 150) & (pixels[:, :, 1] > 80) & (pixels[:, :, 2] < 80)
                captures.append({'time': time.perf_counter() - started, 'path': path.name,
                                 'sha256': sha(path), 'bgra_sha256': hashlib.sha256(raw).hexdigest(),
                                 'countdown_title': font.find(yellow[670:705, :832], 'SDK browser countdown'),
                                 'guard_finished_message': font.find(cyan[:80, :832], 'SDK saved guard finished'),
                                 'save_confirmation': font.find(yellow[250:305, 300:600], 'Mission Saved')})
                return raw

            def click(x, y, label):
                window.click(x, y)
                inputs.append({'time': time.perf_counter() - started, 'action': label, 'xy': [x, y]})

            def key(code, label):
                window.key(code)
                inputs.append({'time': time.perf_counter() - started, 'action': label, 'key': code})

            def title(raw, expected):
                pixels = np.frombuffer(raw, dtype=np.uint8).reshape(736, 1000, 4)
                mask = (pixels[:, :, 0] < 65) & (pixels[:, :, 1] > 175) & (pixels[:, :, 2] > 175)
                found = font.find(mask[:40, 810:], expected)
                checks['title_' + expected.replace(' ', '_') + '_' + str(len(captures))] = found
                if not found:
                    raise ValueError('unexpected page; expected ' + expected)

            def save(name):
                title(capture('pause-' + name), 'Game Options')
                click(920, 289, 'open-save-' + name)
                time.sleep(0.5)
                title(capture('save-browser-' + name), 'Save Mission')
                click(350, 208, 'select-new-slot-' + name)
                click(350, 490, 'focus-description-' + name)
                # The selected new slot puts the caret after the description.
                # Backspace uses ordinary non-extended host keyboard input.
                for _ in range(79):
                    window.key(0x08)
                window.text('browser ' + name)
                inputs.append({'time': time.perf_counter() - started,
                               'action': 'type-description', 'text': 'browser ' + name})
                capture('save-description-' + name)
                before = set((fixture / 'saves').glob('*'))
                click(920, 239, 'save-slot-' + name)
                path = wait_for(lambda: next((p for p in (fixture / 'saves').glob('*.bin')
                    if p not in before), None), 20, 'browser did not create ' + name)
                time.sleep(0.5)
                capture('saved-confirmation-' + name)
                checks['save_confirmation_' + name] = captures[-1]['save_confirmation']
                audit_path = output / ('save-' + name + '.json')
                subprocess.run([str(args.auditor.resolve()), str(path), str(audit_path)],
                               check=True, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
                saves[name] = {'path': str(path), 'sha256': sha(path),
                               'audit': json.loads(audit_path.read_text(encoding='utf-8'))}
                key(0x0D, 'acknowledge-saved-' + name)
                time.sleep(0.4)
                title(capture('saved-menu-' + name), 'Game Options')

            capture('main-menu')
            time.sleep(4)
            click(890, 268, 'single-player')
            time.sleep(3)
            capture('single-player-menu')
            click(890, 268, 'new-campaign')
            time.sleep(3)
            capture('campaign-menu')
            click(395, 170, 'allied-campaign')
            wait_for(lambda: (fixture / 'saves/first.sav').exists(), 40, 'campaign initial save')
            time.sleep(2)
            capture('initial-game')
            key(0x1B, 'pause-a')
            time.sleep(0.5)
            save('a')
            click(920, 664, 'resume-after-a')
            time.sleep(5)
            capture('advanced-game')
            key(0x1B, 'pause-b')
            time.sleep(0.5)
            save('b')
            click(920, 264, 'open-load-browser')
            time.sleep(0.5)
            title(capture('load-browser-before-selection'), 'Load Mission')
            # Descending real file times: b, a, automatic mission save.
            click(350, 222, 'select-browser-a')
            capture('load-browser-selected-a')
            click(920, 239, 'load-browser-a')
            runtime_log = fixture / 'logs/ra2.log'
            loaded = wait_for(lambda: runtime_log.exists() and
                ('Load: restored simulation from ' + str(Path(saves['a']['path']).relative_to(fixture)))
                in runtime_log.read_text(encoding='utf-8', errors='replace'), 40, 'production load commit')
            checks['production_load_commit_logged'] = bool(loaded)
            def game_redrawn():
                capture('load-redraw-' + str(len(captures)))
                return captures[-1]['countdown_title']
            wait_for(game_redrawn, 10, 'loaded game countdown was not redrawn')
            capture('loaded-game')
            key(0x1B, 'pause-c')
            time.sleep(0.5)
            save('c')
            click(920, 664, 'resume-after-c')
            for index in range(34):
                time.sleep(0.5)
                capture('continued-' + str(index).zfill(2))
            capture('continued-game')
            key(0x1B, 'pause-d')
            time.sleep(0.5)
            save('d')
            checks.update(analyze(saves))
            checks['countdown_visible_before_and_after_load'] = all(
                next(item for item in captures if item['path'] == name + '.png')['countdown_title']
                for name in ('initial-game', 'advanced-game', 'loaded-game', 'continued-game'))
            checks['restored_guard_completion_message_visible'] = any(
                item['guard_finished_message'] for item in captures if item['path'].startswith('continued-'))
    except (OSError, ValueError, KeyError, IndexError, TimeoutError, subprocess.SubprocessError) as error:
        report['error'] = f'{type(error).__name__}: {error}'
    finally:
        if window and child and child.poll() is None:
            window.u.PostMessageW(window.handle, 0x10, 0, 0)
        if child:
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                forced = True
                child.terminate()
                child.wait(timeout=10)
        settings.write_bytes(original)
        report['forced_termination'] = forced
        checks['normal_process_exit'] = child is not None and child.returncode == 0 and not forced
        report['source_sha256'] = input_hashes
        report['source_paths'] = {name: str(path.resolve()) for name, path in input_paths.items()}
        checks['all_input_files_unchanged'] = input_hashes == {
            name: sha(path) for name, path in input_paths.items()}
        checks['all_browser_saves_unchanged'] = all(
            sha(Path(saved['path'])) == saved['sha256'] for saved in saves.values())
        checks['task_local_settings_restored'] = settings.read_bytes() == original
        if (fixture / 'logs/ra2.log').exists():
            (output / 'runtime.log').write_bytes((fixture / 'logs/ra2.log').read_bytes())
        report['passed'] = len(saves) == 4 and 'error' not in report and all(checks.values())
        (output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    return report['passed']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('client', 'auditor', 'fixture', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--python-deps', type=Path, default=Path('target/sdk-audio-tools'))
    args = parser.parse_args()
    if sys.platform != 'win32':
        parser.error('requires Windows')
    if args.output.exists() or (args.fixture / 'saves').exists():
        parser.error('output and fixture saves directory must be new')
    if 'saved_guard' not in (args.fixture / 'mission.ts').read_text(encoding='utf-8'):
        parser.error('expected mission_sdk_save_browser_fixture')
    return 0 if run(args) else 1


if __name__ == '__main__':
    raise SystemExit(main())
