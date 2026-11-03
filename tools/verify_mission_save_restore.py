"""Check TS camera/countdown/team/rule state through ordinary client save/load.

Uses the existing hidden campaign restore route, which calls the production
save/preparation/commit owners and rebuilds GPU resources. It does not exercise
clicking the save browser, native SAV compatibility or multiplayer loading.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess

from verify_mission_chrono_screen import capture
from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def normalized_observation(row):
    row = copy.deepcopy(row)
    row.pop("wall_ms")
    for message in row["messages"]:
        message.pop("deadline_ms")
    return row


def message_history(rows):
    history, previous = [], []
    for row in rows:
        messages = [message["text"] for message in row["messages"]]
        if messages != previous:
            history.append({"tick": row["tick"], "messages": messages})
            previous = messages
    return history


def analyze(folder, menu=False, restored=False):
    receipt = read_json(folder / "capture.json")
    raw = (folder / "frame.bgra").read_bytes()
    if ((receipt["surface"]["width"], receipt["surface"]["height"]) != (800, 600)
            or len(raw) != 800 * 600 * 4
            or hashlib.sha256(raw).hexdigest() != receipt["frame"]["sha256"]):
        raise ValueError("invalid GPU frame extent/hash")
    write_png(folder / "preview.png", raw, 800, 600, 3200)
    opening = receipt["opening"]
    current = opening["current"]
    steps = opening["exact_steps"]
    history = message_history(opening["observations"])
    expected = []
    messages = []
    events = [(31, "SDK repeated rule"), (61, "SDK repeated rule"),
                          (80, "SDK saved guard finished"), (91, "SDK repeated rule"),
                          (121, "SDK repeated rule")]
    if menu:
        events.append((151, "SDK repeated rule"))
    for tick, message in events:
        messages.append(message)
        expected.append({"tick": tick, "messages": messages.copy()})
    team = current["teams"]
    target = 167 if menu else 121
    expected_steps = list(range(target))
    if menu and restored:
        expected_steps = list(range(121)) + list(range(16)) + list(range(121, target))
    checks = {
        "ordinary_campaign": receipt["runtime"]["game_mode_nonzero"] is False,
        "steps_follow_authored_timeline_and_explicit_loads": len(steps) == len(expected_steps) and all(
            step == {"tick_before": tick, "tick_after": tick + 1,
                     "binary_frame_before": tick, "binary_frame_after": tick + 1}
            for tick, step in zip(expected_steps, steps)),
        "countdown_keeps_original_start_and_title": current["tick"] == target
            and current["scenario_timer"] == {"remaining": 465 - target, "caption": "TSTXF9940F03"}
            and opening["saw_countdown"],
        "repeated_rule_and_saved_team_finish_at_original_ticks": history == expected,
        "original_member_and_active_team_retained": len(team) == 1
            and team[0]["id"] == 0 and team[0]["members"] == [1]
            and team[0]["cursor"] == (3 if menu else 2) and current["active_types"].get("E1") == 1,
        "camera_naturally_finishes_pan": current["camera"]["focus_rate_bits"] == 0
            and current["camera"]["view_center_world_pixels"] == [-600.0, 2730.0],
        "input_and_cursor_restore": opening["saw_lock"] and opening["saw_unlock"]
            and not current["input_locked"] and current["cursor_depth"] == 0
            and opening["unlocked_arrow_probe"]["press_admitted"]
            and opening["unlocked_arrow_probe"]["release_cleared"]
            and opening["unlocked_mouse_probe"]["capture_released"]
            and opening["unlocked_mouse_probe"]["released_buttons_admit_right_pan"],
    }
    return receipt, raw, {"checks": checks, "message_history": history}


def compare_pair(restored, continuous, restored_raw, continuous_raw):
    sidebar = lambda raw: b"".join(raw[(y * 800 + 633) * 4:(y * 800 + 800) * 4] for y in range(600))
    return {
        "checks": {
            "all_final_observed_fields_match_except_wall_and_message_deadlines": (
                normalized_observation(restored["opening"]["current"])
                == normalized_observation(continuous["opening"]["current"])),
            "final_sidebar_pixels_match": sidebar(restored_raw) == sidebar(continuous_raw),
        },
        "whole_frame_pixels_match": restored_raw == continuous_raw,
        "different_pixels": sum(restored_raw[i:i + 4] != continuous_raw[i:i + 4]
                                for i in range(0, len(restored_raw), 4)),
    }


def compare_menu_load(opening):
    load = opening["load_while_locked"]
    if load is None:
        raise ValueError("capture did not load an unlocked save through the in-game menu")
    saves = {name: Path(opening[name]).read_bytes() for name in ("pre_lock_save", "unlocked_save")}
    return {
        "checks": {
            "two_real_saves_created": all(len(payload) > 1_000_000 for payload in saves.values()),
            "menu_load_replaces_lock_with_saved_unlocked_state": load["depth_before"] == -1
                and not load["locked_after"] and load["depth_after"] == 0,
            "one_cursor_and_normal_drag_after_menu_load": load["cursor_instances_after"] == 1
                and load["right_drag_pans_after"],
        },
        "load": load,
        "saves": {name: {"bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
                  for name, payload in saves.items()},
    }


def compare_restore(opening):
    saved = opening["save_restore"]
    if saved is None:
        raise ValueError("capture did not run the production restore path")
    path = Path(saved["path"])
    payload = path.read_bytes()
    before, after = saved["saved_runtime"], saved["loaded_runtime"]
    progress = struct.unpack("<f", struct.pack("<I", saved["view_before"]["focus_progress_bits"]))[0]
    # Production resource_load.rs first validates the saved rules digest, then
    # applies retained TypeLoad image mutations and updates the runtime digest.
    # The startup owner necessarily changes. Scenario RNG reset is preexisting.
    left, right = copy.deepcopy(before), copy.deepcopy(after)
    for row in (left, right):
        row.pop("startup_admission")
        row["active_rules"].pop("simulation_config_hash")
        row["rng"].pop("scenario")
    checks = {
        "real_saved_payload_matches_receipt": len(payload) == saved["byte_length"]
            and len(payload) > 1_000_000 and hashlib.sha256(payload).hexdigest() == saved["sha256"],
        "save_and_restore_at_same_mid_pan_frame": before["tick"] == after["tick"] == 50
            and before["binary_frame"] == after["binary_frame"] == 50
            and saved["view_before"] == saved["view_after"] and 0.25 <= progress < 1.0,
        "outgoing_lock_cursor_depth_retained": saved["cursor_depth_before"] == saved["cursor_depth_after"] == -1,
        "production_restore_owner_committed": before["startup_admission"]["campaign_installation"]
            and after["startup_admission"]["saved_world_restored"],
        "remaining_immediate_runtime_fields_match": left == right,
    }
    return {"checks": checks, "saved_tick": before["tick"], "save_sha256": saved["sha256"],
            "save_bytes": len(payload), "camera_progress": progress,
            "full_immediate_runtime_matches": before == after,
            "rules_digest_before": before["active_rules"]["simulation_config_hash"],
            "rules_digest_after": after["active_rules"]["simulation_config_hash"],
            "rng_streams_match": {key: before["rng"][key] == after["rng"][key]
                                  for key in ("main", "scenario", "mapgen")}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("client", "fixture", "output"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    parser.add_argument("--menu-fixture", type=Path,
                        help="also check --camera-restore-menu via an in-lock menu load")
    args = parser.parse_args()
    client, fixture, output = (path.resolve() for path in (args.client, args.fixture, args.output))
    source = (fixture / "mission.ts").read_text(encoding="utf-8")
    if "m.team('saved_guard'" not in source or "SDK repeated rule" not in source:
        parser.error("fixture must be generated with --camera-restore")
    output.mkdir(exist_ok=False)
    os.environ["RUST_LOG"] = "info"
    checks, runs = {}, {}
    report = {"passed": False, "checks": checks, "runs": runs,
              "scope": "Production client save/preparation/commit and GPU resource restore in mid-pan; TS countdown, active team, repeated trigger, continued camera/input and final sidebar. Uses existing exact ordinary-frame diagnostics. Not save-browser clicks, multiplayer, native SAV compatibility, every saved field or whole-frame pixel equivalence. Raw runtime differences, Scenario RNG reset and TypeLoad digest mutation remain explicit."}
    try:
        captures = {}
        for name, checkpoint in (("restored", "campaign-sov1-opening-restore-unlocked"),
                                 ("continuous", "campaign-sov1-opening-unlocked")):
            folder = capture(client, fixture, output, name, checkpoint)
            (folder / "runtime.log").write_bytes((fixture / "logs/ra2.log").read_bytes())
            receipt, raw, facts = analyze(folder)
            captures[name] = (receipt, raw)
            runs[name] = facts
            checks.update({f"{name}_{key}": value for key, value in facts["checks"].items()})
        restored, restored_raw = captures["restored"]
        continuous, continuous_raw = captures["continuous"]
        saved = compare_restore(restored["opening"])
        report["restore"] = saved
        checks.update({f"restore_{key}": value for key, value in saved["checks"].items()})
        checks["continuous_has_no_manual_restore"] = continuous["opening"]["save_restore"] is None
        pair = compare_pair(restored, continuous, restored_raw, continuous_raw)
        checks.update(pair["checks"])
        # Keep the broader comparison honest; the two independent process
        # launches need not have the same cosmetic terrain variant samples.
        report["whole_frame_pixels_match"] = pair["whole_frame_pixels_match"]
        report["different_pixels"] = pair["different_pixels"]
        if args.menu_fixture:
            menu_fixture = args.menu_fixture.resolve()
            menu = {}
            for name, checkpoint in (("menu-restored", "campaign-all1-opening-load-unlocked"),
                                     ("menu-continuous", "campaign-all1-opening-tick-167")):
                folder = capture(client, menu_fixture, output, name, checkpoint)
                (folder / "runtime.log").write_bytes((menu_fixture / "logs/ra2.log").read_bytes())
                receipt, raw, facts = analyze(folder, menu=True, restored=name == "menu-restored")
                menu[name] = (receipt, raw)
                runs[name] = facts
                checks.update({f"{name}_{key}": value for key, value in facts["checks"].items()})
            restored, restored_raw = menu["menu-restored"]
            continuous, continuous_raw = menu["menu-continuous"]
            menu_load = compare_menu_load(restored["opening"])
            report["menu_load"] = menu_load
            checks.update({f"menu_{key}": value for key, value in menu_load["checks"].items()})
            menu_pair = compare_pair(restored, continuous, restored_raw, continuous_raw)
            report["menu_comparison"] = menu_pair
            checks.update({f"menu_{key}": value for key, value in menu_pair["checks"].items()})
            report["menu_source_sha256"] = hashlib.sha256((menu_fixture / "mission.ts").read_bytes()).hexdigest()
        report["passed"] = all(checks.values())
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        report["error"] = f"{type(error).__name__}: {error}"
    report["sha256"] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in (
        ("client", client), ("mission", fixture / "mission.ts"),
        ("map", fixture / "SOV01UMD.MAP"), ("verifier", Path(__file__)))}
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
