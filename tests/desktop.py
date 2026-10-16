"""Real GPUI smoke test. Run with devenv's har-smoke, never a browser mock.

Linux owns its virtual display; macOS requires explicit real-desktop opt-in.
Only the app launched by this script is targeted and terminated.
Clipboard assertions exercise public keyboard/pointer behavior and request inspection.
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
        startup_capture["log"]["entries"][0]["request"]["url"] = (
            "https://shop.lantern.test/?source=cli"
        )
        original = startup_capture["log"]["entries"]
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

        def key(keys, repeat=1):
            if desktop:
                return desktop.key(keys, repeat)
            run(
                "xdotool",
                "key",
                "--clearmodifiers",
                "--repeat",
                str(repeat),
                "--delay",
                "35",
                keys,
            )
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

        def selected_url():
            key("ctrl+shift+c")
            return clipboard()

        def unchanged_clipboard(keys):
            # Seed a sentinel so a removed/no-selection copy action cannot pass
            # by returning stale request data. Read directly: the macOS adapter
            # normally waits for a clipboard change, which must not happen here.
            sentinel = "HAR smoke: no copy expected"
            if desktop:
                desktop.prepare_copy()
                sentinel = run("/usr/bin/pbpaste")
                key(keys)
                assert run("/usr/bin/pbpaste") == sentinel
            else:
                subprocess.run(
                    ["xclip", "-selection", "clipboard"],
                    input=sentinel,
                    text=True,
                    check=True,
                    timeout=15,
                )
                key(keys)
                assert clipboard() == sentinel

        def assert_visible(expected):
            # Inspect the visible ordering through navigation and Copy URL,
            # rather than retaining an export backdoor just for the test.
            key("Escape")
            if not expected:
                unchanged_clipboard("ctrl+shift+c")
                return
            key("Up", repeat=len(original))
            for index, entry in enumerate(expected):
                if index:
                    key("Down")
                assert selected_url() == entry["request"]["url"], (
                    f"Visible request {index} differs"
                )
            key("Down")
            assert selected_url() == expected[-1]["request"]["url"], (
                "Navigation must stop at the last visible request"
            )
            key("Up", repeat=len(expected))

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

        assert selected_url() == original[0]["request"]["url"], (
            "--input must load the specified file, not a bundled sample"
        )
        unchanged_clipboard("ctrl+shift+e")
        key("ctrl+o")  # Removed shortcut must not steal focus into a path field.
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
        assert clipboard() == original[0]["request"]["url"]
        key("Down")
        key("ctrl+shift+c")
        assert clipboard() == "https://cdn.lantern.test/assets/shop.css", (
            "Arrow navigation"
        )
        key("Up")

        click(350, 288)  # Select a row through the component table.
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
        click(1100, 290)
        search("503")
        expected = [e for e in original if e["response"]["status"] == 503]
        assert_visible(expected)
        screenshot("search")
        search("no-such-request-98765")
        assert_visible([])
        screenshot("empty-filter")
        search("")

        # Independent facets combine AND (within-facet OR is covered in har.rs).
        click(90, 170)
        click(90, 434)
        expected = [
            e
            for e in original
            if e["response"]["status"] // 100 == 2 and e["request"]["method"] == "POST"
        ]
        assert_visible(expected)
        screenshot("filtered")

        # Checked facets move to the first row of their own group, and the
        # moved row still toggles the same value rather than its old neighbor.
        click(90, 134)  # Uncheck 2xx at the top of Status.
        assert_visible([e for e in original if e["request"]["method"] == "POST"])
        click(90, 326)  # Uncheck POST at the top of Method.
        assert_visible(original)

        click(90, 278)  # Check 5xx from the restored Status order.
        click(90, 206)  # Check 2xx below the promoted 5xx and unchecked Failed.
        assert_visible(
            [e for e in original if e["response"]["status"] // 100 in (2, 5)]
        )
        screenshot("checked-facets")
        click(90, 134)  # Checked values retain their original order: 2xx then 5xx.
        assert_visible([e for e in original if e["response"]["status"] // 100 == 5])
        click(183, 56)  # Reset
        assert_visible(original)

        click(20, 848)  # Slow-request checkbox remains independent of the facets.
        assert_visible([e for e in original if e["time"] >= 1000])
        click(20, 848)
        assert_visible(original)

        # Time sorting, then selection and response inspection.
        key("ctrl+shift+c")
        before_sort = clipboard()
        click(880, 207)  # Component sort icon: descending first.
        key("ctrl+shift+c")
        assert clipboard() == before_sort, "Sorting retains entry identity"
        assert_visible(sorted(original, key=lambda e: e["time"], reverse=True))
        click(880, 207)
        assert_visible(sorted(original, key=lambda e: e["time"]))
        click(880, 207)
        assert_visible(original)
        search("/v1/catalog?")
        click(1163, 158)  # Response
        click(1237, 192)  # Copy content
        expected_body = next(
            e["response"]["content"]["text"]
            for e in original
            if "/v1/catalog?" in e["request"]["url"]
        )
        assert json.loads(clipboard()) == json.loads(expected_body), (
            "Response copies actual body"
        )
        screenshot("response")
        click(1382, 158)  # Raw entry, not the preview/editor state.
        click(1237, 192)
        catalog = next(e for e in original if "/v1/catalog?" in e["request"]["url"])
        assert json.loads(clipboard()) == catalog, "Raw copy preserves entry metadata"
        screenshot("raw")
        search("/v1/cart/coupon")
        click(1080, 158)  # Payload
        click(1237, 192)
        coupon = next(e for e in original if "/v1/cart/coupon" in e["request"]["url"])
        assert json.loads(clipboard()) == json.loads(
            coupon["request"]["postData"]["text"]
        )
        screenshot("payload")
        click(1115, 192)  # Copy cURL without executing the captured request.
        curl = shlex.split(clipboard())
        assert curl[0] == "curl" and coupon["request"]["url"] in curl
        assert curl[curl.index("--request") + 1] == "POST"
        assert coupon["request"]["postData"]["text"] in curl, "cURL retains payload"
        search("https://shop.lantern.test/")
        click(350, 247)  # First matching request owns the fictional response cookie.
        click(1244, 158)
        click(1237, 192)
        assert '"demo_session"' in clipboard() and '"fictional"' in clipboard(), (
            "Cookies copy"
        )
        screenshot("cookies")
        click(1320, 158)
        click(1237, 192)
        assert "wait:" in clipboard(), "Timing content copy"
        screenshot("timing")
        search("")
        click(135, 16)
        # Capture-time ascending and unsorted are equivalent; the timeline must
        # still be able to cycle back to descending after restoring that order.
        for descending in (True, False, True, False):
            click(1190, 237)
            assert_visible(
                sorted(original, key=lambda e: e["startedDateTime"], reverse=descending)
            )
        screenshot("waterfall")
        click(225, 16)
        screenshot("overview")
        unchanged_clipboard("ctrl+shift+e")
        click(305, 16)
        screenshot("issues")
        click(45, 16)
        if recorder:
            recorder.communicate(b"q", timeout=15)
            assert recorder.returncode == 0, "Recording failed"

        click(1416, 56)  # Close inspector.
        screenshot("full-table")
        click(350, 247)  # Reopen the same selected entry, not only a different row.
        unchanged_clipboard("ctrl+shift+e")
        click(1020, 192)
        assert clipboard() == original[0]["request"]["url"], "Row reopens inspector"
        resize(1100, 650 if desktop else 700)
        screenshot("compact")
        resize(1440, 900)
        key("ctrl+q")
        assert app.wait(timeout=10) == 0

        # A FIFO holds startup in the real asynchronous read without a test-only
        # delay in the app. Closing the writer transitions to the loaded screen.
        pending = Path(temp) / "loading.har"
        os.mkfifo(pending)
        app, window = launch("--input", str(pending))
        screenshot("loading")
        with pending.open("w") as writer:
            writer.write(startup_file.read_text())
        time.sleep(1)
        assert selected_url() == original[0]["request"]["url"]
        key("ctrl+q")
        assert app.wait(timeout=10) == 0

        for name, path in [
            ("invalid-file", root / "Cargo.toml"),
            ("missing-file", Path(temp) / "missing.har"),
        ]:
            app, window = launch("--input", str(path))
            screenshot(name)
            key("Escape")  # Must not dismiss the error into a loading screen.
            screenshot(f"{name}-after-escape")
            key("ctrl+q")
            assert app.wait(timeout=10) == 0
        print(
            "PASS: native --input launch, removed open/export shortcuts, clipboard, keyboard/input focus, search, empty state, facets, slow filter, sort cycle, inspector tabs/copies, loading transition, resize, quit"
        )
        if artifacts:
            print("Startup error screenshots require visual review.")
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
