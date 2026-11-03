"""Capture TS trigger or team media through Windows WASAPI loopback.

Requires PyAudioWPatch and numpy (verification only, never a product dependency).
Run with a new output directory. It launches the fixed client's existing hidden
campaign capture and checks the fixture's distinct music/EVA tones at OS output.
"""
import argparse
import hashlib
import json
import subprocess
import sys
import time
import wave
from pathlib import Path


def tone_report(np, mono, rate, hz, minimum_seconds):
    # 100 ms windows give 10 Hz bins. Sum the target and its adjacent bins to
    # tolerate device resampling; require sustained, dominant nonzero energy.
    size = rate // 10
    windows = len(mono) // size
    if not windows:
        return {"passed": False, "reason": "no complete audio windows"}
    frames = mono[:windows * size].reshape(windows, size).astype(np.float64)
    frames -= frames.mean(axis=1, keepdims=True)
    rms = np.sqrt(np.mean(frames * frames, axis=1))
    power = np.abs(np.fft.rfft(frames * np.hanning(size), axis=1)) ** 2
    frequencies = np.fft.rfftfreq(size, 1 / rate)
    band = np.abs(frequencies - hz) <= 10
    total = power.sum(axis=1)
    share = power[:, band].sum(axis=1) / np.maximum(total, 1e-30)
    matches = (rms > 1e-5) & (share >= 0.65)
    indices = np.flatnonzero(matches)
    seconds = float(matches.sum() * size / rate)
    return {
        "passed": seconds >= minimum_seconds,
        "hz": hz,
        "dominant_seconds": seconds,
        "required_seconds": minimum_seconds,
        "first_seconds": float(indices[0] * size / rate) if len(indices) else None,
        "last_seconds": float((indices[-1] + 1) * size / rate) if len(indices) else None,
        "peak_window_rms": float(rms.max()),
        "peak_target_energy_fraction": float(share.max()),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--python-deps", type=Path)
    parser.add_argument("--team-media", action="store_true",
                        help="check Script25/27 sound/music and normal input recovery")
    parser.add_argument("--timeout", type=float, default=90)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("WASAPI loopback requires Windows")
    if not 1 <= args.timeout <= 120:
        parser.error("timeout must be between 1 and 120 seconds")
    if args.python_deps:
        sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    import pyaudiowpatch as pa

    client, fixture, output = (p.resolve() for p in (args.client, args.fixture, args.output))
    if not client.is_file():
        parser.error("client executable does not exist")
    source = (fixture / "mission.ts").read_text(encoding="utf-8")
    required = ("order.playSound(m.assetSound('SdkTone'))", "order.playMusic(m.assetTheme('SdkMusic'))") if args.team_media else ("m.assetTheme('SdkMusic')", "m.assetEva('SdkEva')")
    if not all(text in source for text in required):
        parser.error("fixture does not contain the requested authored audio path")
    if output.exists():
        parser.error("output directory must be new")
    output.mkdir(parents=True)
    chunks = []
    callback_flags = []
    started = time.monotonic()
    with pa.PyAudio() as host:
        device = host.get_default_wasapi_loopback()
        rate = int(device["defaultSampleRate"])
        channels = int(device["maxInputChannels"])

        def record(data, count, timing, flags):
            if flags:
                callback_flags.append(int(flags))
            chunks.append(data)
            return (None, pa.paContinue)

        with host.open(format=pa.paFloat32, channels=channels, rate=rate,
                       input=True, input_device_index=device["index"],
                       frames_per_buffer=1024, stream_callback=record):
            time.sleep(0.5)
            with (output / "client.log").open("wb") as log:
                child = subprocess.Popen([
                    str(client), "--shell-capture", "campaign-sov1-opening-realtime-unlocked" if args.team_media else "campaign-all1-opening-tick-720",
                    "--width", "800", "--height", "600", "--cursor-x", "400",
                    "--cursor-y", "300", "--shell-capture-timeout-seconds", str(int(args.timeout)),
                    "--output", str(output / "capture"),
                ], cwd=fixture, stdout=log, stderr=subprocess.STDOUT,
                    creationflags=subprocess.CREATE_NO_WINDOW)
                timed_out = False
                try:
                    child.wait(timeout=args.timeout + 5)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    child.terminate()
                    child.wait(timeout=10)
            time.sleep(0.25)

    samples = np.frombuffer(b"".join(chunks), dtype="<f4").copy()
    finite = bool(np.isfinite(samples).all())
    frames = samples.reshape(-1, channels)
    # Write bounded diagnostic PCM while keeping the analysis on full floats.
    with wave.open(str(output / "loopback.wav"), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((np.clip(np.nan_to_num(frames), -1, 1) * 32767).astype("<i2").tobytes())
    mono = np.nan_to_num(frames).mean(axis=1)
    tones = {
        "sound": tone_report(np, mono, rate, 440, 0.1),
        "music": tone_report(np, mono, rate, 660, 0.6),
    }
    if not args.team_media:
        tones["eva"] = tone_report(np, mono, rate, 880, 0.4)
    receipt_path = output / "capture" / "capture.json"
    receipt = json.loads(receipt_path.read_text(encoding="utf-8")) if receipt_path.exists() else {}
    runtime_log = fixture / "logs/ra2.log"
    if runtime_log.exists():
        (output / "runtime.log").write_bytes(runtime_log.read_bytes())
    opening = receipt.get("opening", {})
    eva_observed = any(
        observation.get("eva_active") and (observation.get("eva_event") or "").upper() == "SDKEVA"
        for observation in opening.get("observations", [])
    )
    first, second = ("sound", "music") if args.team_media else ("music", "eva")
    order = (tones[first].get("first_seconds") is not None
             and tones[second].get("first_seconds") is not None
             and tones[first]["first_seconds"] < tones[second]["first_seconds"])
    team_checks = {}
    if args.team_media:
        contract = json.loads((fixture / "team-media-runtime-report.json").read_text(encoding="utf-8"))
        movies = [row for row in receipt.get("route", []) if row.get("action") == "movie Escape release"]
        expects_movie = "order.playMovie(m.movie(Movie.A01_P01e))" in source
        team_checks = {
            "ordinary_frame_request_contract": contract["passed"],
            "fullscreen_first_frame_presented_then_skipped": len(movies) == int(expects_movie)
                and all(row["movie_frame"] == 0 for row in movies),
        }
        markers = [message["text"] for row in opening.get("observations", []) for message in row.get("messages", [])]
        authored_path = (all(text in markers for text in ("SDK team sound", "SDK team music", "SDK team movie returned"))
                         and opening.get("clock_domain") == "ordinary wall-clock frame pacing and audio service"
                         and opening.get("exact_steps") == []
                         and opening.get("saw_unlock")
                         and opening.get("unlocked_arrow_probe", {}).get("press_admitted")
                         and opening.get("unlocked_arrow_probe", {}).get("release_cleared")
                         and opening.get("unlocked_mouse_probe", {}).get("released_buttons_admit_right_pan")
                         and all(team_checks.values()))
    else:
        authored_path = opening.get("current", {}).get("tick") == 720 and eva_observed
    passed = (not timed_out and child.returncode == 0 and finite and not callback_flags
              and authored_path and order and all(tone["passed"] for tone in tones.values()))
    report = {
        "passed": passed, "client_exit": child.returncode, "timed_out": timed_out,
        "client": str(client), "client_sha256": hashlib.sha256(client.read_bytes()).hexdigest(),
        "device": device["name"], "sample_rate": rate, "channels": channels,
        "finite": finite, "callback_flags": callback_flags,
        "recorded_seconds": len(frames) / rate, "elapsed_seconds": time.monotonic() - started,
        "source_sha256": hashlib.sha256(source.encode()).hexdigest(),
        "tones": tones, "tone_order": [first, second], "tone_order_passed": order,
        "authored_path_passed": authored_path, "eva_observed_in_client": eva_observed,
        "team_checks": team_checks,
        "client_tick": opening.get("current", {}).get("tick"),
        "scope": "OS WASAPI output loopback of the generated TS campaign. Distinct custom tones verify team sound/music or trigger music/EVA output. Team captures check the existing client's fullscreen first-frame presentation followed by its automatic Escape, plus ordinary input recovery. Not natural movie completion or in-movie pixel comparison. Audio positions use the recorded sample timeline; WASAPI may omit inactive device periods. Not a speaker/listener judgment, retail fidelity, or internal Player PCM.",
    }
    if not args.team_media:
        report["music_precedes_eva"] = order
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(output / "report.json")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
