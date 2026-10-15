"""Check the built package's public launcher, icon resources and CLI contract."""

import argparse
import configparser
import os
import plistlib
import shlex
import struct
import subprocess
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--package", type=Path, default=Path("result"))
args = parser.parse_args()
package = args.package.resolve()
app_id = "dev.thekorn.har-viewer"
binary = package / "bin/har-viewer"
commands = [[str(binary)]]

if sys.platform == "darwin":
    bundle = package / "Applications/HAR Viewer.app/Contents"
    info = plistlib.loads((bundle / "Info.plist").read_bytes())
    assert info["CFBundleIdentifier"] == app_id
    assert info["CFBundlePackageType"] == "APPL"
    assert info["CFBundleName"] == "HAR Viewer"
    executable = bundle / "MacOS" / info["CFBundleExecutable"]
    assert executable.is_file() and os.access(executable, os.X_OK)
    icon = (bundle / "Resources" / info["CFBundleIconFile"]).read_bytes()
    assert icon[:4] == b"icns" and struct.unpack(">I", icon[4:8])[0] == len(icon)
    commands.append([str(executable)])
else:
    entry = configparser.ConfigParser(interpolation=None)
    entry.read(package / f"share/applications/{app_id}.desktop")
    launcher = entry["Desktop Entry"]
    assert launcher["Type"] == "Application"
    assert launcher["Name"] == "HAR Viewer"
    assert launcher["StartupWMClass"] == app_id
    assert not launcher.getboolean("Terminal")
    command = shlex.split(launcher["Exec"])
    assert Path(command[0]) == binary, "Launcher must use the packaged executable"
    commands.append(command)
    icons = package / "share/icons/hicolor"
    for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        png = (icons / f"{size}x{size}/apps/{launcher['Icon']}.png").read_bytes()
        assert png[:8] == b"\x89PNG\r\n\x1a\n"
        assert struct.unpack(">II", png[16:24]) == (size, size)
        assert png[25] == 6, "Icons must retain RGBA transparency"
    assert (icons / f"scalable/apps/{launcher['Icon']}.svg").is_file()

environment = dict(os.environ)
for key in (
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "LD_LIBRARY_PATH",
    "FONTCONFIG_FILE",
    "VK_DRIVER_FILES",
):
    environment.pop(key, None)
for command in commands:
    result = subprocess.run(
        [*command, "--help"],
        env=environment,
        capture_output=True,
        text=True,
        timeout=15,
        check=True,
    )
    assert "Usage: har-viewer [--input <har file> | --sample]" in result.stdout
    result = subprocess.run(
        [*command, "--unknown"],
        env=environment,
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert result.returncode == 2 and "Usage:" in result.stderr
print("PASS: packaged launcher, icon resources, help and argument forwarding")
