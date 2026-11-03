"""Run TS --visibility and --camera-pan fixtures through existing GPU captures.

Uses only Python's standard library. Keeps the original capture bundles and
writes PNG previews beside them, plus a report of visibility/camera/sidebar
facts and selected cameo pixels. It does not certify native-game pixel parity.
"""
import argparse
import hashlib
import json
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path


TICKS = (0, 30, 32, 60, 90, 120, 122, 150, 180, 210, 250, 270, 300, 360, 420, 600)
NAMES = tuple(str(tick) for tick in TICKS) + ("event-review",)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_png(path, raw, width, height, stride):
    rgba = bytearray(width * height * 4)
    for y in range(height):
        row = raw[y * stride:y * stride + width * 4]
        start = y * width * 4
        rgba[start:start + width * 4] = row
        rgba[start:start + width * 4:4] = row[2::4]
        rgba[start + 2:start + width * 4:4] = row[0::4]
    scan = b"".join(b"\0" + rgba[y * width * 4:(y + 1) * width * 4] for y in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff)

    path.write_bytes(b"\x89PNG\r\n\x1a\n"
                     + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(scan)) + chunk(b"IEND", b""))


def load_capture(root, name):
    folder = root / f"capture-{name}"
    receipt = read_json(folder / "capture.json")
    frame = receipt["frame"]
    if (frame["pixel_layout"], frame["width"], frame["height"]) != ("BGRA8", 1024, 768):
        raise ValueError(f"unexpected frame format for {name}")
    raw = (folder / "frame.bgra").read_bytes()
    if len(raw) != frame["row_stride"] * frame["height"] or hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise ValueError(f"frame integrity check failed for {name}")
    write_png(root / f"preview-{name}.png", raw, frame["width"], frame["height"], frame["row_stride"])
    observed = receipt["observations"]["frames"][-1]
    sidebar = receipt["observations"]["sidebar"]["frames"][-1]
    facts = {
        "tick": receipt["final"]["simulation_tick"],
        "complete": receipt["status"] == "COMPLETE",
        "hidden": receipt["lifecycle"]["window_hidden"],
        "render_ready": receipt["render"]["ready"],
        "camera": receipt["render"]["camera"]["top_left"],
        "cells": {",".join(map(str, row["cell"])): row["local_visibility"] for row in observed["terrain"]},
        "cell_world": {",".join(map(str, row["cell"])): row["render"]["cell_center_world"] for row in observed["terrain"]},
        "sidebar_rendered": sidebar["rendered"],
        "sidebar": sidebar["sidebar"],
        "instances": receipt["render"]["instance_counts"],
        "gestures": receipt["observations"].get("gesture_input"),
        "frame_sha256": frame["sha256"],
    }
    return facts, raw, frame["row_stride"]


def crop(capture, type_id):
    facts, raw, stride = capture
    item = next(item for item in facts["sidebar"]["items"] if item["type_id"] == type_id)
    x, y, width, height = map(int, item["rect"])
    return b"".join(raw[row * stride + x * 4:row * stride + (x + width) * 4] for row in range(y, y + height))


