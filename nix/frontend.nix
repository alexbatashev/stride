# Builds the web frontend out of band so the Rust build never needs network or
# a JS toolchain. Produces:
#   $out/dist  - static assets served at runtime via --static-dir
#   $out/ssr   - Argon-generated Rust modules consumed by server/build.rs
#                through STRIDE_PREBUILT_SSR_DIR
{
  lib,
  stdenv,
  nodejs,
  pnpm_10,
  # sha256 of the offline pnpm store. Regenerate with `lib.fakeHash` and read
  # the expected value from the build error after bumping pnpm-lock.yaml.
  pnpmDepsHash ? "sha256-RkmUz3yjWXVbgNKQasUDrrXMaW8e7colP2PsPM25zUE=",
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

  # `pnpm build` bundles dist/ (esbuild + Argon --js). A second Argon pass emits
  # the SSR Rust modules. The component list mirrors server/build.rs by reading
  # the same ssr-components.txt manifest, so the two can never drift.
  buildPhase = ''
    runHook preBuild

    pnpm run build

    stores=(src/stores/*.ts)
    mapfile -t ssr < <(grep -vE '^[[:space:]]*(#|$)' ssr-components.txt)
    icons=(src/components/icons/*.tsx)
    node_modules/.bin/argon compile "''${stores[@]}" "''${ssr[@]}" "''${icons[@]}" \
      --rust --out-dir ssr-out --flat
    node_modules/.bin/argon compile src/shared/timeline.ts src/shared/model-option.ts \
      --shared --out-dir ssr-out --flat
    node_modules/.bin/argon compile src/pages/threads-page-view.tsx src/pages/shell-page-view.tsx \
      --rust --async-server --out-dir ssr-out --flat

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall

    mkdir -p "$out/dist" "$out/ssr"
    cp -r dist/. "$out/dist/"
    cp ssr-out/*.rs "$out/ssr/"

    runHook postInstall
  '';

  meta.description = "Static web frontend and SSR modules for the Stride server";
})
