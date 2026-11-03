"""Observe TS-triggered campaign outcomes in a normal visible Windows client.

Uses ordinary menu/Continue/No mouse input, foreground client PNGs and WASAPI
loopback. No forced result, engine diagnostics, game-state writes or Escape.
Fixture generation/publication is mission_sdk_result_fixture. Desktop must stay
unobstructed. UI masks supplement retained PNG review; no native parity claim.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import struct
import sys
import time
import wave

from verify_mission_audio import tone_report
from verify_mission_movie_window import Window


class Font:
    """Read font bits independently; match known labels, without general OCR."""
    def __init__(self, np, path):
        self.np, self.raw = np, path.read_bytes()
        self.cache = {}
        magic, _, self.bpr, self.rows, _, _, self.stride = struct.unpack_from('<7I', self.raw)
        if magic != 0x546E6F66:
            raise ValueError('expected production GAME.FNT')

    def text(self, text):
        np = self.np
        glyphs = []
        for ch in text:
            index = struct.unpack_from('<H', self.raw, 28 + ord(ch) * 2)[0]
            if not index:
                raise ValueError('font lacks ' + ch)
            offset = 28 + 131072 + (index - 1) * self.stride
            width = self.raw[offset]
            bits = np.frombuffer(self.raw[offset + 1:offset + 1 + self.bpr * self.rows], dtype=np.uint8)
            glyphs.append(np.unpackbits(bits.reshape(self.rows, self.bpr), axis=1)[:, :width].astype(bool))
            glyphs.append(np.zeros((self.rows, 1), dtype=bool))
        bitmap = np.concatenate(glyphs[:-1], axis=1)
        return bitmap

    def find(self, mask, text, scale=1.0):
        np = self.np
        key = (text, scale)
        if key not in self.cache:
            source = self.text(text)
            templates = []
            for phase_y in ([0] if scale == 1 else np.arange(0, 1, 0.125)):
                for phase_x in ([0] if scale == 1 else np.arange(0, 1, 0.125)):
                    yy = np.floor((np.arange(int(source.shape[0] * scale)) + 0.5 - phase_y) / scale).astype(int)
                    xx = np.floor((np.arange(int(source.shape[1] * scale)) + 0.5 - phase_x) / scale).astype(int)
                    yy, xx = yy.clip(0, source.shape[0] - 1), xx.clip(0, source.shape[1] - 1)
                    bitmap = source[yy[:, None], xx]
                    ys, xs = np.nonzero(bitmap)
                    template = bitmap[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
                    if not any(np.array_equal(template, old) for old in templates):
                        templates.append(template)
            self.cache[key] = templates
        return any(self.find_template(mask, template) for template in self.cache[key])

    def find_template(self, mask, template):
        np = self.np
        height, width = template.shape
        ys, xs = np.nonzero(template)
        my, mx = np.nonzero(mask)
        my, mx = my - ys[0], mx - xs[0]
        ok = (my >= 0) & (mx >= 0) & (my + height <= mask.shape[0]) & (mx + width <= mask.shape[1])
        my, mx = my[ok], mx[ok]
        for index in np.linspace(0, len(xs) - 1, min(35, len(xs)), dtype=int):
            ok = mask[my + ys[index], mx + xs[index]]
            my, mx = my[ok], mx[ok]
            if not len(my):
                return False
        return any(np.array_equal(mask[y:y + height, x:x + width], template) for y, x in zip(my, mx))

    def isolated(self, mask, text, clip_bottom=0):
        """Known fitted-shell label: allow raster rounding at 736/600 scale."""
        np = self.np
        ys, xs = np.nonzero(mask)
        if not len(ys):
            return False
        actual = mask[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
        self.find(np.zeros((1, 1), dtype=bool), text, 736 / 600)
        for template in self.cache[(text, 736 / 600)]:
            if clip_bottom:
                # The score's time-value static clips the final raster row.
                # Keep title/message controls at their complete glyph extent.
                template = template[:-clip_bottom]
            if any(abs(a - b) > 2 for a, b in zip(template.shape, actual.shape)):
                continue
            for dy in range(-2, 3):
                for dx in range(-2, 3):
                    canvas = np.zeros((max(template.shape[0], actual.shape[0]) + 4,
                                       max(template.shape[1], actual.shape[1]) + 4), dtype=bool)
                    canvas[2:2 + actual.shape[0], 2:2 + actual.shape[1]] = actual
                    candidate = canvas[2 + dy:2 + dy + template.shape[0], 2 + dx:2 + dx + template.shape[1]]
                    intersection = np.count_nonzero(candidate & template)
                    union = np.count_nonzero(actual) + np.count_nonzero(template) - intersection
                    # Per-glyph fractional origins round independently in the
                    # fitted shell. Require shape overlap as well as extent;
                    # the long title/question and paired buttons gate the UI.
                    if intersection / max(union, 1) >= 0.80:
                        return True
        return False


def labels(np, font, raw, ranking=None):
    pixels = np.frombuffer(raw, dtype=np.uint8).reshape(736, 1000, 4)
    yellow = (pixels[:, :, 0] < 65) & (pixels[:, :, 1] > 175) & (pixels[:, :, 2] > 175)
    cyan = (pixels[:, :, 0] > 150) & (pixels[:, :, 1] > 80) & (pixels[:, :, 2] < 80)
    result = {"score_title": font.isolated(yellow[:45, 810:], "Mission Score"),
            "continue": font.isolated(yellow[670:700, 850:940], "Continue"),
            "yes": font.isolated(yellow[487:502, 665:700], "Yes"),
            "no": font.isolated(yellow[520:545, 650:715], "No"),
            "replay_question": font.isolated(yellow[247:268, 295:565], "Would you like to replay this mission?"),
            "stage_one": font.find(cyan[:70, :810], "SDK stage one"),
            "stage_two": font.find(cyan[:70, :810], "SDK stage two reached"),
            "inherited_flag": font.find(cyan[:70, :810], "SDK inherited flag seven")}
    if ranking:
        for field, region in [("title", yellow[8:32, 155:635]),
                              ("message", yellow[38:65, 155:635]),
                              ("par", yellow[492:518, 235:345])]:
            result["ranking_" + field] = font.isolated(region, ranking[field], clip_bottom=1 if field == "par" else 0)
        result["opposite_ranking_title"] = font.isolated(yellow[8:32, 155:635], ranking["opposite_title"])
        result["opposite_ranking_message"] = font.isolated(yellow[38:65, 155:635], ranking["opposite_message"])
    return result


def run(np, pa, font, client, fixture, output, case):
    output.mkdir(parents=True)
    chunks, flags, packets, samples, inputs = [], [], [], [], []
    child, window = None, None
    started = time.perf_counter()
    with pa.PyAudio() as host:
        device = host.get_default_wasapi_loopback()
        rate, channels = int(device["defaultSampleRate"]), int(device["maxInputChannels"])

        def receive(data, count, timing, status):
            if status:
                flags.append(int(status))
            arrival = time.perf_counter()
            packets.append({"count": count, "arrival": arrival,
                            "capture_start": arrival + timing["input_buffer_adc_time"] - timing["current_time"]})
            chunks.append(data)
            return None, pa.paContinue

        with host.open(format=pa.paFloat32, channels=channels, rate=rate, input=True,
                       input_device_index=device["index"], frames_per_buffer=1024,
                       stream_callback=receive):
            try:
                environment = os.environ.copy()
                environment["RUST_LOG"] = "info"
                with (output / "client.log").open("wb") as log:
                    child = subprocess.Popen([str(client), "-WIN"], cwd=fixture, env=environment,
                                             stdout=log, stderr=log)
                    time.sleep(8)
                    window = Window(child.pid)

                    def click(x, y, label):
                        window.click(x, y)
                        inputs.append({"time": time.perf_counter() - started, "action": label, "xy": [x, y]})

                    window.capture(output / "menu.png")
                    click(890, 268, "single-player")
                    time.sleep(2)
                    click(890, 268, "new-campaign")
                    time.sleep(2)
                    window.capture(output / "campaign.png")
                    click(395, 170, "allied-campaign")
                    origin = time.perf_counter()
                    continued = False
                    while time.perf_counter() - origin < 40:
                        elapsed = time.perf_counter() - origin
                        name = f"frame-{len(samples):03d}.png"
                        raw = window.capture(output / name)
                        # Retain raw cropped UI masks for later pair/control
                        # comparisons; labels must be inspected in the PNGs.
                        def crop(x0, y0, x1, y1):
                            return b"".join(raw[(y * 1000 + x0) * 4:(y * 1000 + x1) * 4] for y in range(y0, y1))
                        samples.append({"time": elapsed, "path": name, **labels(np, font, raw, case.get("ranking")),
                                        "message_sha256": hashlib.sha256(crop(0, 0, 550, 65)).hexdigest(),
                                        "center_sha256": hashlib.sha256(crop(250, 260, 750, 450)).hexdigest(),
                                        "sidebar_sha256": hashlib.sha256(crop(810, 0, 1000, 736)).hexdigest()})
                        if elapsed >= 27 and not continued and case["expected"] != "None":
                            window.capture(output / "before-answer.png")
                            if case["expected"] == "Victory":
                                click(890, 683, "score-continue")
                            else:
                                click(680, 530, "replay-no")
                            continued = True
                        time.sleep(0.25)
            finally:
                if window and child and child.poll() is None:
                    window.u.PostMessageW(window.handle, 0x10, 0, 0)
                if child:
                    try:
                        child.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        child.terminate()
                        child.wait(timeout=10)
                (output / "frames.json").write_text(json.dumps(samples, indent=2), encoding="utf-8")
                (output / "inputs.json").write_text(json.dumps(inputs, indent=2), encoding="utf-8")
    raw = b"".join(chunks)
    (output / "loopback.f32").write_bytes(raw)
    (output / "packets.json").write_text(json.dumps(packets, indent=2), encoding="utf-8")
    frames = np.frombuffer(raw, dtype="<f4").reshape(-1, channels)
    with wave.open(str(output / "loopback.wav"), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((np.clip(frames, -1, 1) * 32767).astype("<i2").tobytes())
    mono = frames.mean(axis=1)
    tones = {str(hz): tone_report(np, mono, rate, hz, 0.6) for hz in (660, 880)}
    checks = {"normal_exit": child.returncode == 0,
              "finite_samples": bool(np.isfinite(frames).all()),
              "no_audio_callback_errors": not flags,
              "victory_eva_presence": tones["660"]["passed"] == (case["expected"] == "Victory"),
              "defeat_eva_presence": tones["880"]["passed"] == (case["expected"] == "Defeat")}
    checks["initial_authored_message"] = any(s["stage_one"] for s in samples)
    before, after = [s for s in samples if s["time"] < 27], [s for s in samples if s["time"] > 28]
    score = any(s["score_title"] and s["continue"] for s in before)
    replay = any(s["yes"] and s["no"] and s["replay_question"] for s in before)
    checks["score_matches_outcome"] = score == (case["expected"] == "Victory")
    checks["replay_prompt_matches_outcome"] = replay == (case["expected"] == "Defeat")
    if case["expected"] == "Victory":
        checks["next_mission_and_inherited_flag"] = any(s["stage_two"] and s["inherited_flag"] for s in after)
        if case.get("ranking"):
            checks["authored_rank_message_and_par_time"] = any(
                s["score_title"] and s["continue"] and s["ranking_title"] and s["ranking_message"] and s["ranking_par"]
                for s in before)
            checks["opposite_ranking_absent"] = not any(s["opposite_ranking_title"] or s["opposite_ranking_message"] for s in before)
    elif case["expected"] == "Defeat":
        checks["no_next_mission"] = not any(s["stage_two"] for s in samples)
        checks["replay_prompt_dismissed"] = not any(s["yes"] and s["no"] for s in samples[-4:])
    else:
        checks["control_stays_in_first_mission"] = not any(s["stage_two"] for s in samples)
    return {"passed": all(checks.values()), "checks": checks, "tones": tones, "case": case,
            "fixture_sha256": {str(path.relative_to(fixture)): hashlib.sha256(path.read_bytes()).hexdigest()
                               for path in [fixture / "first.ts", fixture / "sdk.ts", fixture / "install/first.map"]},
            "rate": rate, "channels": channels, "audio_callback_flags": flags,
            "inputs": inputs, "frame_count": len(samples),
            "scope": "Normal visible client, known text matched against the production font, actual outcome EVA tone loopback and campaign transition; no general OCR, original audio fidelity or native pixel parity claim."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python-deps", type=Path, required=True)
    parser.add_argument("--case", help="optional single case name; default all cases")
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("requires Windows foreground desktop and WASAPI loopback")
    sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    import pyaudiowpatch as pa
    client, fixtures, output = (p.resolve() for p in (args.client, args.fixtures, args.output))
    font = Font(np, fixtures / "game.fnt")
    if output.exists():
        parser.error("output directory must be new")
    cases = json.loads((fixtures / "report.json").read_text(encoding="utf-8"))["cases"]
    selected = [case for case in cases if args.case is None or case["name"] == args.case]
    if not selected:
        parser.error("unknown case")
    output.mkdir(parents=True)
    report = {"client_sha256": hashlib.sha256(client.read_bytes()).hexdigest(),
              "font_sha256": hashlib.sha256(font.raw).hexdigest(),
              "verifier_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "cases": []}
    for case in selected:
        print("Observing " + case["name"], flush=True)
        result = run(np, pa, font, client, fixtures / case["name"], output / case["name"], case)
        report["cases"].append(result)
        report["passed"] = all(c["passed"] for c in report["cases"]) and len(report["cases"]) == len(selected)
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
