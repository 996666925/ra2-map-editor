"""Run a generated TS multiplayer map through two ordinary LAN clients.

Uses existing lan-live-host/guest capture inputs, UDP transport, loading and
frame boundary. Each peer gets isolated mutable files and settings; immutable
MIX archives are hardlinked read-only by convention, never modified by this tool.
Requires the full retail install and the existing multiplayer audit fixture.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time


MARKER = b"VERA20k isolated shell capture fixture\n"
MAP_NAME = "sdk-network.yrm"


def verify_pair(report):
    """Compare real transport receipts, without comparing local UI/source IDs."""
    checks = report["checks"]
    checks["two_normal_process_exits"] = report["exit_codes"] == [0, 0]
    checks["no_forced_termination"] = not report["terminated_owned_processes"]
    checks["two_receipts"] = set(report["receipts"]) == {"host", "guest"}
    if not checks["two_receipts"]:
        return
    lanes = {role: receipt["lan"] for role, receipt in report["receipts"].items()}
    for role, lan in lanes.items():
        checks[role + "_two_rounds"] = len(lan["rounds"]) == 2
        menu = lan["menu_return"]
        checks[role + "_menu_cleanup"] = lan["returned_to_menu"] and all(
            menu[key] is True for key in ("settled", "scenario_retired", "session_retired",
                                         "setup_retired", "transport_retired", "visits_retired"))
    if not all(checks[role + "_two_rounds"] for role in lanes):
        return
    for index, (host, guest) in enumerate(zip(lanes["host"]["rounds"], lanes["guest"]["rounds"])):
        prefix = f"round{index}_"
        hr, gr = host["runtime"], guest["runtime"]
        checks[prefix + "same_seed"] = hr["startup"]["seed"] == gr["startup"]["seed"]
        checks[prefix + "different_processes"] = hr["process_id"] != gr["process_id"]
        checks[prefix + "native_300_equal"] = (len(hr["native_checksums"]) == 300
            and hr["native_checksums"] == gr["native_checksums"])
        checks[prefix + "strong_20_equal"] = (hr["simulation_hashes"] == gr["simulation_hashes"]
            and [row[0] for row in hr["simulation_hashes"]] == list(range(15, 301, 15)))
        checks[prefix + "mobile_objects_equal"] = bool(hr["mobile_objects"]) and hr["mobile_objects"] == gr["mobile_objects"]
        checks[prefix + "terminal_equal"] = host["terminal_simulation"] == guest["terminal_simulation"]
        terminal = host["terminal_simulation"]
        checks[prefix + "terminal_recorded"] = (terminal["tick"] >= 300
            and terminal["recorded_hashes"][-1] == [terminal["tick"], terminal["hash"]]
            and {"trigger_runtime", "team_script_vm"} <= {row[0] for row in terminal["state_hash_owners"]})
        checks[prefix + "same_results_and_return_to_room"] = (host["result"] == guest["result"]
            and host["returned_to_room"] and guest["returned_to_room"])
        checks[prefix + "same_effective_map"] = (hr["map_admission"]["effective_digest"]
            == gr["map_admission"]["effective_digest"])
        for role, runtime in (("host", hr), ("guest", gr)):
            key = prefix + role + "_"
            admission = runtime["map_admission"]
            checks[key + "authored_map_bytes"] = (admission["file"] == MAP_NAME
                and admission["generated"] is False and admission["capacity"] == 3
                and admission["source_sha256"] == report["inputs"]["multiplayer.map"])
            checks[key + "two_live_participants"] = (runtime["participant_count"] == 2
                and runtime["active_participant_count"] == 2 and runtime["cpu_count"] == 0)
            checks[key + "no_network_failure"] = (runtime["failure"] is None
                and runtime["checksum_failure"] == "None")
            actions = runtime["actions"]
            checks[key + "move_stop_through_maxahead"] = (len(actions) == 2
                and [a["input"] for a in actions] == ["tactical Move", "Stop"]
                and all(a["applied"] and a["delay_frames"] == 10
                    and a["application_frame"] == a["native_frame"] + 10 for a in actions)
                and runtime["observed_movement"] is True and runtime["stopped_position"] is not None)


def verify_replays(report, output, auditor):
    """Bind readonly SDK observations to each real round's recorded final hash."""
    checks, audits = report["checks"], {}
    report["replay_audits"] = audits
    for role in ("host", "guest"):
        root = output / role
        recordings = sorted((root / "replays").glob("*.json"))
        checks[role + "_two_recordings"] = len(recordings) == 2
        audits[role] = []
        for index, recording in enumerate(recordings):
            path = root / f"replay-sdk-{index}.json"
            command = [str(auditor), str(root / "install"), str(recording), str(path)]
            report["commands"].append(command)
            try:
                with (root / f"replay-sdk-{index}.log").open("wb") as log:
                    child = subprocess.run(command, stdout=log, stderr=log, timeout=90,
                                           creationflags=subprocess.CREATE_NO_WINDOW)
                audit = json.loads(path.read_text(encoding="utf-8"))
                ok = child.returncode == 0 and audit["passed"] is True
            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                audit, ok = {"passed": False, "error": str(error)}, False
            audits[role].append(audit)
            key = f"round{index}_{role}_"
            checks[key + "strict_replay_and_sdk"] = ok
            if not ok:
                continue
            rounds = report["receipts"][role]["lan"]["rounds"]
            if index >= len(rounds):
                checks[key + "replay_matches_live_round"] = False
                continue
            live = rounds[index]
            verification = audit["verification"]
            checks[key + "replay_matches_live_round"] = (
                audit["seed"] == live["runtime"]["startup"]["seed"]
                and audit["map_sha256"] == report["inputs"]["multiplayer.map"]
                and verification["final_tick"] == live["terminal_simulation"]["tick"]
                and verification["final_state_hash"] == live["terminal_simulation"]["hash"]
                and verification["checked_hashes"] == len(live["terminal_simulation"]["recorded_hashes"]))
    for index in range(2):
        if all(len(audits[role]) > index and audits[role][index].get("passed") is True for role in audits):
            host, guest = audits["host"][index], audits["guest"][index]
            for field in ("at_300", "births", "stages"):
                checks[f"round{index}_sdk_{field}_equal"] = host[field] == guest[field]
        else:
            checks[f"round{index}_sdk_pair_equal"] = False


