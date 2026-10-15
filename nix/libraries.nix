{ pkgs }:
with pkgs;
[
  fontconfig
  freetype
  libxkbcommon
  wayland
  vulkan-loader
  libGL
  xorg.libX11
  xorg.libxcb
  openssl
  zstd
]
