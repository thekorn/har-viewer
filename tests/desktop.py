"""Real GPUI smoke test. Run with devenv's har-smoke, never a browser mock.

Linux owns its virtual display; macOS requires explicit real-desktop opt-in.
Only the app launched by this script is targeted and terminated.
Clipboard assertions exercise public keyboard/pointer behavior and exported HAR data.
"""

import argparse
import json
import os
import shlex
import subprocess
import sys
import tempfile
import time
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--binary", default="target/debug/har-viewer")
parser.add_argument("--artifacts", type=Path)
parser.add_argument("--record", action="store_true")
parser.add_argument(
    "--use-desktop",
    action="store_true",
    help="Allow macOS automation on the real, otherwise idle desktop",
)
args = parser.parse_args()
macos = sys.platform == "darwin"
if macos and not args.use_desktop:
    parser.error("macOS requires --use-desktop; automation uses your real desktop")
if macos and args.record:
    parser.error(
        "--record is unsupported on macOS; use --artifacts for owned-window screenshots"
    )
root = Path(__file__).resolve().parent.parent
fixture = root / "fixtures/sample.har"
original = json.loads(fixture.read_text())["log"]["entries"]
artifacts = args.artifacts
if artifacts:
    artifacts.mkdir(parents=True, exist_ok=True)


def run(*command, **kwargs):
    return subprocess.check_output(command, text=True, timeout=15, **kwargs).strip()


desktop = None
if macos:
    from desktop_macos import MacDesktop

    desktop = MacDesktop(bool(artifacts), run)


