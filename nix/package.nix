# The Stride cloud agent server, built entirely from source. The web frontend
# is supplied prebuilt (see frontend.nix) through STRIDE_PREBUILT_SSR_DIR, so
# this derivation needs no network or Node toolchain.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  cmake,
  perl,
  nasm,
  openssl,
  makeWrapper,
  stride-frontend,
}:
rustPlatform.buildRustPackage {
  pname = "stride-server";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: _type:
      let
        rel = lib.removePrefix (toString ../. + "/") (toString path);
        top = lib.head (lib.splitString "/" rel);
        base = baseNameOf path;
      in
      builtins.elem top [
        "Cargo.toml"
        "Cargo.lock"
        "code"
        "libs"
        "server"
      ]
      && !builtins.elem base [
        "node_modules"
        "target"
        "dist"
      ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Only the server crate (and its path deps) needs building.
  buildAndTestSubdir = "server";

  nativeBuildInputs = [
    pkg-config
    cmake # aws-lc-sys (hyper-rustls), freetype-sys (typst)
    perl # aws-lc-sys codegen
    makeWrapper
    rustPlatform.bindgenHook # libclang for aws-lc-rs bindgen
  ] ++ lib.optionals stdenv.hostPlatform.isx86_64 [ nasm ]; # aws-lc assembly

  buildInputs = [
    openssl # ldap3 (tls-native) + tokio-native-tls
  ];

  # Skip the bundled frontend build; consume the prebuilt SSR modules instead.
  STRIDE_PREBUILT_SSR_DIR = "${stride-frontend}/ssr";

  # Heavy native trees (typst, aws-lc, eryx) make the test build prohibitive and
  # several suites need network/IMAP. CI runs the test matrix separately.
  doCheck = false;

  postInstall = ''
    mkdir -p "$out/share/stride/static"
    cp -r ${stride-frontend}/dist/. "$out/share/stride/static/"

    wrapProgram "$out/bin/server" \
      --set-default STRIDE_STATIC_DIR "$out/share/stride/static"
    ln -s server "$out/bin/stride"
  '';

  meta = {
    description = "Stride semi-autonomous agent server";
    mainProgram = "stride";
  };
}
