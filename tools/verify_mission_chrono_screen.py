"""Verify TS chrono-screen GPU stages and recovery through existing client routes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def analyze(folder, tick, realtime):
    receipt = read_json(folder / "capture.json")
    raw = (folder / "frame.bgra").read_bytes()
    if (receipt["surface"]["width"], receipt["surface"]["height"], len(raw)) != (800, 600, 800 * 600 * 4):
        raise ValueError("unexpected GPU extent")
    if hashlib.sha256(raw).hexdigest() != receipt["frame"]["sha256"]:
        raise ValueError("GPU frame hash mismatch")
    opening = receipt["opening"]
    current = opening["current"]
    if current["tick"] != tick or not opening["saw_lock"] or current["game_speed"] != 2:
        raise ValueError("missed authored tick/input/speed prerequisites")
    pixels = [raw[(y * 800 + x) * 4:(y * 800 + x) * 4 + 3]
              for y in range(50, 540) for x in range(20, 610)]
    chrome = [raw[(y * 800 + x) * 4:(y * 800 + x) * 4 + 3]
              for y in range(10, 200) for x in range(650, 790)]
    facts = {"tick": tick, "white_fraction": sum(min(pixel) >= 250 for pixel in pixels) / len(pixels),
             "mean_brightness": sum(sum(pixel) for pixel in pixels) / (3 * len(pixels)),
             "chrome_white_fraction": sum(min(pixel) >= 250 for pixel in chrome) / len(chrome),
             "frame_sha256": receipt["frame"]["sha256"], "input_locked": current["input_locked"]}
    if realtime:
        if opening["clock_domain"] != "ordinary wall-clock frame pacing and audio service" or opening["exact_steps"]:
            raise ValueError("recovery must use the ordinary host clock")
        marker = lambda text: next(row for row in opening["observations"]
                                   if any(message["text"] == text for message in row["messages"]))
        before, after = marker("SDK chrono start"), marker("SDK chrono finished")
        facts["wall_gap_ms"] = after["wall_ms"] - before["wall_ms"]
        facts["checks"] = {
            "logic_markers_run_at_authored_ticks": before["tick"] == 46 and after["tick"] == 151,
            "timer_advances_only_with_logic": before["scenario_timer"]["remaining"] - after["scenario_timer"]["remaining"] == 105,
            "prior_lock_retained_until_explicit_unlock": after["input_locked"],
            "world_and_timer_continue_after_effect": current["scenario_timer"]["remaining"] == after["scenario_timer"]["remaining"] - 30,
            "input_and_cursor_restored": opening["saw_unlock"] and not current["input_locked"] and current["cursor_depth"] == 0,
            "normal_keyboard_works": opening["unlocked_arrow_probe"]["press_admitted"] and opening["unlocked_arrow_probe"]["release_cleared"],
            "normal_mouse_works": opening["unlocked_mouse_probe"]["released_buttons_admit_right_pan"],
        }
    write_png(folder / "preview.png", raw, 800, 600, 800 * 4)
    return facts


def capture(client, source, output, name, checkpoint):
    folder = output / name
    with (output / f"{name}.log").open("wb") as log:
        child = subprocess.run([str(client), "--shell-capture", checkpoint,
                                "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                                "--shell-capture-timeout-seconds", "120", "--output", str(folder)],
                               cwd=source, stdout=log, stderr=subprocess.STDOUT,
                               creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
    print(f"{name}: exit {child.returncode}", flush=True)
    if child.returncode:
        raise ValueError(f"{name}: client exit {child.returncode}")
    return folder


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--control", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    client, fixture, control, output = (path.resolve() for path in (args.client, args.fixture, args.control, args.output))
    output.mkdir(exist_ok=False)
    checks, observations = {}, {}
    report = {"passed": False, "checks": checks, "observations": observations,
              "scope": "TS Action127 ordinary-frame/replay-service contract, real GPU whitening/white/retained chrome, and autonomous recovery through the existing real-clock campaign loop. Includes a duration-zero no-op control. Not native-game whole-scene pixel parity, sound fidelity, restart/nested effects or save/load."}
    try:
        for name, source in (("chrono", fixture), ("control", control)):
            checks[f"{name}_ordinary_frame_contract"] = read_json(source / "chrono-runtime-report.json")["passed"]
            observations[name] = {}
            for tick in (45, 55, 75, 100):
                folder = capture(client, source, output, f"{name}-{tick}", f"campaign-sov1-opening-tick-{tick}")
                observations[name][str(tick)] = analyze(folder, tick, False)
            for repeat in range(2):
                folder = capture(client, source, output, f"{name}-recovered-{repeat}", "campaign-sov1-opening-realtime-unlocked")
                facts = analyze(folder, 181, True)
                observations[name][f"recovered-{repeat}"] = facts
                checks.update({f"{name}_{repeat}_{key}": value for key, value in facts["checks"].items()})
        chrono, control_facts = observations["chrono"], observations["control"]
        checks["before_effect_is_not_white"] = chrono["45"]["white_fraction"] < 0.05
        checks["warp_visibly_whitens_tactical_pixels"] = chrono["55"]["mean_brightness"] > control_facts["55"]["mean_brightness"] + 20
        for tick in (75, 100):
            checks[f"{tick}_white_tactical_output"] = chrono[str(tick)]["white_fraction"] > 0.999
            checks[f"{tick}_sidebar_retains_nonwhite_chrome"] = chrono[str(tick)]["chrome_white_fraction"] < 0.25
        checks["zero_duration_never_whitens"] = all(facts["white_fraction"] < 0.05 for facts in control_facts.values())
        checks["normal_tactical_pixels_return"] = all(chrono[f"recovered-{repeat}"]["white_fraction"] < 0.05 for repeat in range(2))
        for repeat in range(2):
            extra = chrono[f"recovered-{repeat}"]["wall_gap_ms"] - control_facts[f"recovered-{repeat}"]["wall_gap_ms"]
            report[f"additional_service_wall_ms_{repeat}"] = extra
            checks[f"{repeat}_real_clock_admits_sixteen_service_frames"] = 256 <= extra <= 768
        report["passed"] = all(checks.values())
    except (OSError, ValueError, KeyError, StopIteration, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    report["client_sha256"] = hashlib.sha256(client.read_bytes()).hexdigest()
    report["source_sha256"] = hashlib.sha256((fixture / "mission.ts").read_bytes()).hexdigest()
    report["control_source_sha256"] = hashlib.sha256((control / "mission.ts").read_bytes()).hexdigest()
    report["verifier_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
