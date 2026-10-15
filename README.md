# HAR Viewer

A private, native HTTP archive inspector built with **Rust and GPUI**. Inspired by
[0xpanadol/har-viewer](https://github.com/0xpanadol/har-viewer), with a compact dark
workspace, faceted filters and a request inspector. This is a native application,
not a browser wrapper.

## Run with Nix

Enable Nix flakes, then run from an **X11 or Wayland desktop**:

```sh
nix run github:thekorn/har-viewer
nix run github:thekorn/har-viewer -- /path/to/capture.har
nix run github:thekorn/har-viewer -- --sample
```

The repository is currently private. The `github:` shorthand needs Nix's own
GitHub authentication while it stays private. With an existing authenticated Git
credential helper, use this form instead (no token in the command or URL):

```sh
nix run 'git+https://github.com/thekorn/har-viewer?ref=main' -- --sample
```

Linux packages are provided for x86_64 and aarch64. x86_64 is the tested target;
aarch64 uses the same derivation but has not been validated on hardware. macOS and
Windows are not supported in this iteration.

GPUI needs Vulkan. On NixOS, enable `hardware.graphics.enable`. On other Linux
distributions, or machines without a usable Vulkan GPU, use the bundled software
renderer (slower, but does not require a host GPU driver):

```sh
HAR_VIEWER_SOFTWARE=1 nix run github:thekorn/har-viewer -- --sample
```

The first run builds from source and can take several minutes. A desktop session
is still required with software rendering. Native Browse/Save dialogs additionally
need session D-Bus and a working XDG desktop portal backend. **Open HAR** also
accepts a full file path, and **Copy filtered HAR** works without portals.

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

Files are limited to 256 MiB. Body previews show at most 5,000 lines, 2,000 characters
per line; **Copy content** retains the full value. Inspector source scrolls horizontally.
Minimum window size is 1100 × 650. Captures are in memory only; nothing restores on restart.
This is not feature parity with the reference: comparison, replay, sanitized exports,
image previews, JSON trees, regex/body search, saved views, notes/pins, resizing panels,
light theme, websocket tooling and initiator trees remain future work.

## Development: everything in devenv

Only Nix is needed on the host. `flake.lock` pins nixpkgs and devenv; `Cargo.lock`
pins Rust dependencies. The flake's development shell **is** a `devenv.lib.mkShell`
using `devenv.nix`; there is no parallel ad-hoc toolchain or system package setup.

```sh
nix develop --impure
har-dev --sample
har-check       # rustfmt, Clippy -D warnings, Rust tests, nixfmt, statix, deadnix
har-coverage    # domain + session coverage, minimum 90% lines (not a UI coverage claim)
har-build      # release Cargo build
har-package    # sandboxed Nix distribution build, including Rust tests
har-smoke      # real GPUI window on isolated Xvfb + software Vulkan; build debug first
```

For automation use `nix develop --impure -c har-check`, etc. `--impure` lets devenv
locate the checkout; the distribution package itself builds in pure Nix evaluation.
Direnv users can run `direnv allow`. For software rendering during development:
`har-software har-dev --sample`.

```sh
nix develop --impure -c cargo build --locked
nix develop --impure -c har-smoke --artifacts .amp/in/artifacts
nix develop --impure -c har-smoke --binary ./result/bin/har-viewer
```

Tests cover malformed/partial HAR, timezone offsets, timing arithmetic, unknown
sizes, stable sorting, combined filters, Unicode search, base64 errors, metadata
round trips, real shell argument quoting, and navigation after filtering. The
desktop smoke test asserts clipboard and exported data through real keyboard and
pointer input, including sorting, facets, response inspection and file-load recovery.

`libc` is locked to 0.2.186 because GPUI 0.2.2's transitive `xattr` 0.2.3 uses
`ENOATTR`, removed in libc 0.2.190. Do not blindly update that lock entry without
fixing/upgrading the upstream chain. The build currently also reports an upstream
future-compatibility warning in `proc-macro-error2`.

Known dependency advisory: GPUI 0.2.2 pins Taffy 0.9.0, which pulls in Grid 0.18.0
affected by [GHSA-38c5-483c-4qqp](https://github.com/advisories/GHSA-38c5-483c-4qqp)
(moderate, integer overflow in `expand_rows`). Taffy 0.9.2 permits the fixed Grid
1.0.1, but GPUI's exact pin prevents a lockfile-only upgrade. This app uses flex
and absolute layout, and Taffy's inspected grid occupancy code does not call
`expand_rows`; that is not a guarantee of safety. The dependency alert remains
open pending an upstream-compatible fix. Static-analysis success does not mean
the dependency graph is free of known advisories.

### Zed

Open the repository root so Zed loads `.zed/settings.json` and `.zed/tasks.json`.
Launch Zed from the development shell so its language servers inherit the pinned
Rust toolchain and native build libraries (install Zed's CLI first):

```sh
nix develop --impure -c zed .
```

The settings enable Clippy diagnostics with the same warnings-as-errors policy as
`har-check`, and format Rust, Nix and Python on save using the shell's `rustfmt`,
`nixfmt` and Ruff. Formatters explicitly enter the flake shell; the first save may
take longer while Nix prepares it. Generated build and environment directories are
excluded from Zed's file scans; themes and keybindings remain personal preferences.

Rust support is built into Zed, which manages rust-analyzer when it is not on PATH.
Install Zed's **Nix** extension for Nix syntax support; Nix language-server features
additionally need `nil` or `nixd` in your environment. No language-server packages
are added to the development shell. Existing direnv users can run `direnv allow`;
Zed's direct direnv integration is enabled, but launching from the shell also
supplies the environment to Zed-managed language servers.

Use **task: spawn** and search for **HAR:** to run the sample, checks, tests,
coverage, release build, package build or Linux desktop smoke test. Every task runs
from the repository root through `nix develop --impure`; the smoke task builds the
debug binary first and writes evidence under `.amp/in/artifacts`.

## Structure

`src/har.rs` is the pure parsing/analysis/export layer; `src/session.rs` owns visible
selection and navigation. `src/app.rs` renders and coordinates native I/O;
`src/input.rs` is the version-matched GPUI native input implementation. No network
or filesystem operations live in the HAR model. Nix runtime libraries are shared
between the devenv shell and distribution package.

## License and credits

MIT for this application. The text-input adaptation is Apache-2.0; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). The reference project's interaction
model and visual hierarchy guided this independently written Rust application.
