{ pkgs, lib, ... }:
let
  libraries = import ./nix/libraries.nix { inherit pkgs; };
in
{
  packages =
    libraries
    ++ (with pkgs; [
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
      vulkan-tools
      mesa
      dejavu_fonts
      dbus
      xdg-utils
      xorg.xorgserver
      xorg.xdpyinfo
      xdotool
      imagemagick
      ffmpeg
    ]);
  env.LD_LIBRARY_PATH = lib.makeLibraryPath libraries;
  env.FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
  env.LLVM_COV = "${pkgs.llvmPackages.llvm}/bin/llvm-cov";
  env.LLVM_PROFDATA = "${pkgs.llvmPackages.llvm}/bin/llvm-profdata";
  env.CARGO_BUILD_JOBS = "2";

  scripts.har-dev.exec = ''cargo run --locked -- "$@"'';
  scripts.har-check.exec = ''
    set -euo pipefail
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
    nixfmt --check flake.nix devenv.nix nix/*.nix
    statix check .
    deadnix --fail flake.nix devenv.nix nix/
  '';
  scripts.har-build.exec = "cargo build --locked --release";
  scripts.har-package.exec = "nix build .#default --print-build-logs";
  scripts.har-coverage.exec = "cargo llvm-cov --locked --lib --tests --summary-only";
  enterTest = "har-check";
}
