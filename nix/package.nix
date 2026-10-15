{
  lib,
  pkgs,
  rustPlatform,
  pkg-config,
  clang,
  makeWrapper,
  ...
}:
rustPlatform.buildRustPackage {
  pname = "har-viewer";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../fixtures
      ../tests/har.rs
      ../tests/session.rs
      ../LICENSE
      ../THIRD_PARTY_NOTICES.md
    ];
  };
  cargoHash = "sha256-iVFk49budfryiAuPTsy6n08MnbLk1uS9w8oLe8kGaQk=";
  nativeBuildInputs = [
    pkg-config
    clang
    makeWrapper
  ];
  buildInputs = import ./libraries.nix { inherit pkgs; };
  postInstall = ''
    install -Dm644 LICENSE $out/share/licenses/har-viewer/LICENSE
    install -Dm644 THIRD_PARTY_NOTICES.md $out/share/licenses/har-viewer/THIRD_PARTY_NOTICES.md
  '';
  postFixup = ''
    wrapProgram $out/bin/har-viewer \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath (import ./libraries.nix { inherit pkgs; })
      }:/run/opengl-driver/lib \
      --set-default FONTCONFIG_FILE ${pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; }} \
      --run 'if [ "''${HAR_VIEWER_SOFTWARE:-0}" = 1 ]; then export VK_DRIVER_FILES="${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json"; fi'
  '';
  meta = {
    description = "Private native HTTP archive inspector";
    homepage = "https://github.com/thekorn/har-viewer";
    license = lib.licenses.mit;
    mainProgram = "har-viewer";
    platforms = lib.platforms.linux;
  };
}
