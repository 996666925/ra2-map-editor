"""Observe TS speech-bubble fixtures through the existing hidden GPU capture.

Uses a declared three-frame test SHP when the installation lacks TALK BUBL.
Checks rendered shape pixels, including negative controls; not original artwork.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

from verify_mission_visibility import write_png

EXPECTED = {60: 1, 90: 2, 120: 2, 150: None, 180: 3, 210: None}


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def shapes(path):
    data = path.read_bytes()
    zero, width, height, count = struct.unpack_from("<4H", data)
    if (zero, width, height, count) != (0, 24, 64, 3):
        raise ValueError("requires the declared three-frame test SHP")
    frames = []
    for index in range(count):
        x, y, w, h = struct.unpack_from("<4H", data, 8 + index * 24)
        offset = struct.unpack_from("<I", data, 28 + index * 24)[0]
        pixels = data[offset:offset + w * h]
        if (x, y, w, h, data[16 + index * 24], len(pixels)) != (0, 0, 24, 16, 0, w * h):
            raise ValueError("unexpected test SHP frame")
        white = max(set(pixels) - {0}, key=pixels.count)
        frames.append(([(i % w, i // w) for i, pixel in enumerate(pixels) if pixel == white],
                       [(i % w, i // w) for i, pixel in enumerate(pixels) if pixel not in (0, white)]))
    return frames


def rendered_shapes(raw, masks):
    # The authored actor is fixed at (44,100), with Home at (40,100), zoom1.
    # Search its local screen region; validate the camera independently below.
    bright = set()
    dark = set()
    for y in range(260, 350):
        for x in range(360, 500):
            color = raw[(y * 800 + x) * 4:(y * 800 + x) * 4 + 3]
            if min(color) >= 220:
                bright.add((x, y))
            if max(color) <= 35:
                dark.add((x, y))
    found = []
    for frame, (white, black) in enumerate(masks, 1):
        anchor_x, anchor_y = white[0]
        for x, y in bright:
            ox, oy = x - anchor_x, y - anchor_y
            if all((ox + dx, oy + dy) in bright for dx, dy in white) and all(
                    (ox + dx, oy + dy) in dark for dx, dy in black):
                found.append({"frame": frame, "top_left": [ox, oy]})
    return sorted(found, key=lambda item: (item["frame"], item["top_left"]))


def analyze_capture(folder, tick, masks):
    receipt = read_json(folder / "capture.json")
    raw = (folder / "frame.bgra").read_bytes()
    frame = receipt["frame"]
    if (receipt["surface"]["width"], receipt["surface"]["height"], len(raw)) != (800, 600, 800 * 600 * 4):
        raise ValueError("unexpected GPU frame extent")
    if hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise ValueError("GPU frame digest mismatch")
    opening = receipt["opening"]
    current = opening["current"]
    if (current["tick"] != tick or not opening["saw_lock"] or not opening["saw_unlock"]
            or current["input_locked"] or len(current["teams"]) != 1
            or current["camera"]["view_center_world_pixels"] != [-1800.0, 2130.0]
            or current["camera"]["zoom"] != 1.0):
        raise ValueError("capture missed authored actor/camera/input prerequisites")
    write_png(folder / "preview.png", raw, 800, 600, 800 * 4)
    return {"tick": tick, "rendered_shapes": rendered_shapes(raw, masks),
            "members": current["teams"][0]["members"], "frame_sha256": frame["sha256"],
            "wall_ms": current["wall_ms"],
            "message_visits": [{"tick": row["tick"], "wall_ms": row["wall_ms"]}
                               for row in opening["observations"] if row.get("messages")]}


def detector_controls(masks):
    blank = bytes([80, 80, 80, 255]) * (800 * 600)
    if rendered_shapes(blank, masks):
        return False
    for frame, (white, black) in enumerate(masks, 1):
        raw = bytearray(blank)
        for points, color in ((white, 252), (black, 0)):
            for x, y in points:
                start = ((y + 300) * 800 + x + 420) * 4
                raw[start:start + 4] = bytes([color, color, color, 255])
        if rendered_shapes(raw, masks) != [{"frame": frame, "top_left": [420, 300]}]:
            return False
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--control", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--expiry", action="store_true", help="verify natural expiry of a single 0.5-second bubble")
    args = parser.parse_args()
    client, fixture, control, output = (p.resolve() for p in (args.client, args.fixture, args.control, args.output))
    output.mkdir(exist_ok=False)
    checks, observations = {}, {}
    report = {"passed": False, "checks": checks, "observations": observations,
              "scope": "Current-client hidden GPU output of a TS-recruited actor and declared test SHP; three frames, missing-team no-op, team/global clear. Not original artwork, natural wall-clock expiry, whole-scene parity or GUI save/load."}
    expected_frames = {46: 1, 120: None} if args.expiry else EXPECTED
    if args.expiry:
        report["scope"] = "Current-client GPU display then natural wall-clock expiry of a single TS speech bubble, plus no-cue controls. Uses declared test artwork; not original artwork or GUI save/load."
    try:
        masks = shapes(fixture / "talkbubl.shp")
        checks["detector_rejects_blank_and_distinguishes_all_frames"] = detector_controls(masks)
        if (fixture / "talkbubl.shp").read_bytes() != (control / "talkbubl.shp").read_bytes():
            raise ValueError("control must use the same shape resource")
        checks["ordinary_runtime_cues"] = read_json(fixture / "talk-runtime-report.json")["passed"]
        checks["control_has_no_cues"] = read_json(control / "talk-runtime-report.json")["passed"]
        for name, source in (("authored", fixture), ("control", control)):
            observations[name] = {}
            for tick, expected in expected_frames.items():
                folder = output / f"{name}-{tick}"
                with (output / f"{name}-{tick}.log").open("wb") as log:
                    child = subprocess.run([str(client), "--shell-capture", f"campaign-all1-opening-tick-{tick}",
                                            "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                                            "--shell-capture-timeout-seconds", "120", "--output", str(folder)],
                                           cwd=source, stdout=log, stderr=subprocess.STDOUT,
                                           creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
                print(f"{name} {tick}: exit {child.returncode}", flush=True)
                if child.returncode:
                    raise ValueError(f"{name} {tick}: client exit {child.returncode}")
                facts = analyze_capture(folder, tick, masks)
                observations[name][str(tick)] = facts
                wanted = expected if name == "authored" else None
                actual = [shape["frame"] for shape in facts["rendered_shapes"]]
                checks[f"{name}_{tick}_shape"] = actual == ([] if wanted is None else [wanted])
        first_tick = str(next(iter(expected_frames)))
        checks["same_original_actor_across_all_captures"] = all(
            facts["members"] == observations["authored"][first_tick]["members"]
            for group in observations.values() for facts in group.values())
        if args.expiry:
            cues = read_json(fixture / "talk-runtime-report.json")["cues"]
            checks["single_cue_without_clear_or_replacement"] = len(cues) == 1 and cues[0]["value"] == 1 and cues[0]["tick"] == 46
            checks["native_duration_is_thirty_16ms_buckets"] = len(cues) == 1 and cues[0]["duration"] == 30
            ages = {}
            for tick in (46, 120):
                facts = observations["authored"][str(tick)]
                request = next(row for row in facts["message_visits"] if row["tick"] == 46)
                ages[str(tick)] = facts["wall_ms"] - request["wall_ms"]
            report["observed_wall_age_ms"] = ages
            checks["visible_before_wall_deadline"] = 0 <= ages["46"] < 30 * 16
            checks["absent_after_wall_deadline"] = ages["120"] >= 30 * 16
        report["passed"] = all(checks.values())
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    report["client_sha256"] = hashlib.sha256(client.read_bytes()).hexdigest()
    report["source_sha256"] = hashlib.sha256((fixture / "mission.ts").read_bytes()).hexdigest()
    report["control_source_sha256"] = hashlib.sha256((control / "mission.ts").read_bytes()).hexdigest()
    report["shape_sha256"] = hashlib.sha256((fixture / "talkbubl.shp").read_bytes()).hexdigest()
    report["verifier_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
