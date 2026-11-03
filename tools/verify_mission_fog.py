"""Compare TS campaign FogOfWar controls using existing opening captures."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def observe(folder, cells):
    receipt = read_json(folder / "capture.json")
    surface = receipt["surface"]
    raw = (folder / "frame.bgra").read_bytes()
    if (surface["width"], surface["height"], surface["row_stride"], len(raw)) != (800, 600, 3200, 1920000):
        raise ValueError("unexpected raw frame dimensions")
    if hashlib.sha256(raw).hexdigest() != receipt["frame"]["sha256"]:
        raise ValueError("frame hash mismatch")
    write_png(folder / "preview.png", raw, 800, 600, 3200)
    opening = receipt["opening"]
    current = opening["current"]
    camera = current["camera_origin"]
    samples = []
    for rx, ry in cells:
        # iso_to_screen is the tile image's left corner; the cell center is
        # one half-width to the right. Flat terrain, zoom=1 in this fixture.
        x, y = round((rx - ry) * 30 - camera[0]), round((rx + ry) * 15 + 30 - camera[1])
        if not (12 <= x < 620 and 8 <= y < 550):
            raise ValueError(f"sample outside tactical viewport: {rx, ry} -> {x, y}")
        pixels = b"".join(raw[row * 3200 + (x - 10) * 4:row * 3200 + (x + 10) * 4]
                          for row in range(y - 4, y + 4))
        samples.append({"cell": [rx, ry], "center": [x, y],
                        "rgb_mean": [sum(pixels[ch::4]) / (len(pixels) // 4) for ch in (2, 1, 0)],
                        "sha256": hashlib.sha256(pixels).hexdigest()})
    sidebar = b"".join(raw[y * 3200 + 640 * 4:y * 3200 + 795 * 4] for y in range(0, 550))
    return {"tick": current["tick"], "viewer": current["fog_viewer"], "camera": camera,
            "locked_then_unlocked": opening["saw_lock"] and opening["saw_unlock"] and not current["input_locked"],
            "map_sha256": receipt["selected_map_source"]["source_sha256"],
            "campaign": receipt["startup"]["campaign"]["id"], "active_types": current["active_types"],
            "samples": samples, "sidebar_sha256": hashlib.sha256(sidebar).hexdigest()}


def analyze(fixture, output):
    manifest = read_json(fixture / "fixture.json")
    facts, checks = {}, {}
    for case in manifest["cases"]:
        name = case["name"]
        source = fixture / name
        facts[name] = item = observe(output / name, manifest["cells"])
        checks[name + "_campaign_tick_viewer"] = item["tick"] == case["tick"] and item["campaign"] == "ALL1" and item["viewer"] == "Americans"
        checks[name + "_authored_lock_unlock"] = item["locked_then_unlocked"]
        checks[name + "_map_binding"] = item["map_sha256"] == sha256(source / "ALL01UMD.MAP") == sha256(source / "install/ALL01UMD.MAP")
        checks[name + "_original_power_plant"] = item["active_types"] == {"GAPOWR": 1}
        runtime = read_json(source / "runtime.json")[-1]
        cells = runtime["cells"]
        checks[name + "_independent_simulation_flags"] = runtime["tick"] == 120 and runtime["fog"] == case["fog"] and runtime["viewer"] == "Americans"
        checks[name + "_independent_sight_exploration"] = cells[0]["visible"] and cells[0]["revealed"] and not cells[1]["visible"] and cells[1]["revealed"] == case["reveal"] and not cells[2]["visible"] and not cells[2]["revealed"]
        checks[name + "_current_sdk"] = sha256(source / "sdk.ts") == sha256(Path(__file__).resolve().parents[1] / "crates/mission-script/sdk/mission.ts")
    sample = lambda fog, reveal, index: facts[f"fog_{str(fog).lower()}_reveal_{str(reveal).lower()}"]["samples"][index]
    energy = lambda row: sum(row["rgb_mean"])
    normal, fogged = sample(False, True, 1), sample(True, True, 1)
    checks["explored_out_of_sight_is_darker_with_fog"] = energy(normal) > 20 and energy(fogged) < energy(normal) * 0.8
    checks["explored_out_of_sight_pixels_differ"] = normal["sha256"] != fogged["sha256"]
    checks["camera_identical_across_controls"] = len({tuple(item["camera"]) for item in facts.values()}) == 1
    checks["sidebar_identical_across_controls"] = len({item["sidebar_sha256"] for item in facts.values()}) == 1
    checks["visible_terrain_brightness_within_ten_percent"] = min(energy(item["samples"][0]) for item in facts.values()) > 20 and max(energy(item["samples"][0]) for item in facts.values()) / min(energy(item["samples"][0]) for item in facts.values()) < 1.10
    checks["unexplored_terrain_black_across_controls"] = all(energy(item["samples"][2]) == 0 for item in facts.values())
    checks["without_reveal_target_is_black"] = all(energy(sample(fog, False, 1)) == 0 for fog in (False, True))
    report = {"passed": all(checks.values()), "checks": checks, "facts": facts,
              "scope": "Four TS campaign tasks, actual ALL1 opening client and raw flat terrain samples. Independent ordinary campaign simulation supplies sight/exploration observations; shell receipt does not expose those cells. No engine edits. Not moving scouts, elevated terrain, network, save compatibility or original-game pixel parity."}
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--analyze-only", action="store_true")
    args = parser.parse_args()
    client, fixture, output = (p.resolve() for p in (args.client, args.fixture, args.output))
    manifest = read_json(fixture / "fixture.json")
    if not args.analyze_only:
        output.mkdir(parents=True, exist_ok=False)
        for case in manifest["cases"]:
            name = case["name"]
            with (output / f"{name}.log").open("wb") as log:
                child = subprocess.run([str(client), "--shell-capture", f"campaign-all1-opening-tick-{case['tick']}",
                                        "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                                        "--shell-capture-timeout-seconds", "120", "--output", str(output / name)],
                                       cwd=fixture / name, stdout=log, stderr=subprocess.STDOUT,
                                       creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
            print(f"{name}: exit {child.returncode}", flush=True)
            if child.returncode:
                raise RuntimeError(f"{name} failed; see retained client log")
        paths = [client, fixture / "fixture.json", Path(__file__).resolve(),
                 Path(__file__).resolve().with_name("verify_mission_visibility.py")]
        for case in manifest["cases"]:
            source = fixture / case["name"]
            paths.extend(source / name for name in ("mission.ts", "sdk.ts", "ALL01UMD.MAP", "runtime.json", "config.toml"))
        (output / "inputs.json").write_text(json.dumps({str(p): sha256(p) for p in paths}, indent=2), encoding="utf-8")
    report = analyze(fixture, output)
    print(json.dumps({"passed": report["passed"], "failed": [k for k, v in report["checks"].items() if not v]}))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
