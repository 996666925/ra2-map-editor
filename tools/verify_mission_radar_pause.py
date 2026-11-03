"""Verify radar movie progression, scenario pause and autonomous recovery with real media."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from verify_mission_chrono_screen import capture, read_json
from verify_mission_visibility import write_png


def analyze(folder, log, paused):
    receipt = read_json(folder / "capture.json")
    opening = receipt["opening"]
    rows = opening["observations"]
    current = opening["current"]
    def marker(text):
        row = next(row for row in rows if any(m["text"] == text for m in row["messages"]))
        return {key: row[key] for key in ("tick", "wall_ms", "scenario_timer", "input_locked")}
    before, after = marker("SDK radar start"), marker("SDK radar finished")
    # RealtimeUnlocked waits for the observed unlock edge, not an exact final
    # tick. Rendering can complete after another ordinary simulation frame.
    unlocked = next(row for row in rows if row["tick"] >= 301 and not row["input_locked"])
    cues = [marker(f"SDK movie cue {letter}") for letter in "ABCD"]
    ticks = [cue["tick"] for cue in cues]
    walls = [cue["wall_ms"] for cue in cues]
    raw = (folder / "frame.bgra").read_bytes()
    if (receipt["surface"]["width"], receipt["surface"]["height"], len(raw)) != (800, 600, 800 * 600 * 4):
        raise ValueError("unexpected GPU extent")
    if hashlib.sha256(raw).hexdigest() != receipt["frame"]["sha256"]:
        raise ValueError("GPU hash mismatch")
    write_png(folder / "preview.png", raw, 800, 600, 800 * 4)
    checks = {
        "real_wall_clock": opening["clock_domain"] == "ordinary wall-clock frame pacing and audio service" and opening["exact_steps"] == [],
        "authored_logic_markers": before["tick"] == 46 and after["tick"] == 241 and unlocked["tick"] == 301 and current["tick"] >= unlocked["tick"],
        "actual_movie_admitted": f"Radar movie admitted: A01_P01e.BIK, source=movmd03.mix, pause={str(paused).lower()}" in log,
        "distinct_video_times_reached": all(a < b for a, b in zip(walls, walls[1:])) and walls[-1] - walls[0] >= 1700,
        "video_progression_matches_pause_mode": len(set(ticks)) == 1 if paused else all(a < b for a, b in zip(ticks, ticks[1:])),
        "timer_matches_video_pause_mode": len({cue["scenario_timer"]["remaining"] for cue in cues}) == 1 if paused else all(a["scenario_timer"]["remaining"] > b["scenario_timer"]["remaining"] for a, b in zip(cues, cues[1:])),
        "timer_uses_logic_frames": before["scenario_timer"]["remaining"] - after["scenario_timer"]["remaining"] == 195,
        "prior_lock_retained": all(cue["input_locked"] for cue in cues) and after["input_locked"],
        "subsequent_logic_and_timer_resume": current["tick"] - after["tick"] >= 60 and current["scenario_timer"]["remaining"] == after["scenario_timer"]["remaining"] - (current["tick"] - after["tick"]),
        "input_and_cursor_restored": opening["saw_lock"] and opening["saw_unlock"] and not current["input_locked"] and current["cursor_depth"] == 0,
        "keyboard_restored": not opening["unlocked_arrow_probe"]["custom_binding"] and opening["unlocked_arrow_probe"]["press_admitted"] and opening["unlocked_arrow_probe"]["release_cleared"],
        "mouse_restored": opening["unlocked_mouse_probe"]["capture_released"] and opening["unlocked_mouse_probe"]["released_buttons_admit_right_pan"],
        "movie_housing_retired": opening["radar_display"]["phase"] not in ("MovieOpening", "MovieRestoring"),
        "native_speed_preserved": all(row["game_speed"] == 2 for row in rows),
    }
    return {"checks": checks, "before": before, "after": after, "subtitle_cues": cues,
            "wall_gap_ms": after["wall_ms"] - before["wall_ms"],
            "radar_display": opening["radar_display"], "frame_sha256": receipt["frame"]["sha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("client", "fixture", "control", "output"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    parser.add_argument("--repeats", type=int, choices=range(1, 5), default=2)
    args = parser.parse_args()
    client, fixture, control, output = (p.resolve() for p in (args.client, args.fixture, args.control, args.output))
    output.mkdir(exist_ok=False)
    # The client writes its admission records to the normal task-local log.
    # Keep INFO enabled and preserve each run before the next one truncates it.
    os.environ["RUST_LOG"] = "info"
    checks, runs = {}, []
    report = {"passed": False, "checks": checks, "runs": runs,
              "scope": "Real Bink video callbacks at four media times, frozen scenario tick/timer for Action117 versus progressing Action100, autonomous end-of-movie recovery, retained lock and normal input. Final GPU output only; not in-movie pixel matching, audio fidelity, menu/skip/nested/save-load combinations."}
    try:
        for name, source in (("paused", fixture), ("unpaused", control)):
            checks[f"{name}_ordinary_frame_contract"] = read_json(source / "radar-runtime-report.json")["passed"]
            movie = next(m for m in read_json(source / "movie-inventory.json")["movies"] if m["id"] == "A01_P01e")
            metadata = movie["metadata"]
            checks[f"{name}_real_three_second_movie"] = movie["installed"] and metadata["source"] == "movmd03.mix" and metadata["frames"] == 45 and metadata["fps"] == 15
            subtitle = (source / "subtitlemd.txt").read_bytes()
            checks[f"{name}_subtitle_published_unchanged"] = subtitle == (source / "install/subtitlemd.txt").read_bytes() and subtitle.startswith(b"\xff\xfe")
            checks[f"{name}_subtitle_cues_not_injected_by_ts"] = all(f"SDK movie cue {letter}" not in (source / "mission.ts").read_text(encoding="utf-8") for letter in "ABCD")
        for repeat in range(args.repeats):
            pair = {}
            for name, source, paused in (("paused", fixture, True), ("unpaused", control, False)):
                tag = f"{name}-{repeat}"
                folder = capture(client, source, output, tag, "campaign-sov1-opening-realtime-unlocked")
                runtime_log = (source / "logs/ra2.log").read_bytes()
                (folder / "runtime.log").write_bytes(runtime_log)
                pair[name] = analyze(folder, runtime_log.decode("utf-8", errors="replace"), paused)
                checks.update({f"{tag}_{key}": value for key, value in pair[name]["checks"].items()})
            extra = pair["paused"]["wall_gap_ms"] - pair["unpaused"]["wall_gap_ms"]
            pair["additional_pause_ms"] = extra
            checks[f"{repeat}_additional_stall_matches_three_second_video"] = 2500 <= extra <= 3500
            runs.append(pair)
        report["passed"] = all(checks.values())
    except (OSError, ValueError, KeyError, StopIteration, subprocess.TimeoutExpired) as error:
        report["error"] = f"{type(error).__name__}: {error}"
    report["sha256"] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in (
        ("client", client), ("mission", fixture / "mission.ts"), ("control", control / "mission.ts"),
        ("subtitles", fixture / "subtitlemd.txt"), ("verifier", Path(__file__)))}
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