def analyze(root):
    captures = {name: load_capture(root, name) for name in NAMES}
    facts = {name: capture[0] for name, capture in captures.items()}

    def revealed(name, cell):
        return facts[str(name)]["cells"][cell]["revealed"]

    def centered(name, cell):
        point = facts[str(name)]["cell_world"][cell]
        camera = facts[str(name)]["camera"]
        # This fixture fixes 1024x768 with the native 168px sidebar and
        # 32px bottom bar; the tactical viewport center is (428,368).
        return abs(camera[0] + 428 - point[0]) <= 1 and abs(camera[1] + 368 - point[1]) <= 1

    flashed = crop(captures["30"], "E1")
    normal = crop(captures["32"], "E1")
    expired = crop(captures["122"], "E1")
    energy = lambda pixels: sum(pixels[0::4]) + sum(pixels[1::4]) + sum(pixels[2::4])
    flashed_energy, normal_energy = energy(flashed), energy(normal)
    checks = {
        "all_captures_complete_hidden_and_ready": all(f["complete"] and f["hidden"] and f["render_ready"] and f["sidebar_rendered"] for f in facts.values()),
        "all_requested_ticks_reached": all(facts[str(tick)]["tick"] == tick for tick in TICKS) and facts["event-review"]["tick"] == 210,
        "initial_target_and_far_cell_shrouded": not revealed(0, "80,100") and not revealed(0, "115,100"),
        "local_reveal_opens_target_but_not_far_cell": revealed(30, "80,100") and revealed(30, "85,100") and not revealed(30, "115,100"),
        "local_conceal_closes_target": not revealed(60, "80,100") and not revealed(60, "85,100"),
        "zone_reveal_reaches_far_cell": revealed(90, "80,100") and revealed(90, "115,100"),
        "reshroud_closes_target_and_keeps_own_base_sight": not revealed(120, "80,100") and facts["120"]["cells"]["40,100"]["visible"],
        "whole_map_reveal_opens_all_watched_cells": all(row["revealed"] for row in facts["150"]["cells"].values()),
        "grow_shroud_preserves_whole_map_reveal": all(row["revealed"] for row in facts["180"]["cells"].values()),
        "jump_centers_target": centered(30, "80,100"),
        "home_changes_camera": facts["210"]["camera"] != facts["180"]["camera"],
        "authored_infantry_tab_rendered": facts["30"]["sidebar"]["active_tab"] == "infantry",
        "authored_building_tab_rendered": facts["210"]["sidebar"]["active_tab"] == "building",
        "selected_cameo_visibly_pulses": flashed_energy > normal_energy * 1.15,
        "selected_cameo_returns_to_normal_after_deadline": expired == normal,
        "other_cameo_unchanged_during_pulse": crop(captures["30"], "ENGINEER") == crop(captures["32"], "ENGINEER"),
        "radar_online_before_outage": facts["270"]["instances"]["minimap"] > 0,
        "radar_outage_hides_minimap": facts["300"]["instances"]["minimap"] == 0 and facts["360"]["instances"]["minimap"] == 0,
        "radar_recovers_after_duration_and_open_animation": facts["600"]["instances"]["minimap"] > 0,
        "remapped_key_reviews_authored_radar_alert": centered("event-review", "100,85") and facts["event-review"]["camera"] != facts["210"]["camera"],
    }
    return {"passed": all(checks.values()), "checks": checks, "captures": facts,
            "cameo_rgb_sums": {"flashed": flashed_energy, "normal": normal_energy, "expired": energy(expired)},
            "scope": "Current-client hidden GPU capture of TS actions. Fog samples, retained rendered sidebar, camera endpoints, minimap presence, event review and selected cameo pixels are checked. PNG previews support inspection. Not native-game pixel parity, all zone geometries, LocalSize state verification or GUI save/load acceptance."}


