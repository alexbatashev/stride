# Builds the web frontend out of band so the Rust build never needs network or
# a JS toolchain. Produces:
#   $out/dist  - static assets served at runtime via --static-dir
#   $out/ssr   - Argon-generated Rust modules consumed by server/build.rs
#                through ARGON_PREBUILT_DIR
{
  lib,
  stdenv,
  nodejs,
  pnpm_10,
  # sha256 of the offline pnpm store. Regenerate with `lib.fakeHash` and read
  # the expected value from the build error after bumping pnpm-lock.yaml.
  pnpmDepsHash ? "sha256-m+f9Ns+YCqWgpu9OYpsPxNcYVtf49Od7wOAMgSYX2Ss=",
}:
let
  pnpm = pnpm_10;
in
stdenv.mkDerivation (finalAttrs: {
  pname = "stride-frontend";
  version = "0.1.0";

  src = ../server/frontend;

  nativeBuildInputs = [
    nodejs
    pnpm
    pnpm.configHook
  ];

  pnpmDeps = pnpm.fetchDeps {
    inherit (finalAttrs) pname version src;
    fetcherVersion = 3;
    hash = pnpmDepsHash;
  };

  buildPhase = ''
    runHook preBuild

    pnpm run build

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/dist" "$out/ssr"
    cp -r dist/. "$out/dist/"
    cp -r rust/. "$out/ssr/"

    runHook postInstall
  '';

  meta.description = "Static web frontend and SSR modules for the Stride server";
})
