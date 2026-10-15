{ pkgs, lib, ... }:
let
  libraries = import ./nix/libraries.nix { inherit pkgs; };
in
{
  packages =
    libraries
    ++ (with pkgs; [
      git
      cargo
      rustc
      rustfmt
      clippy
      pkg-config
      clang
      cmake
      cargo-llvm-cov
      llvmPackages.llvm
      nixfmt
      statix
      deadnix
      actionlint
      ruff
      vulkan-tools
      mesa
      dejavu_fonts
      dbus
      xdg-utils
      xorg-server
      xdpyinfo
      xdotool
      xclip
      imagemagick
      ffmpeg-full
      python3
    ]);
  env = {
    LD_LIBRARY_PATH = lib.makeLibraryPath libraries;
    FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
    LLVM_COV = "${pkgs.llvmPackages.llvm}/bin/llvm-cov";
    LLVM_PROFDATA = "${pkgs.llvmPackages.llvm}/bin/llvm-profdata";
    CARGO_BUILD_JOBS = "2";
  };

  scripts = {
    har-dev.exec = ''cargo run --locked -- "$@"'';
    har-check.exec = ''
      set -euo pipefail
      cargo fmt --all --check
      cargo clippy --locked --all-targets -- -D warnings
      cargo test --locked
      nixfmt --check flake.nix devenv.nix nix/*.nix
      statix check .
      deadnix --fail flake.nix devenv.nix nix/
      actionlint
      ruff check tests/desktop.py
      ruff format --check tests/desktop.py
    '';
    har-build.exec = "cargo build --locked --release";
    har-package.exec = "nix build .#default --print-build-logs";
    har-coverage.exec = "cargo llvm-cov --locked --no-default-features --lib --tests --summary-only --fail-under-lines 90";
    har-smoke.exec = ''har-software python tests/desktop.py "$@"'';
    har-software.exec = ''
      export VK_DRIVER_FILES="${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json"
      exec "$@"
    '';
  };
  enterTest = "har-check";
}