def prepare(game, fixture, root):
    install, settings = root / "install", root / "settings"
    install.mkdir(parents=True)
    settings.mkdir()
    for source in game.iterdir():
        if not source.is_file():
            continue
        target = install / source.name
        if source.suffix.lower() == ".mix":
            os.link(source, target)
        elif source.suffix.lower() in {".ini", ".csf", ".map", ".mmx", ".mpr", ".pkt", ".pal"}:
            shutil.copy2(source, target)
    for directory in (install, settings):
        (directory / ".vera20k-lan-capture-root").write_bytes(MARKER)
    (install / MAP_NAME).write_bytes((fixture / "multiplayer.map").read_bytes())
    (settings / "VERA.INI").write_text(
        "[Intro]\nPlay=no\n[VERA]\nSkipStartupLogo=yes\nFullScreen=no\nLockCursor=no\n",
        encoding="utf-8")
    (root / "config.toml").write_text(
        "[paths]\nsettings_dir = " + json.dumps(str(settings))
        + "\nra2_dir = " + json.dumps(str(install)) + "\n", encoding="utf-8")
    return install


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--replay-audit", type=Path, required=True,
                        help="built mission_sdk_lan_replay_audit example executable")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--host-port", type=int, default=33120)
    parser.add_argument("--guest-port", type=int, default=33130)
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("this runner currently uses Windows process flags")
    game, fixture, client, output = (p.resolve() for p in (args.game, args.fixture, args.client, args.output))
    auditor = args.replay_audit.resolve()
    if not all(path.is_file() for path in (client, auditor, fixture / "multiplayer.map")):
        parser.error("client, replay auditor and generated fixture must exist")
    if output.exists():
        parser.error("output directory must be new")
    ports = [args.host_port, args.host_port + 2, args.guest_port, args.guest_port + 2]
    if len(set(ports)) != 4 or not all(1024 <= port <= 65535 for port in ports):
        parser.error("four distinct primary/alternate ports above 1023 are required")
    if not 30 <= args.timeout <= 300:
        parser.error("timeout must be between 30 and 300 seconds")
    sockets = []
    try:
        for port in ports:
            probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            sockets.append(probe)
            probe.bind(("127.0.0.1", port))
    finally:
        for probe in sockets:
            probe.close()
    report = {"client_sha256": hashlib.sha256(client.read_bytes()).hexdigest(),
              "replay_audit_sha256": hashlib.sha256(auditor.read_bytes()).hexdigest(),
              "verifier_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "inputs": {name: hashlib.sha256((fixture / name).read_bytes()).hexdigest()
                         for name in ("multiplayer.map", "mission.ts", "sdk.ts")}, "commands": []}
    output.mkdir(parents=True)
    processes, logs = [], []
    started = time.monotonic()
    try:
        for role, local, peer in (("host", args.host_port, args.guest_port),
                                   ("guest", args.guest_port, args.host_port)):
            root = output / role
            install = prepare(game, fixture, root)
            command = [str(client), "--shell-capture", "lan-live-" + role,
                       "--width", "800", "--height", "600", "--cursor-x", "400", "--cursor-y", "300",
                       "--output", str(root / "capture"), "--lan-bind", f"127.0.0.1:{local}",
                       "--lan-peer", f"127.0.0.1:{peer}", "--shell-capture-timeout-seconds", str(args.timeout)]
            if role == "host":
                command += ["--lan-map", MAP_NAME]
            report["commands"].append(command)
            environment = os.environ.copy()
            environment["RA2_DIR"] = str(install)
            environment["RUST_LOG"] = "info"
            log = (root / "client.log").open("wb")
            logs.append(log)
            processes.append(subprocess.Popen(command, cwd=root, env=environment,
                                                stdout=log, stderr=log,
                                                creationflags=subprocess.CREATE_NO_WINDOW))
        while time.monotonic() - started < args.timeout + 10:
            codes = [child.poll() for child in processes]
            if all(code is not None for code in codes) or any(code not in (None, 0) for code in codes):
                break
            time.sleep(0.25)
    finally:
        report["terminated_owned_processes"] = []
        for child in processes:
            if child.poll() is None:
                report["terminated_owned_processes"].append(child.pid)
                child.terminate()
                child.wait(timeout=10)
        for log in logs:
            log.close()
    report["exit_codes"] = [child.returncode for child in processes]
    report["elapsed_seconds"] = time.monotonic() - started
    report["receipts"] = {}
    for role in ("host", "guest"):
        path = output / role / "capture/capture.json"
        if path.exists():
            report["receipts"][role] = json.loads(path.read_text(encoding="utf-8"))
    report["checks"] = {}
    try:
        verify_pair(report)
        if len(report["receipts"]) == 2:
            verify_replays(report, output, auditor)
    except (KeyError, TypeError, IndexError, ValueError) as error:
        report["checks"]["valid_receipt_schema"] = False
        report["verification_error"] = str(error)
    report["passed"] = bool(report["checks"]) and all(report["checks"].values())
    report["scope"] = "two owned App processes on loopback UDP; two rounds, 300 native checksums, strong hashes and terminal owners; readonly SDK timing/team observations from four strictly verified actual recordings linked to live terminal hashes. No separate-machine, WOL transport, network save/load or original SAV compatibility claim."
    (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
