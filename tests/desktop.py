"""Real GPUI/X11 smoke test. Run with devenv's har-smoke, never a browser mock.

Owns and terminates its virtual display and app; never touches an existing desktop.
Clipboard assertions exercise public keyboard/pointer behavior and exported HAR data.
"""

import argparse
import json
import os
import subprocess
import tempfile
import time
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--binary", default="target/debug/har-viewer")
parser.add_argument("--artifacts", type=Path)
parser.add_argument("--record", action="store_true")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
fixture = root / "fixtures/sample.har"
original = json.loads(fixture.read_text())["log"]["entries"]
artifacts = args.artifacts
if artifacts:
    artifacts.mkdir(parents=True, exist_ok=True)


def run(*command, **kwargs):
    return subprocess.check_output(command, text=True, timeout=15, **kwargs).strip()


with (
    tempfile.TemporaryDirectory(prefix="har-desktop-") as temp,
    open(Path(temp) / "desktop.log", "w+") as log,
):
    processes = []
    recorder = None
    window = None
    app = None
    try:
        read_fd, write_fd = os.pipe()
        display = subprocess.Popen(
            [
                "Xvfb",
                "-displayfd",
                str(write_fd),
                "-screen",
                "0",
                "1440x900x24",
                "-nolisten",
                "tcp",
            ],
            pass_fds=(write_fd,),
            stdout=log,
            stderr=log,
        )
        processes.append(display)
        os.close(write_fd)
        import select

        assert select.select([read_fd], [], [], 15)[0], "Xvfb did not start"
        number = os.read(read_fd, 32).decode().strip()
        os.close(read_fd)
        os.environ["DISPLAY"] = f":{number}"
        os.environ.pop("WAYLAND_DISPLAY", None)
        os.environ["XDG_RUNTIME_DIR"] = temp
        app = subprocess.Popen(
            [str(Path(args.binary).resolve()), "--sample"], stdout=log, stderr=log
        )
        processes.append(app)
        window = None
        for _ in range(100):
            result = subprocess.run(
                ["xdotool", "search", "--name", "^HAR Viewer$"],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode == 0:
                window = result.stdout.splitlines()[0]
                break
            assert app.poll() is None, "App exited before opening a window"
            time.sleep(0.2)
        assert window, "No native window appeared"
        run(
            "xdotool",
            "windowmap",
            window,
            "windowmove",
            window,
            "0",
            "0",
            "windowsize",
            window,
            "1440",
            "900",
            "windowfocus",
            window,
        )
        time.sleep(1)

        def key(keys):
            run("xdotool", "key", "--clearmodifiers", keys)
            time.sleep(0.25)

        pointer = [700, 400]

        def click(x, y):
            # Genuine pointer motion passes over neighbors; useful in recordings too.
            start = pointer[:]
            for step in range(1, 9):
                run(
                    "xdotool",
                    "mousemove",
                    str(round(start[0] + (x - start[0]) * step / 8)),
                    str(round(start[1] + (y - start[1]) * step / 8)),
                )
                time.sleep(0.015)
            pointer[:] = [x, y]
            run("xdotool", "click", "1")
            time.sleep(0.3)

        def clipboard(expected=None):
            # X11 selection transfer is asynchronous; wait for the requested value,
            # without sending the copy action again or accepting stale clipboard data.
            deadline = time.monotonic() + 5
            while True:
                value = run("xclip", "-selection", "clipboard", "-o")
                if expected is None or value == expected or time.monotonic() >= deadline:
                    return value
                time.sleep(0.1)

        def exported():
            key("ctrl+shift+e")
            return json.loads(clipboard())["log"]["entries"]

        def search(text):
            key("ctrl+f")
            key("ctrl+a")
            if text:
                run("xdotool", "type", "--clearmodifiers", "--delay", "35", text)
            else:
                key("BackSpace")
            key("Escape")

        def screenshot(name):
            if artifacts:
                time.sleep(0.3)
                run(
                    "magick",
                    "import",
                    "-window",
                    window,
                    str(artifacts / f"{name}.png"),
                )

        assert exported() == original, "Initial export must preserve all original data"
        screenshot("requests")
        if artifacts and args.record:
            recorder = subprocess.Popen(
                [
                    "ffmpeg",
                    "-y",
                    "-f",
                    "x11grab",
                    "-framerate",
                    "15",
                    "-video_size",
                    "1440x900",
                    "-i",
                    os.environ["DISPLAY"],
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-crf",
                    "23",
                    "-pix_fmt",
                    "yuv420p",
                    str(artifacts / "interaction.mp4"),
                ],
                stdin=subprocess.PIPE,
                stdout=log,
                stderr=log,
            )
            processes.append(recorder)
        key("ctrl+shift+c")
        assert clipboard() == "https://shop.lantern.test/"
        key("Down")
        key("ctrl+shift+c")
        assert clipboard() == "https://cdn.lantern.test/assets/shop.css", (
            "Arrow navigation"
        )
        key("Up")

        search("503")
        expected = [e for e in original if e["response"]["status"] == 503]
        assert exported() == expected, "Native search must affect export"
        screenshot("search")
        search("no-such-request-98765")
        assert exported() == [], "No results must not export hidden rows"
        screenshot("empty-filter")
        search("")

        # Independent facets combine AND (within-facet OR is covered in har.rs).
        click(90, 232)
        click(90, 513)
        expected = [
            e
            for e in original
            if e["response"]["status"] // 100 == 2 and e["request"]["method"] == "POST"
        ]
        assert exported() == expected, "Status and method facets"
        screenshot("filtered")
        click(165, 123)  # Reset
        assert exported() == original

        # Time sorting, then selection and response inspection.
        click(940, 245)
        click(940, 245)
        assert exported() == sorted(original, key=lambda e: e["time"], reverse=True), (
            "Descending time sort"
        )
        search("/v1/catalog?")
        click(1163, 240)  # Response
        click(1237, 283)  # Copy content
        expected_body = next(
            e["response"]["content"]["text"]
            for e in original
            if "/v1/catalog?" in e["request"]["url"]
        )
        assert json.loads(clipboard()) == json.loads(expected_body), (
            "Response copies actual body"
        )
        screenshot("response")
        click(1285, 240)
        screenshot("timing")
        search("")
        click(152, 78)
        screenshot("waterfall")
        click(250, 78)
        screenshot("overview")
        click(325, 78)
        screenshot("issues")
        click(60, 78)
        if recorder:
            recorder.communicate(b"q", timeout=15)
            assert recorder.returncode == 0, "Recording failed"

        # File I/O via the real native path input, including failure recovery.
        key("ctrl+o")
        time.sleep(0.8)
        run(
            "xdotool",
            "type",
            "--clearmodifiers",
            "--delay",
            "35",
            str(root / "Cargo.toml"),
        )
        key("ctrl+a")
        key("ctrl+c")
        copied_path = clipboard(str(root / "Cargo.toml"))
        assert copied_path == str(root / "Cargo.toml"), (
            f"Path field accepts native text; got {copied_path!r}"
        )
        key("Return")
        time.sleep(0.5)
        screenshot("invalid-file")
        key("Escape")
        assert len(exported()) == 35, "Bad file must not discard previous capture"
        key("ctrl+o")
        time.sleep(0.8)
        key("ctrl+a")
        run("xdotool", "type", "--clearmodifiers", "--delay", "35", str(fixture))
        key("ctrl+a")
        key("ctrl+c")
        assert clipboard(str(fixture)) == str(fixture), "Path replacement"
        key("Return")
        time.sleep(0.8)
        key("Escape")
        assert exported() == original, (
            "Open path replaces capture and resets filters/sort"
        )
        run("xdotool", "windowsize", window, "1100", "700")
        screenshot("compact")
        run("xdotool", "windowsize", window, "1440", "900")
        key("ctrl+q")
        assert app.wait(timeout=10) == 0
        print(
            "PASS: native X11 launch, clipboard, keyboard navigation, search, empty state, combined facets, sorting, response copy, file loading, error recovery, resize, quit"
        )
    except BaseException:
        if artifacts and window and app.poll() is None:
            screenshot("failure")
        log.flush()
        log.seek(0)
        print(log.read())
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
