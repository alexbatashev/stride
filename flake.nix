{
  description = "Stride — semi-autonomous agent server, packaged for Nix deployment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    let
      # Overlay so external flakes can pull `stride-server` into their own
      # nixpkgs. package.nix's `stride-frontend` arg is filled from here.
      overlay = final: _prev: {
        stride-frontend = final.callPackage ./nix/frontend.nix { };
        stride-server = final.callPackage ./nix/package.nix { };
      };
    in
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ overlay ];
        };
      in
      {
        packages = {
          default = pkgs.stride-server;
          stride-server = pkgs.stride-server;
          stride-frontend = pkgs.stride-frontend;
        };

        apps.default = {
          type = "app";
          program = "${pkgs.stride-server}/bin/stride";
          meta.description = "Run the Stride server (pass -c <config.toml>)";
        };

        # Frontend is the cheap, deterministic build; keep it in `nix flake
        # check`. The full server build stays opt-in via `nix build`.
        checks.stride-frontend = pkgs.stride-frontend;

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.cargo-nextest
            pkgs.nodejs
            pkgs.pnpm_10
            pkgs.pkg-config
            pkgs.cmake
            pkgs.openssl
          ];
        };
      }
    )
    // {
      overlays.default = overlay;

      nixosModules.default = {
        imports = [ ./nix/module.nix ];
        nixpkgs.overlays = [ overlay ];
      };
    };
}
