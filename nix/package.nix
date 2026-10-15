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
  src = lib.cleanSource ../.;
  cargoHash = lib.fakeHash;
  nativeBuildInputs = [
    pkg-config
    clang
    makeWrapper
  ];
  buildInputs = import ./libraries.nix { inherit pkgs; };
  postFixup = ''
    wrapProgram $out/bin/har-viewer \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (import ./libraries.nix { inherit pkgs; })}
  '';
  meta = {
    description = "Private native HTTP archive inspector";
    homepage = "https://github.com/thekorn/har-viewer";
    license = lib.licenses.mit;
    mainProgram = "har-viewer";
    platforms = lib.platforms.linux;
  };
}
