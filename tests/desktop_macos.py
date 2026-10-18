"""Owned-process macOS input and capture adapter (no privacy settings changes)."""

import ctypes as C
import json
import os
import platform
import pwd
import subprocess
import time


class Point(C.Structure):
    _fields_ = [("x", C.c_double), ("y", C.c_double)]


class Size(C.Structure):
    _fields_ = [("width", C.c_double), ("height", C.c_double)]


class Rect(C.Structure):
    _fields_ = [("origin", Point), ("size", Size)]


class MacDesktop:
    def __init__(self, screenshots, run):
        self.run = run
        self.app = None
        self.cg = C.CDLL(
            "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics"
        )
        self.ax = C.CDLL(
            "/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices"
        )
        self.objc = C.CDLL("/usr/lib/libobjc.A.dylib")
        C.CDLL("/System/Library/Frameworks/AppKit.framework/AppKit")
        self.objc.objc_getClass.argtypes = [C.c_char_p]
        self.objc.objc_getClass.restype = C.c_void_p
        self.objc.sel_registerName.argtypes = [C.c_char_p]
        self.objc.sel_registerName.restype = C.c_void_p
        self.pool = self.msg(self.cls("NSAutoreleasePool"), "new")
        self.cf = C.CDLL(
            "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation"
        )
        self.cf.CFRelease.argtypes = [C.c_void_p]
        self.cg.CGSessionCopyCurrentDictionary.restype = C.c_void_p
        session = self.cg.CGSessionCopyCurrentDictionary()
        if not session:
            raise RuntimeError("A logged-in macOS desktop session is required.")
        try:
            key = self.msg(
                self.cls("NSString"),
                "stringWithUTF8String:",
                b"CGSSessionScreenIsLocked",
                types=[C.c_char_p],
            )
            locked = self.msg(session, "objectForKey:", key)
            if locked and self.msg(locked, "boolValue", result=C.c_bool):
                raise RuntimeError(
                    "Unlock the macOS desktop before running automation."
                )
        finally:
            self.cf.CFRelease(session)
        self.ax.AXIsProcessTrusted.restype = C.c_bool
        if not self.ax.AXIsProcessTrusted():
            raise RuntimeError(
                "Accessibility permission required for the launching terminal/Python in System Settings > Privacy & Security > Accessibility; then retry."
            )
        if (
            self.run("/usr/bin/stat", "-f", "%Su", "/dev/console")
            != pwd.getpwuid(os.getuid()).pw_name
        ):
            raise RuntimeError(
                "Run as the user logged into the unlocked macOS desktop."
            )
        try:
            enabled = self.run(
                "/usr/bin/osascript",
                "-e",
                'tell application "System Events" to get UI elements enabled',
            )
            if enabled != "true":
                raise RuntimeError(
                    "System Events UI scripting is disabled; check Accessibility permission."
                )
        except subprocess.CalledProcessError as error:
            raise RuntimeError(
                "System Events automation is unavailable. Allow the launching terminal to control System Events in Privacy & Security > Automation, and check Accessibility permission."
            ) from error
        if screenshots:
            self.cg.CGPreflightScreenCaptureAccess.restype = C.c_bool
            if not self.cg.CGPreflightScreenCaptureAccess():
                raise RuntimeError(
                    "Screenshots require Screen Recording permission for the launching terminal/Python. Grant it manually or omit --artifacts."
                )
        # Retain every pasteboard item's types and bytes in memory, never on disk.
        self.board = self.msg(self.cls("NSPasteboard"), "generalPasteboard")
        initial_change = self.msg(self.board, "changeCount", result=C.c_long)
        self.saved = []
        items = self.msg(self.board, "pasteboardItems")
        for i in range(self.msg(items, "count", result=C.c_ulong)):
            item = self.msg(items, "objectAtIndex:", i, types=[C.c_ulong])
            copy = self.msg(self.cls("NSPasteboardItem"), "new")
            types = self.msg(item, "types")
            for j in range(self.msg(types, "count", result=C.c_ulong)):
                kind = self.msg(types, "objectAtIndex:", j, types=[C.c_ulong])
                data = self.msg(item, "dataForType:", kind)
                if not data or not self.msg(
                    copy, "setData:forType:", data, kind, result=C.c_bool
                ):
                    raise RuntimeError(
                        "Cannot safely preserve clipboard; clear it manually before retrying."
                    )
            self.saved.append(copy)
        if self.msg(self.board, "changeCount", result=C.c_long) != initial_change:
            raise RuntimeError(
                "Clipboard changed during preservation; leave the desktop idle and retry."
            )
        self.change = None
        self.pending = None
        self.pointer = None

    def prepare_copy(self):
        # Clear stale data before a real copy action; never return the old clipboard.
        self.run("/usr/bin/pbcopy", input="HAR smoke: awaiting owned app copy")
        self.change = self.msg(self.board, "changeCount", result=C.c_long)
        self.pending = self.change

    def cls(self, name):
        return self.objc.objc_getClass(name.encode())

    def msg(self, obj, selector, *args, types=None, result=C.c_void_p):
        if result is Rect and platform.machine() == "x86_64":
            rect = Rect()
            fn = C.CFUNCTYPE(None, C.POINTER(Rect), C.c_void_p, C.c_void_p, *types)(
                ("objc_msgSend_stret", self.objc)
            )
            fn(C.byref(rect), obj, self.objc.sel_registerName(selector.encode()), *args)
            return rect
        fn = C.CFUNCTYPE(
            result, C.c_void_p, C.c_void_p, *(types or [C.c_void_p] * len(args))
        )(("objc_msgSend", self.objc))
        return fn(obj, self.objc.sel_registerName(selector.encode()), *args)

    def script(self, body):
        assert self.app and self.app.poll() is None, "Owned app is not running"
        return self.run(
            "/usr/bin/osascript",
            "-e",
            f'tell application "System Events"\nset p to first application process whose unix id is {self.app.pid}\ntell p\nset frontmost to true\n{body}\nend tell\nend tell',
        )

    def start(self, app):
        self.app = app
        for _ in range(100):
            try:
                if self.script("return count of windows") != "0":
                    self.resize(1440, 900)
                    time.sleep(1)
                    return
            except subprocess.CalledProcessError:
                pass
            assert app.poll() is None, "App exited before opening a window"
            time.sleep(0.2)
        raise RuntimeError(
            "No owned native window appeared; unlock the desktop and check Accessibility permission."
        )

    def geometry(self):
        values = self.script("return (position of window 1) & (size of window 1)")
        x, y, w, h = map(float, values.split(", "))
        # Standard titled/closable/miniaturizable/resizable GPUI window. AppKit
        # measures its native titlebar rather than hard-coding a pixel offset.
        rect = self.msg(
            self.cls("NSWindow"),
            "contentRectForFrameRect:styleMask:",
            Rect(Point(0, 0), Size(w, h)),
            15,
            types=[Rect, C.c_ulong],
            result=Rect,
        )
        return x, y + h - rect.size.height, w, rect.size.height

    def resize(self, w, h):
        _, _, _, content_h = self.geometry()
        frame_h = float(self.script("return item 2 of size of window 1"))
        self.script(
            f"set size of window 1 to {{{w}, {round(h + frame_h - content_h)}}}"
        )
        time.sleep(0.3)
        _, _, actual_w, actual_h = self.geometry()
        assert abs(actual_w - w) <= 1 and abs(actual_h - h) <= 1, (
            "Desktop must accommodate requested content size"
        )

    def key(self, keys, repeat=1):
        if keys in ("ctrl+c", "ctrl+shift+c", "ctrl+shift+e"):
            self.prepare_copy()
        parts = keys.split("+")
        key = parts[-1]
        modifiers = [
            "command down" if p == "ctrl" else "shift down" for p in parts[:-1]
        ]
        codes = {
            "Down": 125,
            "Up": 126,
            "BackSpace": 51,
            "Escape": 53,
            "Return": 36,
            "Tab": 48,
            "space": 49,
        }
        action = (
            f"key code {codes[key]}" if key in codes else f"keystroke {json.dumps(key)}"
        )
        action += " using {" + ", ".join(modifiers) + "}" if modifiers else ""
        if repeat > 1:
            action = f"repeat {repeat} times\n{action}\ndelay 0.035\nend repeat"
        self.script(action)
        time.sleep(0.25)

    def type_text(self, text):
        self.script(f"keystroke {json.dumps(text, ensure_ascii=False)}")

    def click(self, x, y, clicks=1):
        self.prepare_copy()
        left, top, _, _ = self.geometry()
        self.script("return name")  # Refocus only the owned process before CG input.
        self.cg.CGEventCreateMouseEvent.argtypes = [
            C.c_void_p,
            C.c_uint32,
            Point,
            C.c_uint32,
        ]
        self.cg.CGEventCreateMouseEvent.restype = C.c_void_p
        self.cg.CGEventPostToPid.argtypes = [C.c_int, C.c_void_p]
        self.cg.CGEventSetIntegerValueField.argtypes = [
            C.c_void_p,
            C.c_uint32,
            C.c_int64,
        ]
        target = (left + x, top + y)
        start = self.pointer or target
        motions = [
            (
                5,
                Point(
                    start[0] + (target[0] - start[0]) * step / 8,
                    start[1] + (target[1] - start[1]) * step / 8,
                ),
            )
            for step in range(1, 9)
        ]
        events = [(kind, point, 0) for kind, point in motions] + [
            (kind, Point(*target), count)
            for count in range(1, clicks + 1)
            for kind in (1, 2)
        ]
        for kind, point, count in events:
            event = self.cg.CGEventCreateMouseEvent(None, kind, point, 0)
            if count:
                # kCGMouseEventClickState: AppKit passes this through as clickCount.
                self.cg.CGEventSetIntegerValueField(event, 1, count)
            # PID-directed native events cannot click through into another app.
            self.cg.CGEventPostToPid(self.app.pid, event)
            self.cf.CFRelease(event)
            time.sleep(0.03)
        self.pointer = target
        time.sleep(0.3)

    def clipboard(self, expected=None):
        deadline = time.monotonic() + 5
        while True:
            count = self.msg(self.board, "changeCount", result=C.c_long)
            if count != self.pending:
                value = self.run("/usr/bin/pbpaste")
                self.change = count
                if expected is None or value == expected:
                    return value
            if time.monotonic() >= deadline:
                raise AssertionError(
                    "Owned app did not copy the expected clipboard value"
                )
            time.sleep(0.1)

    def screenshot(self, path):
        # Public CG window metadata is toll-free bridged to NSArray/NSDictionary.
        # Filter on PID and normal window layer, never fall back to screen capture.
        assert self.app and self.app.poll() is None
        self.cg.CGWindowListCopyWindowInfo.argtypes = [C.c_uint32, C.c_uint32]
        self.cg.CGWindowListCopyWindowInfo.restype = C.c_void_p
        windows = self.cg.CGWindowListCopyWindowInfo(1, 0)
        matches = []
        try:
            for i in range(self.msg(windows, "count", result=C.c_ulong)):
                info = self.msg(windows, "objectAtIndex:", i, types=[C.c_ulong])

                def number_for(name, info=info):
                    key = C.c_void_p.in_dll(self.cg, name).value
                    value = self.msg(info, "objectForKey:", key)
                    return self.msg(value, "intValue", result=C.c_int)

                if (
                    number_for("kCGWindowOwnerPID") == self.app.pid
                    and number_for("kCGWindowLayer") == 0
                ):
                    matches.append(number_for("kCGWindowNumber"))
        finally:
            self.cf.CFRelease(windows)
        if len(matches) != 1:
            raise RuntimeError("Cannot uniquely identify owned window for screenshot")
        number = matches[0]
        self.run("/usr/sbin/screencapture", "-x", "-o", f"-l{number}", str(path))

    def close(self):
        # Do not overwrite a clipboard changed by the human during the test.
        if (
            self.change is not None
            and self.msg(self.board, "changeCount", result=C.c_long) == self.change
        ):
            self.msg(self.board, "clearContents", result=C.c_long)
            array = self.msg(self.cls("NSMutableArray"), "array")
            for item in self.saved:
                self.msg(array, "addObject:", item, result=None)
            if self.saved and not self.msg(
                self.board, "writeObjects:", array, result=C.c_bool
            ):
                raise RuntimeError("Could not restore the original clipboard")
        for item in self.saved:
            self.msg(item, "release", result=None)
        self.msg(self.pool, "drain", result=None)
