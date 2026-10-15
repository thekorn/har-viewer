{
  lib,
  pkgs,
  rustPlatform,
  pkg-config,
  clang,
  makeWrapper,
  ...
}:
let
  appId = "dev.thekorn.har-viewer";
  version = "0.1.0";
  desktopItem = pkgs.makeDesktopItem {
    name = appId;
    desktopName = "HAR Viewer";
    comment = "Inspect HTTP archives locally";
    exec = "@out@/bin/har-viewer";
    icon = appId;
    terminal = false;
    categories = [
      "Development"
      "Network"
    ];
    startupWMClass = appId;
  };
in
rustPlatform.buildRustPackage {
  pname = "har-viewer";
  inherit version;
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../assets
      ../fixtures
      ../tests/cli.rs
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
  ]
  ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ rustPlatform.bindgenHook ];
  buildInputs = import ./libraries.nix { inherit pkgs; };
  postInstall = ''
    install -Dm644 LICENSE $out/share/licenses/har-viewer/LICENSE
    install -Dm644 THIRD_PARTY_NOTICES.md $out/share/licenses/har-viewer/THIRD_PARTY_NOTICES.md
  ''
  + lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
    install -Dm644 ${desktopItem}/share/applications/${appId}.desktop \
      $out/share/applications/${appId}.desktop
    substituteInPlace $out/share/applications/${appId}.desktop \
      --replace-fail '@out@' "$out"
    for icon in assets/icons/*.png; do
      size=$(basename "$icon" .png)
      install -Dm644 "$icon" "$out/share/icons/hicolor/''${size}x''${size}/apps/${appId}.png"
    done
    install -Dm644 assets/icon.svg $out/share/icons/hicolor/scalable/apps/${appId}.svg
  ''
  + lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
    app="$out/Applications/HAR Viewer.app/Contents"
    mkdir -p "$app/MacOS" "$app/Resources"
    mv "$out/bin/har-viewer" "$app/MacOS/har-viewer"
    substitute ${./Info.plist} "$app/Info.plist" \
      --subst-var-by appId '${appId}' --subst-var-by version '${version}'
    install -m644 assets/icon.icns "$app/Resources/har-viewer.icns"
    # CLI launches must execute the bundled binary too, preserving bundle identity.
    makeWrapper "$app/MacOS/har-viewer" "$out/bin/har-viewer"
  '';
  postFixup = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
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
    platforms = lib.platforms.linux ++ [ "aarch64-darwin" ];
  };
}