def analyze_pan(root):
    facts = {}
    for tick in (30, 60, 90, 150):
        folder = root / f"pan-{tick}"
        receipt = read_json(folder / "capture.json")
        frame, surface = receipt["frame"], receipt["surface"]
        raw = (folder / "frame.bgra").read_bytes()
        if (surface["pixel_layout"], surface["width"], surface["height"]) != ("BGRA8", 800, 600):
            raise ValueError("unexpected camera capture extent")
        if len(raw) != surface["row_stride"] * surface["height"] or hashlib.sha256(raw).hexdigest() != frame["sha256"]:
            raise ValueError("camera capture frame integrity check failed")
        write_png(root / f"preview-pan-{tick}.png", raw, surface["width"], surface["height"], surface["row_stride"])
        current = receipt["opening"]["current"]
        facts[str(tick)] = {"tick": current["tick"], "camera": current["camera"],
                            "origin": current["camera_origin"], "frame_sha256": frame["sha256"]}
    start = facts["30"]["camera"]
    middle = facts["60"]["camera"]
    end = facts["90"]["camera"]
    home = facts["150"]["camera"]
    def near(point, expected):
        return all(abs(a - b) <= 1 for a, b in zip(point, expected))
    checks = {
        "pan_capture_ticks_reached": all(facts[str(tick)]["tick"] == tick for tick in (30, 60, 90, 150)),
        "pan_starts_at_authored_base_waypoint": near(start["view_center_world_pixels"], [-1800, 2130]),
        "pan_has_active_intermediate_frame": middle["focus_rate_bits"] != 0 and middle["focus_progress_bits"] != 0
            and middle["view_center_world_pixels"] != start["view_center_world_pixels"]
            and middle["view_center_world_pixels"] != end["view_center_world_pixels"],
        "pan_finishes_at_authored_target": near(end["view_center_world_pixels"], [-600, 2730]) and end["focus_rate_bits"] == 0,
        "home_returns_to_local_base": near(home["view_center_world_pixels"], [-1620, 2265]) and home["focus_rate_bits"] == 0,
    }
    return checks, facts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--pan-fixture", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--reuse-visibility", type=Path,
                        help="reuse complete visibility bundles after verifying client/source/contract fingerprints")
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("this runner uses Windows hidden child processes")
    client, fixture, contract, output = (path.resolve() for path in (args.client, args.fixture, args.contract, args.output))
    pan_fixture = args.pan_fixture.resolve()
    if not client.is_file() or not contract.is_file():
        parser.error("client and contract must exist")
    if not all((fixture / f"profile-{name}.json").is_file() for name in NAMES):
        parser.error("fixture must be generated with --visibility")
    if "action.centerCamera(view,4)" not in (pan_fixture / "mission.ts").read_text(encoding="utf-8"):
        parser.error("pan fixture must be generated with --camera-pan")
    if output.exists():
        parser.error("output directory must be new")
    reuse = args.reuse_visibility.resolve() if args.reuse_visibility else None
    if reuse:
        previous = read_json(reuse / "report.json")
        for key, path in (("client_sha256", client), ("source_sha256", fixture / "mission.ts"), ("contract_sha256", contract)):
            if previous.get(key) != hashlib.sha256(path.read_bytes()).hexdigest():
                parser.error(f"reuse fingerprint mismatch: {key}")
        for name in NAMES:
            receipt = read_json(reuse / f"capture-{name}" / "capture.json")
            for path, expected in (
                (fixture / f"profile-{name}.json", receipt["profile"]["sha256"]),
                (fixture / "config.toml", receipt["inputs"]["config"]["sha256"]),
                (Path(receipt["map_source"]["path"]), receipt["map_source"]["source_sha256"]),
            ):
                if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                    parser.error(f"reuse input changed: {path}")
        # Validate every byte and receipt before accepting reused captures.
        analyze(reuse)
    output.mkdir(parents=True)
    report = None
    for name in NAMES:
        if reuse:
            shutil.copytree(reuse / f"capture-{name}", output / f"capture-{name}")
            continue
        with (output / f"client-{name}.log").open("wb") as log:
            try:
                child = subprocess.run([str(client), "--tactical-capture", "map-observe-v1", "--profile",
                                        str(fixture / f"profile-{name}.json"), "--contract", str(contract),
                                        "--output", str(output / f"capture-{name}")], cwd=fixture,
                                       stdout=log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW,
                                       timeout=130)
            except subprocess.TimeoutExpired:
                report = {"passed": False, "failed_capture": name, "reason": "client timeout"}
                break
        print(f"capture {name}: exit {child.returncode}", flush=True)
        if child.returncode:
            report = {"passed": False, "failed_capture": name, "client_exit": child.returncode}
            break
    if report is None:
        for tick in (30, 60, 90, 150):
            with (output / f"client-pan-{tick}.log").open("wb") as log:
                try:
                    child = subprocess.run([str(client), "--shell-capture", f"campaign-all1-opening-tick-{tick}",
                                            "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                                            "--shell-capture-timeout-seconds", "120", "--output", str(output / f"pan-{tick}")],
                                           cwd=pan_fixture, stdout=log, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
                except subprocess.TimeoutExpired:
                    report = {"passed": False, "failed_pan_capture": tick, "reason": "client timeout"}
                    break
            print(f"camera {tick}: exit {child.returncode}", flush=True)
            if child.returncode:
                report = {"passed": False, "failed_pan_capture": tick, "client_exit": child.returncode}
                break
    if report is None:
        try:
            report = analyze(output)
            pan_checks, pan_facts = analyze_pan(output)
            report["checks"].update(pan_checks)
            report["pan_captures"] = pan_facts
            local_size = read_json(fixture / "local-size-report.json")
            report["checks"]["headless_local_size_runtime"] = local_size["passed"]
            report["local_size"] = local_size
            report["scope"] = "Current-client GPU fog/sidebar/camera/minimap and radar review, plus ordinary headless LocalSize authority. Selected cameo pixels and camera motion are checked; PNG previews support inspection. Not native-game pixel parity, all zone geometry, GPU LocalSize clipping or GUI save/load acceptance. Radar review uses isolated configurable J binding."
            report["passed"] = all(report["checks"].values())
        except (OSError, ValueError, KeyError, StopIteration) as error:
            report = {"passed": False, "analysis_error": str(error)}
    report["client_sha256"] = hashlib.sha256(client.read_bytes()).hexdigest()
    report["source_sha256"] = hashlib.sha256((fixture / "mission.ts").read_bytes()).hexdigest()
    report["pan_source_sha256"] = hashlib.sha256((pan_fixture / "mission.ts").read_bytes()).hexdigest()
    report["contract_sha256"] = hashlib.sha256(contract.read_bytes()).hexdigest()
    report["reused_visibility_capture_root"] = str(reuse) if reuse else None
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
