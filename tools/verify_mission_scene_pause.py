"""Verify a TS timed pause using the client's ordinary wall-clock campaign loop."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def analyze(folder):
    receipt = read_json(folder / "capture.json")
    opening = receipt["opening"]
    current = opening["current"]
    observations = opening["observations"]
    marker = lambda text: next(row for row in observations if any(message["text"] == text for message in row["messages"]))
    before, after = marker("SDK pause start"), marker("SDK pause resumed")
    raw = (folder / "frame.bgra").read_bytes()
    if (receipt["surface"]["width"], receipt["surface"]["height"], len(raw)) != (800, 600, 800 * 600 * 4):
        raise ValueError("unexpected GPU output size")
    if hashlib.sha256(raw).hexdigest() != receipt["frame"]["sha256"]:
        raise ValueError("GPU output hash mismatch")
    write_png(folder / "preview.png", raw, 800, 600, 800 * 4)
    arrow, mouse = opening["unlocked_arrow_probe"], opening["unlocked_mouse_probe"]
    checks = {
        "ordinary_wall_clock_not_exact_steps": opening["clock_domain"] == "ordinary wall-clock frame pacing and audio service" and opening["exact_steps"] == [],
        "authored_markers_in_order": before["tick"] == 46 and after["tick"] == 61,
        "native_game_speed_preserved": all(row["game_speed"] == 2 for row in observations),
        "logic_timer_advances_only_fifteen_frames": before["scenario_timer"]["remaining"] - after["scenario_timer"]["remaining"] == 15,
        "existing_lock_retained_until_explicit_unlock": before["input_locked"] and after["input_locked"],
        "normal_logic_and_timer_continue": current["tick"] == 91 and current["scenario_timer"]["remaining"] == after["scenario_timer"]["remaining"] - 30,
        "final_input_and_cursor_restored": opening["saw_lock"] and opening["saw_unlock"] and not current["input_locked"] and current["cursor_depth"] == 0,
        "normal_keyboard_input_works": arrow["press_admitted"] and arrow["release_cleared"],
        "normal_mouse_input_works": mouse["capture_released"] and mouse["released_buttons_admit_right_pan"],
    }
    return {"checks": checks, "before": {"tick": before["tick"], "wall_ms": before["wall_ms"], "timer": before["scenario_timer"]},
            "after": {"tick": after["tick"], "wall_ms": after["wall_ms"], "timer": after["scenario_timer"]},
            "wall_gap_ms": after["wall_ms"] - before["wall_ms"], "final_tick": current["tick"],
            "frame_sha256": receipt["frame"]["sha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--control", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repeats", type=int, choices=range(1, 5), default=2)
    args = parser.parse_args()
    client, fixture, control, output = (path.resolve() for path in (args.client, args.fixture, args.control, args.output))
    output.mkdir(exist_ok=False)
    checks, runs = {}, []
    report = {"passed": False, "checks": checks, "runs": runs,
              "scope": "TS Action110 timed pause and real host wall-clock expiry, contrasted with a no-pause task. Logic timer, explicit lock retention, post-pause keyboard/mouse and final GPU output are checked. Not pause audio fidelity, nested consumers, save/load, paused movies or chrono-screen rendering."}
    try:
        for name, source in (("paused", fixture), ("control", control)):
            checks[f"{name}_ordinary_runtime_contract"] = read_json(source / "scene-pause-runtime-report.json")["passed"]
        for repeat in range(args.repeats):
            pair = {}
            for name, source in (("paused", fixture), ("control", control)):
                folder = output / f"{name}-{repeat}"
                with (output / f"{name}-{repeat}.log").open("wb") as log:
                    child = subprocess.run([str(client), "--shell-capture", "campaign-sov1-opening-realtime-unlocked",
                                            "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                                            "--shell-capture-timeout-seconds", "120", "--output", str(folder)],
                                           cwd=source, stdout=log, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
                print(f"{name} {repeat}: exit {child.returncode}", flush=True)
                if child.returncode:
                    raise ValueError(f"{name} {repeat}: client exit {child.returncode}")
                pair[name] = analyze(folder)
                checks.update({f"{repeat}_{name}_{key}": value for key, value in pair[name]["checks"].items()})
            extra = pair["paused"]["wall_gap_ms"] - pair["control"]["wall_gap_ms"]
            pair["additional_pause_ms"] = extra
            # Two native units =120*16ms, with a bounded allowance for render
            # observations on either side of the ordinary service boundary.
            checks[f"{repeat}_additional_delay_matches_1920ms_pause"] = 1920 - 256 <= extra <= 1920 + 256
            checks[f"{repeat}_paused_gap_includes_wall_deadline"] = pair["paused"]["wall_gap_ms"] >= 1920 - 64
            checks[f"{repeat}_control_has_no_comparable_stall"] = 0 < pair["control"]["wall_gap_ms"] < 1000
            runs.append(pair)
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
