"""Real subprocess audit of the headless CLI. Uses locally generated P5 maps."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game-dir", type=Path, required=True)
    parser.add_argument("--maps", type=Path, default=Path("target/p5-compatibility"))
    parser.add_argument("--output", type=Path, default=Path("target/p5-cli"))
    parser.add_argument("--exe", type=Path, default=Path("target/debug/ra2-map-editor.exe"))
    args = parser.parse_args()
    game, maps, output, exe = (p.resolve() for p in (args.game_dir, args.maps, args.output, args.exe))
    output.mkdir(parents=True, exist_ok=True)
    source = maps / "compat-TEMPERATE.map"
    original = hashlib.sha256(source.read_bytes()).hexdigest()
    profile = output / "profile.json"
    # Exercise profile and preset paths relative to their files, using a
    # different working directory for every child process.
    import os
    try:
        game_path = os.path.relpath(game, output)
    except ValueError:  # Windows installations on another drive.
        game_path = str(game)
    profile.write_text(json.dumps({"version": 1, "name": "CLI audit", "schema": "vera20k-pinned-yr-v1", "game_dir": game_path}), encoding="utf-8")
    preset = output / "preset.json"
    preset.write_text(json.dumps({"version": 1, "profile": "profile.json", "output": "preset-output", "max_edge": 256}), encoding="utf-8")
    cases = []

    def run(name, command, expected, *options, report=True):
        report_path = output / f"{name}.json"
        cmd = [str(exe), command, "--preset", str(preset), *map(str, options)]
        if report:
            cmd.extend(["--report", str(report_path)])
        result = subprocess.run(cmd, cwd=exe.parent, capture_output=True, timeout=60)
        (output / f"{name}.stdout.log").write_bytes(result.stdout)
        (output / f"{name}.stderr.log").write_bytes(result.stderr)
        assert result.returncode == expected, (name, result.returncode, result.stderr.decode(errors="replace"))
        data = json.loads(report_path.read_text(encoding="utf-8")) if report else None
        cases.append({"case": name, "exit_code": result.returncode, "passed": True, "report": str(report_path) if report else None})
        return data

    valid = run("validate", "validate", 0, source)
    assert valid["passed"] and valid["profile"]["resources"]
    mixed = run("batch", "validate", 2, source, output / "missing.map")
    assert len(mixed["maps"]) == 2 and mixed["maps"][0]["passed"] and not mixed["maps"][1]["passed"]
    exported = run("export", "export", 0, "--overwrite", "--max-edge", "512", source)
    assert exported["preset"]["max_edge"] == 512
    png = Path(exported["maps"][0]["output"])
    assert png.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"
    import struct
    assert max(struct.unpack(">II", png.read_bytes()[16:24])) == 512
    published = run("publish", "publish", 0, "--overwrite", maps / "compat-SNOW.map")
    published_map = Path(published["maps"][0]["output"])
    sidecar = json.loads(Path(str(published_map) + ".yr-editor.json").read_text(encoding="utf-8"))
    assert b"[PreviewPack]" in published_map.read_bytes() and sidecar["resource_profile"] == published["profile"]
    run("no-overwrite", "publish", 2, maps / "compat-SNOW.map")
    run("duplicate-stems", "publish", 1, "--output", output / "duplicates", source, maps / "a" / source.name, report=False)
    run("input-report", "validate", 1, "--report", source, source, report=False)
    run("output-report", "export", 1, "--report", output / "preset-output" / "compat-TEMPERATE.png", source, report=False)
    run("input-publish", "publish", 1, "--overwrite", "--output", maps, source, report=False)
    missing = output / "missing-resource.map"
    missing.write_bytes(source.read_bytes().replace(b"[Basic]", b"[Basic]\nBrief=EDITOR_AUDIT_MISSING_MEDIA", 1))
    dependency = run("missing-dependency", "validate", 2, missing)
    assert not dependency["maps"][0]["dependencies"]["passed"]
    run("skip-dependency", "validate", 0, "--no-dependencies", missing)
    assert hashlib.sha256(source.read_bytes()).hexdigest() == original, "input changed"
    audit = {"passed": True, "source_sha256_unchanged": original, "cases": cases}
    (output / "audit.json").write_text(json.dumps(audit, indent=2), encoding="utf-8")
    print(output / "audit.json")


if __name__ == "__main__":
    main()
