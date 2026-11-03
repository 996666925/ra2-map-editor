"""Observe the team-media fixture's natural movie return in a normal Windows client.

Requires a visible, unobstructed desktop. Uses ordinary campaign-menu mouse input;
never sends Escape or uses engine capture adapters. Captures only the owned
foreground client's rectangle. The fixed 1000x736 viewport makes the fixture's
sidebar, message and subtitle masks comparable. No native-game parity is claimed.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import time

from verify_mission_visibility import write_png


class Window:
    def __init__(self, pid):
        self.u = u = c.WinDLL("user32", use_last_error=True)
        self.g = g = c.WinDLL("gdi32", use_last_error=True)
        u.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
        u.SetProcessDpiAwarenessContext(c.c_void_p(-4))
        u.GetForegroundWindow.restype = w.HWND
        u.GetDC.argtypes, u.GetDC.restype = [w.HWND], w.HDC
        u.ReleaseDC.argtypes = [w.HWND, w.HDC]
        u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
        u.GetWindowThreadProcessId.restype = w.DWORD
        for name in ("BringWindowToTop", "SetForegroundWindow", "IsWindowVisible"):
            getattr(u, name).argtypes = [w.HWND]
        u.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
        u.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
        u.MoveWindow.argtypes = [w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.BOOL]
        u.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
        u.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
        g.CreateCompatibleDC.argtypes, g.CreateCompatibleDC.restype = [w.HDC], w.HDC
        g.CreateCompatibleBitmap.argtypes = [w.HDC, c.c_int, c.c_int]
        g.CreateCompatibleBitmap.restype = w.HBITMAP
        g.SelectObject.argtypes, g.SelectObject.restype = [w.HDC, w.HGDIOBJ], w.HGDIOBJ
        g.BitBlt.argtypes = [w.HDC, c.c_int, c.c_int, c.c_int, c.c_int,
                            w.HDC, c.c_int, c.c_int, w.DWORD]
        g.GetDIBits.argtypes = [w.HDC, w.HBITMAP, w.UINT, w.UINT,
                               c.c_void_p, c.c_void_p, w.UINT]
        g.DeleteObject.argtypes, g.DeleteDC.argtypes = [w.HGDIOBJ], [w.HDC]
        found = []

        @c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        def visit(handle, _):
            owner, title = w.DWORD(), c.create_unicode_buffer(256)
            u.GetWindowThreadProcessId(handle, c.byref(owner))
            u.GetWindowTextW(handle, title, len(title))
            if owner.value == pid and u.IsWindowVisible(handle) and title.value == "VERA20K":
                found.append(handle)
            return True

        u.EnumWindows(visit, 0)
        if len(found) != 1:
            raise RuntimeError(f"expected one owned VERA20K window, got {found}")
        self.handle = found[0]
        foreground = u.GetForegroundWindow()
        source = u.GetWindowThreadProcessId(foreground, None)
        target = u.GetWindowThreadProcessId(self.handle, None)
        attached = source != target and u.AttachThreadInput(source, target, True)
        try:
            u.BringWindowToTop(self.handle)
            u.SetForegroundWindow(self.handle)
        finally:
            if attached:
                u.AttachThreadInput(source, target, False)
        # Correct for the actual DPI-dependent frame, rather than assuming its size.
        u.MoveWindow(self.handle, 40, 40, 1024, 800, True)
        rect = w.RECT()
        u.GetClientRect(self.handle, c.byref(rect))
        u.MoveWindow(self.handle, 40, 40, 2024 - rect.right, 1536 - rect.bottom, True)
        time.sleep(0.5)

    def check(self):
        if self.u.GetForegroundWindow() != self.handle:
            raise RuntimeError("owned client lost foreground; capture stopped")
        rect = w.RECT()
        self.u.GetClientRect(self.handle, c.byref(rect))
        if (rect.right, rect.bottom) != (1000, 736):
            raise RuntimeError("client viewport changed")

    def click(self, x, y):
        self.check()
        point = w.POINT(x, y)
        self.u.ClientToScreen(self.handle, c.byref(point))
        if not self.u.SetCursorPos(point.x, point.y):
            raise c.WinError(c.get_last_error())
        time.sleep(0.3)
        self.u.mouse_event(2, 0, 0, 0, 0)
        time.sleep(0.15)
        self.u.mouse_event(4, 0, 0, 0, 0)

    def capture(self, path):
        self.check()
        point = w.POINT()
        self.u.ClientToScreen(self.handle, c.byref(point))
        u, g = self.u, self.g
        # GetDC(hwnd) can retain the first Vulkan frame. Read the displayed client
        # rectangle from the screen DC; never save other desktop regions.
        dc = u.GetDC(None)
        memory = g.CreateCompatibleDC(dc)
        bitmap = g.CreateCompatibleBitmap(dc, 1000, 736)
        old = g.SelectObject(memory, bitmap)
        try:
            if not g.BitBlt(memory, 0, 0, 1000, 736, dc, point.x, point.y, 0x00CC0020):
                raise c.WinError(c.get_last_error())
            g.SelectObject(memory, old)
            info = c.create_string_buffer(40)
            struct.pack_into("<IiiHHIIiiII", info, 0, 40, 1000, -736, 1, 32, 0, 0, 0, 0, 0, 0)
            buffer = c.create_string_buffer(1000 * 736 * 4)
            if g.GetDIBits(dc, bitmap, 0, 736, buffer, info, 0) != 736:
                raise c.WinError(c.get_last_error())
            self.check()
            raw = buffer.raw
            write_png(path, raw, 1000, 736, 4000)
            return raw
        finally:
            g.SelectObject(memory, old)
            g.DeleteObject(bitmap)
            g.DeleteDC(memory)
            u.ReleaseDC(None, dc)


def facts(raw):
    def pixel(x, y):
        offset = (y * 1000 + x) * 4
        return raw[offset:offset + 3]

    def crop(x0, y0, x1, y1):
        return b"".join(raw[(y * 1000 + x0) * 4:(y * 1000 + x1) * 4]
                        for y in range(y0, y1))

    # Fixture-specific observations, with complete PNGs retained for inspection.
    sidebar = pixel(900, 60)
    game = min(sidebar) > 70 and max(pixel(400, 100)) == 0
    movie = not game and sum(pixel(500, 300)) > 150
    mask = bytes(min(pixel(x, y)) >= 250 for y in range(640, 715) for x in range(750, 815))
    message = sum(pixel(x, y)[0] > 150 and pixel(x, y)[1] > 90
                  and pixel(x, y)[2] < 80 for y in range(40, 53) for x in range(150))
    return {"game": game, "movie_candidate": movie,
            "subtitle_white_pixels": sum(mask),
            "subtitle_mask_sha256": hashlib.sha256(mask).hexdigest(),
            "movie_pixels_sha256": hashlib.sha256(crop(100, 100, 750, 600)).hexdigest(),
            "third_message_cyan_pixels": message}


def run(client, fixture, output, control):
    output.mkdir(parents=True)
    settings = fixture / "settings" / "VERA.INI"
    original = settings.read_bytes() if settings.exists() else None
    # Only the task-local settings file is changed; restore it after clean exit.
    text = original.decode("utf-8") if original is not None else "[Intro]\nPlay=no\n[VERA]\nFullScreen=no\nSkipStartupLogo=yes\nLockCursor=no\n"
    text = text.replace("SkipStartupLogo=no", "SkipStartupLogo=yes")
    text = text.replace("LockCursor=yes", "LockCursor=no")
    settings.parent.mkdir(exist_ok=True)
    settings.write_text(text, encoding="utf-8")
    child, window = None, None
    samples = []
    try:
        with (output / "client.log").open("wb") as log:
            child = subprocess.Popen([str(client), "-WIN"], cwd=fixture, stdout=log, stderr=log)
            time.sleep(8)
            window = Window(child.pid)
            window.capture(output / "menu.png")
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / "single-player.png")
            window.click(890, 268)
            time.sleep(2)
            window.capture(output / "campaign.png")
            window.click(395, 535)
            started = time.perf_counter()
            # No further pointer/keyboard injection until the run ends.
            while time.perf_counter() - started < 22:
                elapsed = time.perf_counter() - started
                name = f"frame-{len(samples):03d}.png"
                raw = window.capture(output / name)
                samples.append({"time": elapsed, "path": name, **facts(raw)})
                time.sleep(0.1)
    finally:
        if window and child and child.poll() is None:
            window.u.PostMessageW(window.handle, 0x10, 0, 0)
        if child:
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=10)
        if original is None:
            settings.unlink(missing_ok=True)
        else:
            settings.write_bytes(original)
        (output / "frames.json").write_text(json.dumps(samples, indent=2), encoding="utf-8")

    first_game = next((i for i, frame in enumerate(samples) if frame["game"]), len(samples))
    movie = [i for i in range(first_game + 1, len(samples))
             if samples[i]["movie_candidate"] and not samples[i]["game"]]
    stable = []
    for i in movie:
        frame = samples[i]
        if frame["subtitle_white_pixels"] > 1000:
            key = frame["subtitle_mask_sha256"]
            if not stable or stable[-1]["sha256"] != key:
                stable.append({"sha256": key, "frame": i, "path": frame["path"],
                               "time": frame["time"], "pixels": frame["subtitle_white_pixels"]})
    duration = samples[movie[-1] + 1]["time"] - samples[movie[0]]["time"] if movie and movie[-1] + 1 < len(samples) else None
    sampling_gap = max(b["time"] - a["time"] for a, b in zip(samples, samples[1:]))
    checks = {"clean_exit": child.returncode == 0,
              "game_seen": first_game < len(samples),
              "sampling_gap_under_half_second": sampling_gap < 0.5,
              "game_and_third_message_afterwards": any(f["game"] and f["third_message_cyan_pixels"] > 50 for f in samples[(movie[-1] + 1 if movie else first_game):]),
              "movie_presence_matches_control": bool(movie) != control}
    if not control:
        checks.update({"three_second_visible_interval": duration is not None and 2.7 <= duration <= 3.4,
                       "four_ordered_subtitle_masks": len(stable) == 4,
                       "moving_movie_pixels": len({samples[i]["movie_pixels_sha256"] for i in movie}) >= 8})
    return {"passed": all(checks.values()), "checks": checks, "control": control,
            "verifier_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "client_sha256": hashlib.sha256(client.read_bytes()).hexdigest(),
            "inputs": {name: hashlib.sha256((fixture / name).read_bytes()).hexdigest()
                       for name in ("mission.ts", "sdk.ts", "SOV01UMD.MAP", "subtitlemd.txt")},
            "first_game_frame": first_game, "movie_frames": movie,
            "observation_origin_perf_counter": started,
            "visible_interval_seconds": duration, "subtitle_masks": stable,
            "max_sampling_gap_seconds": sampling_gap,
            "scope": "normal campaign GUI; no Escape; displayed pixels and subsequent message; subtitle mask labels require visual review; no movie audio or native pixel parity"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--control-fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "win32":
        parser.error("requires Windows")
    if args.output.exists():
        parser.error("output directory must be new")
    for fixture, movie in ((args.fixture, True), (args.control_fixture, False)):
        source = (fixture / "mission.ts").read_text(encoding="utf-8")
        if ("order.playMovie(m.movie(Movie.A01_P01e))" in source) != movie:
            parser.error("expected --team-media and --team-media-control fixtures")
        settings = fixture / "settings" / "VERA.INI"
        if settings.exists() and "[Intro]\nPlay=no" not in settings.read_text(encoding="utf-8"):
            parser.error("task-local settings must disable the startup intro")
        if (fixture / "SOV01UMD.MAP").read_bytes() != (fixture / "install" / "SOV01UMD.MAP").read_bytes():
            parser.error("installed map differs from the authored fixture")
    report = {"runs": [run(args.client.resolve(), fixture.resolve(), args.output / name, control)
                       for fixture, name, control in ((args.fixture, "movie", False),
                                                       (args.control_fixture, "control", True))]}
    report["passed"] = all(item["passed"] for item in report["runs"])
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
