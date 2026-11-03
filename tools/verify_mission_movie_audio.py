"""Compare a TS team movie's WASAPI output with an independent FFmpeg decode.

Runs the ordinary visible campaign-window verifier and its next() control.
Requires Windows, FFmpeg, numpy and PyAudioWPatch, only for local verification.
The reference BIK must be exported by --team-media from production asset lookup.
Does not change engine code, original game files, or SDK playback behavior.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import wave

from verify_mission_movie_window import run


def decode(ffmpeg, arguments):
    result = subprocess.run([str(ffmpeg), "-v", "error", "-nostdin", *arguments,
                             "-ac", "1", "-ar", "8000", "-c:a", "pcm_f32le",
                             "-f", "f32le", "pipe:1"], capture_output=True, check=True)
    return result.stdout


def capture_time(index, packets, device_rate):
    input_index = int(index * device_rate / 8000)
    packet = next((p for p in packets if p["sample_start"] <= input_index < p["sample_start"] + p["count"]), None)
    return None if packet is None else packet["capture_start_perf_counter"] + (input_index - packet["sample_start"]) / device_rate


def match(np, recorded, reference, packets, device_rate, offset, length):
    """Normalized FFT cross-correlation, retaining both timing and strength."""
    template = reference[int(offset * 8000):int((offset + length) * 8000)].astype(np.float64)
    template -= template.mean()
    signal = recorded.astype(np.float64)
    count, width = len(signal), len(template)
    if count < width or np.linalg.norm(template) < 1e-8:
        raise ValueError("reference or loopback is empty/silent/too short")
    size = 1 << (count + width - 2).bit_length()
    numerator = np.fft.irfft(np.fft.rfft(signal, size)
                             * np.fft.rfft(template[::-1], size), size)[width - 1:count]
    sums = np.concatenate(([0.0], np.cumsum(signal)))
    squares = np.concatenate(([0.0], np.cumsum(signal * signal)))
    variance = squares[width:] - squares[:-width] - (sums[width:] - sums[:-width]) ** 2 / width
    energy = np.sqrt(np.maximum(variance, 0)) * np.linalg.norm(template)
    correlations = np.divide(numerator, energy, out=np.zeros_like(numerator), where=energy > 1e-8)
    index = int(np.argmax(np.abs(correlations)))
    return {"correlation": float(correlations[index]), "absolute_correlation": float(abs(correlations[index])),
            "recorded_sample_seconds": index / 8000,
            "match_start_perf_counter": capture_time(index, packets, device_rate),
            "reference_offset_seconds": offset, "reference_length_seconds": length,
            "reference_rms": float(np.sqrt(np.mean(template * template)))}


def spectral_match(np, recorded, reference, packets, device_rate, offset, length):
    """Find the full time/frequency fingerprint, allowing output phase changes.

    64 ms Hann windows, 16 ms hop, 47..3422 Hz magnitudes; Pearson correlation
    over every bin of the whole segment, searched across the complete recording.
    This identifies the soundtrack without asserting matching PCM phase/samples.
    """
    def spectrum(signal):
        windows = np.lib.stride_tricks.sliding_window_view(signal, 512)[::128]
        return np.abs(np.fft.rfft(windows * np.hanning(512), axis=1))[:, 3:220]

    template = spectrum(reference[int(offset * 8000):int((offset + length) * 8000)])
    signal = spectrum(recorded)
    count, width, bins = len(signal), len(template), template.shape[1]
    size = 1 << (count + width - 2).bit_length()
    numerator = np.fft.irfft(np.fft.rfft(signal, size, axis=0)
                             * np.fft.rfft(template[::-1], size, axis=0), size, axis=0).sum(axis=1)[width - 1:count]
    sums = np.concatenate(([0.0], np.cumsum(signal.sum(axis=1))))
    squares = np.concatenate(([0.0], np.cumsum((signal * signal).sum(axis=1))))
    local_sum = sums[width:] - sums[:-width]
    elements = width * bins
    numerator -= local_sum * template.sum() / elements
    variance = squares[width:] - squares[:-width] - local_sum * local_sum / elements
    energy = np.sqrt(np.maximum(variance, 0)) * np.linalg.norm(template - template.mean())
    correlations = np.divide(numerator, energy, out=np.zeros_like(numerator), where=energy > 1e-8)
    index = int(np.argmax(correlations))
    return {"correlation": float(correlations[index]), "recorded_sample_seconds": index * 128 / 8000,
            "match_start_perf_counter": capture_time(index * 128, packets, device_rate),
            "reference_offset_seconds": offset, "reference_length_seconds": length}


def capture(np, pa, client, fixture, output, control, ffmpeg, reference, window_runner=run):
    chunks, packets, flags = [], [], []
    sample_count = 0
    with pa.PyAudio() as host:
        device = host.get_default_wasapi_loopback()
        rate, channels = int(device["defaultSampleRate"]), int(device["maxInputChannels"])

        def receive(data, count, timing, status):
            nonlocal sample_count
            arrival = time.perf_counter()
            if status:
                flags.append(int(status))
            # Map PortAudio's ADC time into the same monotonic clock as PNGs.
            # Keep each packet's mapping: inactive WASAPI periods can be absent.
            start = arrival + timing["input_buffer_adc_time"] - timing["current_time"]
            packets.append({"sample_start": sample_count, "count": count,
                            "capture_start_perf_counter": start,
                            "callback_perf_counter": arrival, "timing": timing})
            sample_count += count
            chunks.append(data)
            return None, pa.paContinue

        with host.open(format=pa.paFloat32, channels=channels, rate=rate, input=True,
                       input_device_index=device["index"], frames_per_buffer=1024,
                       stream_callback=receive):
            # run() owns the output directory, process, window and setting cleanup.
            window_report = window_runner(client, fixture, output, control)
            time.sleep(0.25)

    raw = b"".join(chunks)
    (output / "loopback.f32").write_bytes(raw)
    (output / "packets.json").write_text(json.dumps(packets, indent=2), encoding="utf-8")
    frames = np.frombuffer(raw, dtype="<f4").reshape(-1, channels)
    finite = bool(np.isfinite(frames).all())
    with wave.open(str(output / "loopback.wav"), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(rate)
        wav.writeframes((np.clip(np.nan_to_num(frames), -1, 1) * 32767).astype("<i2").tobytes())
    reduced = decode(ffmpeg, ["-f", "f32le", "-ar", str(rate), "-ac", str(channels),
                              "-i", str(output / "loopback.f32")])
    recorded = np.frombuffer(reduced, dtype="<f4")
    matches = [match(np, recorded, reference, packets, rate, offset, length)
               for offset, length in ((0.25, 1.6), (1.25, 1.3))]
    fingerprints = [spectral_match(np, recorded, reference, packets, rate, offset, length)
                    for offset, length in ((0.25, 1.6), (1.25, 1.3))]
    checks = {"ordinary_window_checks": window_report["passed"], "finite_samples": finite,
              "no_callback_errors": not flags, "recording_samples_present": len(frames) > 0,
              "playback_recording_nonzero": control or float(np.sqrt(np.mean(frames ** 2))) > 1e-5}
    if control:
        checks["soundtrack_absent_in_next_control"] = all(m["absolute_correlation"] < 0.3 for m in matches)
        checks["spectral_fingerprints_absent_in_next_control"] = all(m["correlation"] < 0.6 for m in fingerprints)
    else:
        observations = json.loads((output / "frames.json").read_text(encoding="utf-8"))
        origin, movie = window_report["observation_origin_perf_counter"], window_report["movie_frames"]
        start = origin + observations[movie[0]]["time"] if movie else None
        end = origin + observations[movie[-1]]["time"] if movie else None
        checks["two_spectral_fingerprints_match_output"] = all(m["correlation"] >= 0.9 for m in fingerprints)
        checks["matches_during_visible_movie"] = (start is not None and all(
            m["match_start_perf_counter"] is not None
            and start - 0.6 <= m["match_start_perf_counter"] - m["reference_offset_seconds"] <= start + 0.6
            and m["match_start_perf_counter"] + m["reference_length_seconds"] <= end + 0.6
            for m in fingerprints))
    report = {"passed": all(checks.values()), "checks": checks, "window": window_report,
              "device": device["name"], "rate": rate, "channels": channels, "callback_flags": flags,
              "recorded_seconds": len(frames) / rate, "waveform_matches": matches,
              "strict_waveform_match": all(m["absolute_correlation"] >= 0.8 for m in matches),
              "spectral_fingerprints": fingerprints,
              "loopback_sha256": hashlib.sha256(raw).hexdigest()}
    (output / "audio-report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--control-fixture", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, required=True)
    parser.add_argument("--python-deps", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("requires Windows")
    if args.output.exists():
        parser.error("output directory must be new")
    sys.path.insert(0, str(args.python_deps.resolve()))
    import numpy as np
    import pyaudiowpatch as pa

    for fixture, movie in ((args.fixture, True), (args.control_fixture, False)):
        source = (fixture / "mission.ts").read_text(encoding="utf-8")
        if ("order.playMovie(m.movie(Movie.A01_P01e))" in source) != movie:
            parser.error("expected --team-media and --team-media-control fixtures")
        if (fixture / "SOV01UMD.MAP").read_bytes() != (fixture / "install/SOV01UMD.MAP").read_bytes():
            parser.error("installed map differs from authored fixture")
        settings = fixture / "settings/VERA.INI"
        if settings.exists() and "[Intro]\nPlay=no" not in settings.read_text(encoding="utf-8"):
            parser.error("task-local settings must disable the startup intro")
    bik = args.fixture / "A01_P01e.bik"
    reference_bytes = decode(args.ffmpeg, ["-i", str(bik.resolve()), "-map", "0:a:0"])
    reference = np.frombuffer(reference_bytes, dtype="<f4")
    if len(reference) < 2.8 * 8000 or not np.isfinite(reference).all() or float(np.sqrt(np.mean(reference ** 2))) < 0.01:
        parser.error("independent reference is too short, invalid or silent")
    args.output.mkdir(parents=True)
    (args.output / "reference.f32").write_bytes(reference_bytes)
    report = {"reference_bik_sha256": hashlib.sha256(bik.read_bytes()).hexdigest(),
              "reference_pcm_sha256": hashlib.sha256(reference_bytes).hexdigest(),
              "reference_seconds": len(reference) / 8000,
              "verifier_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "ffmpeg_version": subprocess.run([str(args.ffmpeg), "-version"], capture_output=True,
                                                text=True, check=True).stdout.splitlines()[0],
              "runs": []}
    for fixture, name, control in ((args.fixture, "movie", False), (args.control_fixture, "control", True)):
        report["runs"].append(capture(np, pa, args.client.resolve(), fixture.resolve(),
                                       args.output / name, control, args.ffmpeg, reference))
    report["passed"] = all(item["passed"] for item in report["runs"])
    report["scope"] = "OS WASAPI output time/frequency fingerprints matched to independent FFmpeg soundtrack decode, with visible movie timing and next() control; strict waveform correlation retained separately and may fail; no microphone, speaker/listener judgment, every-sample fidelity or original executable comparison"
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