with (
    tempfile.TemporaryDirectory(prefix="har-desktop-") as temp,
    open(Path(temp) / "desktop.log", "w+") as log,
):
    processes = []
    recorder = None
    window = None
    app = None
    try:
        if not desktop:
            read_fd, write_fd = os.pipe()
            try:
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
            finally:
                os.close(write_fd)
            import select

            try:
                assert select.select([read_fd], [], [], 15)[0], "Xvfb did not start"
                number = os.read(read_fd, 32).decode().strip()
                assert number.isdigit(), "Xvfb did not provide a display number"
            finally:
                os.close(read_fd)
            os.environ["DISPLAY"] = f":{number}"
            os.environ.pop("WAYLAND_DISPLAY", None)
            os.environ["XDG_RUNTIME_DIR"] = temp
        startup_capture = json.loads(fixture.read_text())
        startup_capture["log"]["comment"] = "Loaded from the --input file, not --sample"
        startup_file = Path(temp) / "startup capture ü.har"
        startup_file.write_text(json.dumps(startup_capture))

        def launch(*arguments):
            app = subprocess.Popen(
                [str(Path(args.binary).resolve()), *arguments],
                stdout=log,
                stderr=log,
            )
            processes.append(app)
            window = None
            if desktop:
                desktop.start(app)
                window = str(app.pid)
            else:
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
            return app, window

        app, window = launch("--input", str(startup_file))

        def key(keys):
            if desktop:
                return desktop.key(keys)
            run("xdotool", "key", "--clearmodifiers", keys)
            time.sleep(0.25)

        def type_text(text):
            if desktop:
                return desktop.type_text(text)
            run("xdotool", "type", "--clearmodifiers", "--delay", "35", text)

        def resize(width, height):
            if desktop:
                return desktop.resize(width, height)
            run("xdotool", "windowsize", window, str(width), str(height))

        pointer = [700, 400]

        def click(x, y):
            if desktop:
                return desktop.click(x, y)
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
            if desktop:
                return desktop.clipboard(expected)
            # X11 selection transfer is asynchronous; wait for the requested value,
            # without sending the copy action again or accepting stale clipboard data.
            deadline = time.monotonic() + 5
            while True:
                result = subprocess.run(
                    ["xclip", "-selection", "clipboard", "-o"],
                    text=True,
                    capture_output=True,
                    timeout=15,
                    check=False,
                )
                if result.returncode:
                    if time.monotonic() >= deadline:
                        result.check_returncode()
                    time.sleep(0.1)
                    continue
                value = result.stdout.strip()
                if (
                    expected is None
                    or value == expected
                    or time.monotonic() >= deadline
                ):
                    return value
                time.sleep(0.1)

        def exported():
            key("ctrl+shift+e")
            return json.loads(clipboard())["log"]["entries"]

        def search(text):
            key("ctrl+f")
            key("ctrl+a")
            if text:
                type_text(text)
            else:
                key("BackSpace")
            key("Escape")

        def screenshot(name):
            if artifacts:
                time.sleep(0.3)
                if desktop:
                    return desktop.screenshot(artifacts / f"{name}.png")
                run(
                    "magick",
                    "import",
                    "-window",
                    window,
                    str(artifacts / f"{name}.png"),
                )

        key("ctrl+shift+e")
        assert json.loads(clipboard()) == startup_capture, (
            "--input must load the specified file and preserve all original data"
        )
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

        click(350, 348)  # Select a row through the component table.
        key("Down")
        key("ctrl+shift+c")
        assert clipboard() == original[2]["request"]["url"], "Table keyboard selection"
        key("Escape")
        key("ctrl+f")
        key("Down")
        key("ctrl+shift+c")
        assert clipboard() == original[2]["request"]["url"], (
            "Input arrows must not navigate requests"
        )
        key("Return")

        # Request search must remain reachable even while the library editor is focused.
        click(1100, 350)
        search("503")
        expected = [e for e in original if e["response"]["status"] == 503]
        assert exported() == expected, "Native search must affect export"
        screenshot("search")
        search("no-such-request-98765")
        assert exported() == [], "No results must not export hidden rows"
        screenshot("empty-filter")
        search("")

        # Independent facets combine AND (within-facet OR is covered in har.rs).
        click(90, 230)
        click(90, 494)
        expected = [
            e
            for e in original
            if e["response"]["status"] // 100 == 2 and e["request"]["method"] == "POST"
        ]
        assert exported() == expected, "Status and method facets"
        screenshot("filtered")
        click(183, 116)  # Reset
        assert exported() == original

        click(20, 848)  # Slow-request checkbox remains independent of the facets.
        assert exported() == [e for e in original if e["time"] >= 1000], "Slow filter"
        click(20, 848)
        assert exported() == original

        # Time sorting, then selection and response inspection.
        key("ctrl+shift+c")
        selected_url = clipboard()
        click(880, 267)  # Component sort icon: descending first.
        assert exported() == sorted(original, key=lambda e: e["time"], reverse=True), (
            "Descending time sort"
        )
        key("ctrl+shift+c")
        assert clipboard() == selected_url, "Sorting retains entry identity"
        click(880, 267)
        assert exported() == sorted(original, key=lambda e: e["time"]), (
            "Ascending time sort"
        )
        click(880, 267)
        assert exported() == original, "Third sort restores capture-time order"
        search("/v1/catalog?")
        click(1163, 218)  # Response
        click(1237, 252)  # Copy content
        expected_body = next(
            e["response"]["content"]["text"]
            for e in original
            if "/v1/catalog?" in e["request"]["url"]
        )
        assert json.loads(clipboard()) == json.loads(expected_body), (
            "Response copies actual body"
        )
        screenshot("response")
        click(1382, 218)  # Raw entry, not the preview/editor state.
        click(1237, 252)
        catalog = next(e for e in original if "/v1/catalog?" in e["request"]["url"])
        assert json.loads(clipboard()) == catalog, "Raw copy preserves entry metadata"
        screenshot("raw")
        search("/v1/cart/coupon")
        click(1080, 218)  # Payload
        click(1237, 252)
        coupon = next(e for e in original if "/v1/cart/coupon" in e["request"]["url"])
        assert json.loads(clipboard()) == json.loads(
            coupon["request"]["postData"]["text"]
        )
        screenshot("payload")
        click(1115, 252)  # Copy cURL without executing the captured request.
        curl = shlex.split(clipboard())
        assert curl[0] == "curl" and coupon["request"]["url"] in curl
        assert curl[curl.index("--request") + 1] == "POST"
        assert coupon["request"]["postData"]["text"] in curl, "cURL retains payload"
        search("https://shop.lantern.test/")
        click(350, 307)  # First matching request owns the fictional response cookie.
        click(1244, 218)
        click(1237, 252)
        assert '"demo_session"' in clipboard() and '"fictional"' in clipboard(), (
            "Cookies copy"
        )
        screenshot("cookies")
        click(1320, 218)
        click(1237, 252)
        assert "wait:" in clipboard(), "Timing content copy"
        screenshot("timing")
        search("")
        click(135, 76)
        # Capture-time ascending and unsorted are equivalent; the timeline must
        # still be able to cycle back to descending after restoring that order.
        for descending in (True, False, True, False):
            click(1190, 297)
            assert exported() == sorted(
                original, key=lambda e: e["startedDateTime"], reverse=descending
            ), "Waterfall timeline sort must keep cycling"
        screenshot("waterfall")
        click(225, 76)
        screenshot("overview")
        click(700, 655)  # Overview's component button, not the keyboard shortcut.
        assert json.loads(clipboard())["log"]["entries"] == original, (
            "Overview HAR copy"
        )
        click(305, 76)
        screenshot("issues")
        click(45, 76)
        if recorder:
            recorder.communicate(b"q", timeout=15)
            assert recorder.returncode == 0, "Recording failed"

        # File I/O via the real native path input, including failure recovery.
        key("ctrl+o")
        time.sleep(0.8)
        type_text(str(root / "Cargo.toml"))
        key("ctrl+a")
        key("ctrl+c")
        copied_path = clipboard(str(root / "Cargo.toml"))
        assert copied_path == str(root / "Cargo.toml"), "Path field accepts native text"
        key("Return")
        time.sleep(0.5)
        screenshot("invalid-file")
        key("Escape")
        assert len(exported()) == 35, "Bad file must not discard previous capture"
        key("ctrl+o")
        time.sleep(0.8)
        key("ctrl+a")
        type_text(str(fixture))
        key("ctrl+a")
        key("ctrl+c")
        assert clipboard(str(fixture)) == str(fixture), "Path replacement"
        key("Return")
        time.sleep(0.8)
        key("Escape")
        assert exported() == original, (
            "Open path replaces capture and resets filters/sort"
        )
        click(1416, 116)  # Close inspector.
        screenshot("full-table")
        click(350, 307)  # Reopen the same selected entry, not only a different row.
        exported()  # Replace clipboard with HAR so a missing button cannot pass.
        click(1020, 252)
        assert clipboard() == original[0]["request"]["url"], "Row reopens inspector"
        resize(1100, 650 if desktop else 700)
        screenshot("compact")
        resize(1440, 900)
        key("ctrl+q")
        assert app.wait(timeout=10) == 0

        app, window = launch()
        screenshot("welcome")
        click(710, 576)  # Explore the bundled sample from the empty welcome screen.
        assert exported() == original, "Welcome sample button"
        key("ctrl+q")
        assert app.wait(timeout=10) == 0
        print(
            "PASS: native --input launch, clipboard, keyboard/input focus, search, empty state, facets, slow filter, sort cycle, inspector tabs/copies, file loading, error recovery, resize, welcome sample, quit"
        )
    except BaseException:
        if artifacts and window and app and app.poll() is None:
            try:
                screenshot("failure")
            except (
                OSError,
                RuntimeError,
                subprocess.SubprocessError,
                AssertionError,
                ValueError,
            ) as error:
                print(f"Failure screenshot unavailable: {type(error).__name__}")
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
        if desktop:
            desktop.close()
