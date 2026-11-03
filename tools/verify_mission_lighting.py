"""Capture TS lighting fixtures with the existing fixed Battle GPU observer.

Keeps raw frames/receipts and PNG previews. Uses Python's standard library;
tests terrain pixels independently of the reported palette inputs.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

from verify_mission_visibility import write_png


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(game, output):
    install, settings = output / "install", output / "settings"
    install.mkdir()
    settings.mkdir()
    for source in game.iterdir():
        if not source.is_file():
            continue
        target = install / source.name
        if source.suffix.lower() == ".mix":
            os.link(source, target)
        elif source.suffix.lower() in {".ini", ".csf", ".pal", ".fnt"}:
            shutil.copy2(source, target)
    (output / "config.toml").write_text(
        "[paths]\nra2_dir = " + json.dumps(str(install))
        + "\nsettings_dir = " + json.dumps(str(settings)) + "\n", encoding="utf-8")
    (settings / "VERA.INI").write_text(
        "[Intro]\nPlay=no\n[VERA]\nSkipStartupLogo=yes\nFullScreen=no\nLockCursor=no\n",
        encoding="utf-8")


def observe(folder):
    receipt = read_json(folder / "capture.json")
    frame = receipt["frame"]
    raw = (folder / "frame.bgra").read_bytes()
    if (frame["pixel_layout"], frame["width"], frame["height"]) != ("BGRA8", 1024, 768):
        raise ValueError("unexpected frame format")
    if len(raw) != frame["height"] * frame["row_stride"] or hashlib.sha256(raw).hexdigest() != frame["sha256"]:
        raise ValueError("frame integrity failure")
    write_png(folder / "preview.png", raw, frame["width"], frame["height"], frame["row_stride"])
    terrain = receipt["observations"]["frames"][-1]["terrain"]
    crops, cells = [], []
    for cell in terrain:
        render = cell["render"]
        center, camera = render["cell_center_world"], render["camera_top_left"]
        x, y = (round(center[i] - camera[i]) for i in range(2))
        if not (15 <= x < 840 and 8 <= y < 730):
            raise ValueError("terrain sample outside unobstructed viewport")
        crop = b"".join(raw[row * frame["row_stride"] + (x - 10) * 4:
                            row * frame["row_stride"] + (x + 10) * 4]
                        for row in range(y - 4, y + 4))
        crops.append(crop)
        cells.append({"cell": cell["cell"], "palette_light": render["palette_light"],
                      "atlas": render["atlas_key"], "visible": cell["local_visibility"],
                      "a_center": render["a_center"], "pixel_center": [x, y]})
    pixels = b"".join(crops)
    count = len(pixels) // 4
    rgb = [sum(pixels[channel::4]) / count for channel in (2, 1, 0)]
    # The header, logo and empty build slots are UI; exclude the changing
    # superweapon timer region and bottom controls.
    sidebar = b"".join(raw[y * frame["row_stride"] + 860 * 4:
                           y * frame["row_stride"] + 1020 * 4] for y in range(0, 700))
    return {"complete": receipt["status"] == "COMPLETE", "tick": receipt["final"]["simulation_tick"],
            "hidden": receipt["lifecycle"]["window_hidden"], "render_ready": receipt["render"]["ready"],
            "source_sha256": receipt["map_source"]["source_sha256"], "cells": cells,
            "rgb_mean": rgb, "terrain_sha256": hashlib.sha256(pixels).hexdigest(),
            "sidebar_sha256": hashlib.sha256(sidebar).hexdigest(),
            "command_results": receipt["observations"].get("commands", []),
            "shroud": receipt["loaded_session"]["options"]["shroud"],
            "fog_of_war": receipt["loaded_session"]["options"]["fog_of_war"]}


def analyze(fixture, output):
    manifest = read_json(fixture / "fixture.json")
    facts, checks = {}, {}
    for case in manifest["cases"]:
        name = f"{case['name']}-{case['tick']}"
        item = observe(output / f"capture-{name}")
        facts[name] = item
        checks[name + "_complete"] = item["complete"] and item["hidden"] and item["render_ready"] and item["tick"] == case["tick"]
        checks[name + "_source"] = item["source_sha256"] == sha256(fixture / case["map"])
        # With both options off, the renderer uses revealed terrain at full
        # A=127 even outside current unit sight; visible can correctly be false.
        checks[name + "_four_unshrouded_samples"] = not item["shroud"] and not item["fog_of_war"] and len(item["cells"]) == 4 and all(c["visible"]["revealed"] and c["a_center"] == 127 for c in item["cells"])
    control = facts["control-0"]
    checks["dim_pixels_darker"] = sum(facts["dim-0"]["rgb_mean"]) < sum(control["rgb_mean"]) * 0.65
    for channel, name in enumerate(("red", "green", "blue")):
        rgb = facts[f"{name}-0"]["rgb_mean"]
        checks[name + "_pixels_channel"] = rgb[channel] > 10 and all(rgb[c] < 1 for c in range(3) if c != channel)
        checks[name + "_sidebar_unaffected"] = facts[f"{name}-0"]["sidebar_sha256"] == control["sidebar_sha256"]
    checks["dim_sidebar_unaffected"] = facts["dim-0"]["sidebar_sha256"] == control["sidebar_sha256"]
    checks["authored_pixels_differ"] = facts["authored-0"]["terrain_sha256"] != control["terrain_sha256"]
    checks["authored_palette_differs"] = facts["authored-0"]["cells"][0]["palette_light"] != control["cells"][0]["palette_light"]
    for name, tick, returned in (("storm", 300, 1000), ("nuke_fast", 260, 500), ("dominator_fast", 60, 500)):
        observed, normal = facts[f"{name}-{tick}"], facts[f"authored-{tick}"]
        checks[name + "_terrain_pixels_change"] = observed["terrain_sha256"] != normal["terrain_sha256"]
        checks[name + "_terrain_palette_changes"] = observed["cells"][0]["palette_light"] != normal["cells"][0]["palette_light"]
        if name != "dominator_fast":
            checks[name + "_terrain_returns"] = facts[f"{name}-{returned}"]["terrain_sha256"] == facts[f"authored-{returned}"]["terrain_sha256"]
    # PsyDom's FadingBack->Idle publishes a relight profile without refreshing
    # Cells. Its final scalar retains DominatorGround. Compare actual pixels
    # to an independent TS map with precisely that Ground value.
    returned = facts["dominator_fast-500"]
    retained = facts["dominator_retained_control-500"]
    checks["dominator_returned_rgb"] = returned["cells"][0]["palette_light"][:3] == facts["authored-500"]["cells"][0]["palette_light"][:3]
    checks["dominator_retained_ground_palette"] = [c["palette_light"] for c in returned["cells"]] == [c["palette_light"] for c in retained["cells"]]
    checks["dominator_retained_ground_pixels"] = returned["terrain_sha256"] == retained["terrain_sha256"]
    rows = read_json(fixture / "dominator_fast/runtime.json")
    normal = next(row["lighting"] for row in rows if row["tick"] == 500)
    checks["dominator_simulation_ambient_returned"] = normal["current_ambient"] == 61 and normal["target_ambient"] == 61 and normal["selected_profile"] == "Normal"
    checks["dominator_not_initial_cell_pixels"] = returned["terrain_sha256"] != facts["authored-500"]["terrain_sha256"]
    for name, tick in (("nuke", 260), ("dominator", 60)):
        checks[name + "_interval_changes_pixels"] = facts[f"{name}_fast-{tick}"]["terrain_sha256"] != facts[f"{name}_slow-{tick}"]["terrain_sha256"]
    for name in ("dominator_fast-60", "dominator_slow-60"):
        commands = facts[name]["command_results"]
        checks[name + "_ordinary_launch"] = len(commands) == 1 and commands[0]["bindings"]["/LaunchSuperWeapon/sw_type_id"]["type_id"] == "PsychicDominatorSpecial" and commands[0]["owner"] == "SDK lighting"
    report = {"passed": all(checks.values()), "checks": checks, "facts": facts,
              "scope": "Current fixed client, TS-created maps, existing Skirmish GPU capture, four terrain pixel samples, RGB/ambient inputs, transient effects and fade intervals. Dominator returns simulation ambient/RGB but retains per-cell Ground until a later refresh; independent control checks that behavior. No engine modifications. Not original-game parity, campaign GPU or full-state equivalence."}
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
    if not args.analyze_only:
        if output.exists():
            parser.error("output directory must be new")
        output.mkdir(parents=True)
        prepare(game, output)
        for case in read_json(fixture / "fixture.json")["cases"]:
            name = f"{case['name']}-{case['tick']}"
            with (output / f"client-{name}.log").open("wb") as log:
                child = subprocess.run([str(client), "--tactical-capture", "map-observe-v1", "--profile", str(fixture / case["profile"]), "--contract", str(contract), "--output", str(output / f"capture-{name}")], cwd=output, stdout=log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW, timeout=130)
            print(f"capture {name}: {child.returncode}", flush=True)
            if child.returncode:
                raise RuntimeError(f"capture {name} failed; inspect retained log")
        paths = [client, contract, fixture / "fixture.json", Path(__file__).resolve(), output / "config.toml"]
        for case in read_json(fixture / "fixture.json")["cases"]:
            paths.extend([fixture / case["profile"], fixture / case["map"], fixture / case["name"] / "mission.ts", fixture / case["name"] / "sdk.ts", fixture / case["name"] / "runtime.json"])
        (output / "inputs.json").write_text(json.dumps({str(p): sha256(p) for p in paths}, indent=2), encoding="utf-8")
    report = analyze(fixture, output)
    print(json.dumps({"passed": report["passed"], "failed": [k for k, v in report["checks"].items() if not v]}))
    raise SystemExit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
