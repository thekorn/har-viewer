<p align="center">
  <img src="assets/icons/256.png" width="128" height="128" alt="HAR Viewer waterfall icon">
</p>

# HAR Viewer

A private, native HTTP archive inspector built with **Rust and GPUI**. Inspired by
[0xpanadol/har-viewer](https://github.com/0xpanadol/har-viewer), with a compact dark
workspace, faceted filters and a request inspector. This is a native application,
not a browser wrapper.

## Run with Nix

Enable Nix flakes, then run from a **macOS desktop** or a **Linux X11/Wayland desktop**:

```sh
nix run github:thekorn/har-viewer
nix run github:thekorn/har-viewer -- --input /path/to/capture.har
nix run github:thekorn/har-viewer -- --sample
```

`--input <har file>` loads that capture immediately on launch. Quote paths containing
spaces. File paths on the command line require `--input`; positional paths are not
accepted. Run without arguments to start without a capture, or use `--sample` for
the bundled fixture. `--input` and `--sample` are mutually exclusive.

The repository is currently private. The `github:` shorthand needs Nix's own
GitHub authentication while it stays private. With an existing authenticated Git
credential helper, use this form instead (no token in the command or URL):

```sh
nix run 'git+https://github.com/thekorn/har-viewer?ref=main' -- --sample
```

Packages and development shells are provided for Linux (x86_64 and aarch64) and
Apple Silicon macOS (aarch64). macOS uses native Cocoa windows and Metal; it does
not need XQuartz, Vulkan, or a Linux VM. The pinned Nixpkgs has dropped Intel Mac
support; Intel macOS and Windows are not supported by this flake.

On Linux, GPUI needs Vulkan. On NixOS, enable `hardware.graphics.enable`. On other Linux
distributions, or machines without a usable Vulkan GPU, use the bundled software
renderer (slower, but does not require a host GPU driver):

```sh
HAR_VIEWER_SOFTWARE=1 nix run github:thekorn/har-viewer -- --sample
```

The first run builds from source and can take several minutes. A desktop session
is still required with software rendering. On Linux, native Browse/Save dialogs
additionally need session D-Bus and a working XDG desktop portal backend. **Open HAR** also
accepts a full file path, and **Copy filtered HAR** works without portals.

On macOS, Nix supplies the SDK and bindgen toolchain; host Xcode is not required.
GPUI compiles embedded Metal shader source when the renderer starts, avoiding
Xcode's proprietary build-time shader compiler. A Metal-capable GPU is required;
`HAR_VIEWER_SOFTWARE` has no effect on macOS.

### Desktop installation and icon

The Nix package includes the Waterfall app icon. On Linux it installs a
`dev.thekorn.har-viewer.desktop` launcher and hicolor PNG/SVG icons. Install the
package through your NixOS/Home Manager configuration or `nix profile install .`
from this checkout, with the profile's `share` directory in the desktop session's
`XDG_DATA_DIRS`. A one-off `nix run` does not register a desktop launcher. GPUI
supplies the matching X11 window class / Wayland app ID; desktop shells use it to
find the icon. X11 window managers that require `_NET_WM_ICON` may still show a
generic window icon because GPUI 0.2.2 does not set that property.

On macOS, `nix develop --impure -c har-package` produces
`result/Applications/HAR Viewer.app`, including its Finder/Dock icon. Launch with
`open "result/Applications/HAR Viewer.app"`; expose the bundle in Applications
through your Nix desktop integration for normal Finder access. The packaged CLI
also executes the binary inside the bundle. Plain `cargo run` is unbundled and
does not provide the macOS app icon.

## First iteration

- Open via command line, native file chooser, path input or file drop; bundled
  fictional shopping capture for exploration. Parsing runs in the background.
- Virtualized request table; sort by method, URL, status, type, size or duration.
  Close the inspector to show the extra table columns.
- Case-insensitive URL/method/status search; status, method, resource type and
  domain filters; slow-request preset. Choices within a facet are OR, facets AND.
  Sidebar counts describe the whole capture; table and overview reflect filters.
- Headers, request payload, JSON/text response, cookies, additive timing phases and
  original entry JSON. Base64 text is decoded; binary content is identified.
- Waterfall and request histogram, overview totals and per-domain distribution,
  local issue checks for errors, latency, large responses and uncached assets.
- Copy URL, shell-quoted cURL, inspector content or filtered HAR. Save filtered HAR
  to a **new** file without overwriting the original. Extensions/metadata survive export.
- Native Unicode input, clipboard, keyboard navigation and recoverable load errors.

No telemetry, uploads, replay or remote requests. HAR files often contain passwords,
cookies and tokens: **exports and clipboard copies are NOT sanitized**. Review them
before sharing. Resource sizes are decoded HAR content sizes, not wire transfer sizes;
unknown sizes contribute zero to totals. Unknown timestamps do not extend capture time.

### Shortcuts

Use **Command (⌘)** instead of **Ctrl** on macOS. In-app hints follow the platform.

| Shortcut | Action |
| --- | --- |
| Ctrl O | Open a capture |
| Ctrl F | Focus request search |
| ↑ / ↓ | Previous / next visible request (outside text fields) |
| Enter | Open the entered path, or return focus to the request list |
| Escape | Dismiss open/error UI and return focus to the request list |
| Ctrl Shift C | Copy selected URL |
| Ctrl Shift E | Copy visible requests as HAR |
| Ctrl Q | Quit |

### Deliberate limits

Files are limited to 1 GiB (1,073,741,824 bytes). Body previews show at most 5,000
lines, 2,000 characters per line; **Copy content** retains the full value.
Inspector source scrolls horizontally.
Unpaired UTF-16 surrogate escapes in JSON string values are loaded as `�`, with a
validation warning in Issues. Copies and exports contain these replacements, not
the original code units; the source file is never modified. Unpaired surrogates
in object keys are rejected to avoid merging distinct fields. Valid surrogate
pairs and literal backslash-u text are preserved; other malformed JSON is rejected.
Minimum window size is 1100 × 650. Captures are in memory only; nothing restores on restart.
This is not feature parity with the reference: comparison, replay, sanitized exports,
image previews, JSON trees, regex/body search, saved views, notes/pins, resizing panels,
light theme, websocket tooling and initiator trees remain future work.

## Development: everything in devenv

Only Nix is needed on the host. `flake.lock` pins nixpkgs and devenv; `Cargo.lock`
pins Rust dependencies. The flake's development shell **is** a `devenv.lib.mkShell`
using `devenv.nix`; there is no parallel ad-hoc toolchain or system package setup.
Use `nix develop --impure` on both platforms for the pinned CI toolchain.
The standalone `devenv shell` command also reads this module, but resolves its own
inputs in `devenv.lock`, independently of `flake.lock`.

```sh
nix develop --impure
har-dev --sample
har-check       # rustfmt, Clippy -D warnings, Rust tests, nixfmt, statix, deadnix
har-coverage    # domain + session coverage, minimum 90% lines (not a UI coverage claim)
har-build      # release Cargo build
har-package    # Nix distribution build, including Rust tests
har-smoke      # Linux: isolated Xvfb + software Vulkan; build debug first
# macOS: har-smoke --use-desktop (see permissions below)
```

For automation use `nix develop --impure -c har-check`, etc. `--impure` lets devenv
locate the checkout; the distribution package itself builds in pure Nix evaluation.
Build sandboxing is separate: it follows the Nix daemon's `sandbox` setting.
In particular, do not treat a successful macOS package build as sandbox validation
when sandboxing is disabled or the daemon rejects an untrusted override.
Direnv users can run `direnv allow`. For software rendering during development:
`har-software har-dev --sample` on Linux. On macOS, `har-software` simply runs its
command with the native Metal renderer.

```sh
nix develop --impure -c cargo build --locked
nix develop --impure -c har-smoke --artifacts .amp/in/artifacts
nix develop --impure -c har-smoke --binary ./result/bin/har-viewer
```

On macOS, the same smoke assertions run against the real desktop, not Xvfb:

```sh
nix develop --impure -c cargo build --locked
nix develop --impure -c har-smoke --use-desktop --artifacts .amp/in/artifacts
nix develop --impure -c har-smoke --use-desktop --binary ./result/bin/har-viewer
```

Unlock the screen and leave the desktop idle during this test. It focuses only
the app it starts, requires room for a 1440 × 900 content area, and preserves the
clipboard in memory (restoring it unless it changed outside the test). Grant the
launching terminal/Python **Accessibility** and **Automation → System Events**
permissions in System Settings. **Screen Recording** is needed only with
`--artifacts`; captures contain only the owned app window and fictional fixture.
The script never changes privacy permissions. Video recording (`--record`) is
Linux-only. None of these automation permissions are needed for normal app use.

Tests cover malformed/partial HAR, timezone offsets, timing arithmetic, unknown
sizes, stable sorting, combined filters, Unicode search, base64 errors, metadata
round trips, real shell argument quoting, and navigation after filtering. The
desktop smoke test asserts clipboard and exported data through real keyboard and
pointer input, including sorting, facets, response inspection and file-load recovery.

CI builds and checks Ubuntu Linux on x86_64 and aarch64, including Nix packages,
the headless CLI contract, and debug and packaged desktop smoke tests. macOS
validation is manual for now; native rendering/interaction must be checked on
an unlocked Mac with Metal and the permissions above.

`libc` is locked to 0.2.186 because GPUI 0.2.2's transitive `xattr` 0.2.3 uses
`ENOATTR`, removed in libc 0.2.190. Do not blindly update that lock entry without
fixing/upgrading the upstream chain. The build currently also reports an upstream
future-compatibility warning in `proc-macro-error2`, and in `block` on macOS.

Known dependency advisory: GPUI 0.2.2 pins Taffy 0.9.0, which pulls in Grid 0.18.0
affected by [GHSA-38c5-483c-4qqp](https://github.com/advisories/GHSA-38c5-483c-4qqp)
(moderate, integer overflow in `expand_rows`). Taffy 0.9.2 permits the fixed Grid
1.0.1, but GPUI's exact pin prevents a lockfile-only upgrade. This app uses flex
and absolute layout, and Taffy's inspected grid occupancy code does not call
`expand_rows`; that is not a guarantee of safety. The dependency alert remains
open pending an upstream-compatible fix. Static-analysis success does not mean
the dependency graph is free of known advisories.

## Structure

`src/har.rs` is the pure parsing/analysis/export layer; `src/session.rs` owns visible
selection and navigation. `src/app.rs` renders and coordinates native I/O;
`src/input.rs` is the version-matched GPUI native input implementation. No network
or filesystem operations live in the HAR model. Nix runtime libraries are shared
between the devenv shell and distribution package.

`assets/icon.svg` is the editable Waterfall artwork. The checked-in PNG sizes and
macOS ICNS are generated from it, with transparent margins. Regenerate on Linux
using `nix develop --impure -c python assets/generate.py`; no extra tools are needed.
After `har-package`, run `nix develop --impure -c python tests/package.py` to check
the packaged launcher/bundle, icon resources and headless CLI on either platform.

## License and credits

MIT for this application. The text-input adaptation is Apache-2.0; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The reference project's interaction
model and visual hierarchy guided this independently written Rust application.
