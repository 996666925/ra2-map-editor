"""Verify TS LocalSize radar rebuilds with existing GPU and minimap input."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from verify_mission_lighting import prepare
from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observe(folder):
    receipt = read_json(folder / "capture.json")
    frame = receipt["frame"]
    raw = (folder / "frame.bgra").read_bytes()
    if (frame["width"], frame["height"], frame["row_stride"], len(raw)) != (1024, 768, 4096, 3145728):
        raise ValueError("unexpected raw frame extent")
    if hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise ValueError("frame hash mismatch")
    write_png(folder / "preview.png", raw, 1024, 768, 4096)
    radar = b"".join(raw[y * 4096 + 860 * 4:y * 4096 + 1018 * 4] for y in range(28, 180))
    rows = receipt["observations"].get("gesture_input", {}).get("receipts", [])
    return {"complete": receipt["status"] == "COMPLETE" and receipt["render"]["ready"],
            "tick": receipt["final"]["simulation_tick"],
            "source_sha256": receipt["map_source"]["source_sha256"],
            "radar_sha256": hashlib.sha256(radar).hexdigest(),
            "radar_rgb_mean": sum(sum(radar[ch::4]) for ch in (0, 1, 2)) / (len(radar) // 4 * 3),
            "camera": receipt["render"]["camera"]["top_left"],
            "gestures": rows}


def analyze(fixture, output):
    manifest = read_json(fixture / "fixture.json")
    checks, facts = {"ordinary_geometry_and_checkpoints": manifest["passed"]}, {}
    for case in manifest["cases"]:
        name = f"{case['name']}-{case['tick']}"
        facts[name] = item = observe(output / name)
        checks[name + "_complete_tick_map"] = item["complete"] and item["tick"] == case["tick"] and item["source_sha256"] == sha256(fixture / case["map"])
        checks[name + "_radar_has_pixels"] = item["radar_rgb_mean"] > 10
        if case["tick"]:
            rows = item["gestures"]
            checks[name + "_ordinary_minimap_input"] = len(rows) == 1 and rows[0]["neutral_input_restored"] and rows[0]["issue_after_step"] == case["tick"] - 1 and rows[0]["minimap"]["resolved_position"] == [940, 100]
            if len(rows) != 1:
                raise ValueError("expected one minimap gesture")
            x, y = rows[0]["minimap"]["cell"]
            cropped = case["name"] != "control" and case["tick"] in (165, 195) or case["name"] == "shrink" and case["tick"] == 240
            left, top, width, height = (20, 20, 24, 24) if cropped else (2, 4, 60, 54)
            checks[name + "_radar_target_uses_active_geometry"] = 64 + top * 2 < x + y <= 64 + 2 + (top + height) * 2 and x - y < (left + width) * 2 - 64 and y - x < 64 - left * 2
    target = lambda name: facts[name]["gestures"][0]["minimap"]["cell"]
    for tick in (165, 195, 240):
        checks[f"shrink_{tick}_radar_pixels_differ_from_full"] = facts[f"shrink-{tick}"]["radar_sha256"] != facts[f"control-{tick}"]["radar_sha256"]
        checks[f"shrink_{tick}_same_click_targets_different_cell"] = target(f"shrink-{tick}") != target(f"control-{tick}")
    checks["equal_writer_keeps_cropped_target"] = target("cycle-165") == target("cycle-195") == target("shrink-195")
    checks["restoration_restores_radar_target"] = target("cycle-240") == target("control-240")
    checks["restoration_restores_radar_pixels"] = facts["cycle-240"]["radar_sha256"] == facts["control-240"]["radar_sha256"]
    checks["initial_radar_pixels_identical"] = len({facts[name + "-0"]["radar_sha256"] for name in ("control", "shrink", "cycle")}) == 1
    report = {"passed": all(checks.values()), "checks": checks, "facts": facts,
              "scope": "TS Action40, twelve real Battle GPU frames, production radar recompose and normal minimap mouse conversion. Independent simulation checks original entity membership, flat terrain zones, retained building movement blockers, equal writers, and checkpoints at ticks165/195 through240. No engine edits. Not whole tactical clipping, elevation/bridges, all hierarchy levels, GUI load or full-state equivalence."}
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--analyze-only", action="store_true")
    args = parser.parse_args()
    client, game, fixture, contract, output = (p.resolve() for p in (args.client, args.game, args.fixture, args.contract, args.output))
    manifest = read_json(fixture / "fixture.json")
    if not manifest["passed"]:
        parser.error("ordinary geometry/checkpoint fixture must pass first")
    if not args.analyze_only:
        output.mkdir(parents=True, exist_ok=False)
        prepare(game, output)
        for case in manifest["cases"]:
            name = f"{case['name']}-{case['tick']}"
            with (output / f"{name}.log").open("wb") as log:
                child = subprocess.run([str(client), "--tactical-capture", "map-observe-v1", "--profile", str(fixture / case["profile"]), "--contract", str(contract), "--output", str(output / name)], cwd=output, stdout=log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
            print(f"{name}: exit {child.returncode}", flush=True)
            if child.returncode:
                raise RuntimeError(f"capture {name} failed; see retained log")
        paths = [client, contract, fixture / "fixture.json", Path(__file__).resolve(), output / "config.toml"]
        for case in manifest["cases"]:
            source = fixture / case["name"]
            paths.extend([fixture / case["profile"], fixture / case["map"], source / "mission.ts", source / "sdk.ts", source / "continuous.json", source / "restored-165.json", source / "restored-195.json"])
        (output / "inputs.json").write_text(json.dumps({str(p): sha256(p) for p in paths}, indent=2), encoding="utf-8")
    report = analyze(fixture, output)
    print(json.dumps({"passed": report["passed"], "failed": [k for k, v in report["checks"].items() if not v]}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
