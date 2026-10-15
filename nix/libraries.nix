{ pkgs }:
with pkgs;
[
  openssl
  zstd
]
++ lib.optionals stdenv.hostPlatform.isLinux [
  fontconfig
  freetype
  libxkbcommon
  wayland
  vulkan-loader
  libGL
  libx11
  libxcb
]
