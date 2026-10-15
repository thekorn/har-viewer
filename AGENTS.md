# HAR Viewer

- All development, builds, formatting, static analysis and tests run inside the
  devenv-backed flake shell: `nix develop --impure -c <command>`.
- Use `har-check`, `har-coverage`, `har-package` and `har-smoke`; see README.
- GPUI is pinned to 0.2.2. Use its published source, not Zed main's incompatible API.
- Keep parsing/analysis/export independent of GPUI. Extend public contract tests
  in tests/har.rs and tests/session.rs; do not execute network requests.
- Changes to Cargo.lock require updating the fixed cargoHash in nix/package.nix
  and verifying a sandboxed package build.
- UI changes require native rendering and interaction checks, not a browser mock.
  On Linux, `har-smoke --artifacts .amp/in/artifacts` owns its Xvfb display and app
  processes. On macOS, add `--use-desktop` and use an unlocked desktop with the
  Accessibility/Automation permissions documented in README; screenshots also
  require Screen Recording. Do not bypass or change OS privacy permissions.
- HAR data is sensitive. Never upload real user captures, fetch captured URLs,
  or imply exports are sanitized. Use the fictional bundled fixture for evidence.
