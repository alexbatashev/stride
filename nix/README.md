# Deploying Stride with Nix

This flake builds the Stride server (Rust workspace + web frontend) entirely
from source and exposes it as a package and a NixOS module.

## Outputs

| Output | What it is |
| --- | --- |
| `packages.<system>.stride-server` (also `.default`) | The server binary + static assets. |
| `packages.<system>.stride-frontend` | Just the built web assets and SSR modules. |
| `apps.<system>.default` | `nix run` entry point (`stride -c <config>`). |
| `overlays.default` | Adds `stride-server`/`stride-frontend` to any nixpkgs. |
| `nixosModules.default` | The `services.stride` systemd service. |

## Build and run locally

```sh
nix build .#stride-server          # result/bin/stride
nix run  .          -- -c ./config.toml
```

The static frontend is shipped inside the package and located automatically via
`STRIDE_STATIC_DIR`; override with `--static-dir` if needed.

## Use in an external NixOS host

```nix
{
  inputs.stride.url = "github:frontiers-labs/stride"; # this repo

  outputs = { nixpkgs, stride, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        stride.nixosModules.default
        {
          services.stride = {
            enable = true;
            openFirewall = true;
            environmentFile = [ "/run/secrets/stride.env" ];  # secrets, see below
            settings = {
              providers.openai = { kind = "OpenAI"; url = "https://api.openai.com/v1"; };
              models.gpt = { slug = "gpt-4.1"; provider = "openai"; reasoning_effort = "high"; };
              server.listen_addr = "0.0.0.0:3000";
              server.allow_registration = false;
            };
          };
        }
      ];
    };
  };
}
```

`services.stride.settings` mirrors the TOML schema in
[`server/config.toml.example`](../server/config.toml.example) one-to-one
(`providers`, `models`, `server`, `tools`, `mcp`). It renders to a `config.toml`
in the store, so **do not** put tokens there.

## Secrets

Tokens and keys are read from the environment (never the store). Put them in the
file(s) referenced by `environmentFile`, one `KEY=value` per line:

```sh
# /run/secrets/stride.env  (mode 0400, owned by the stride user)
STRIDE_JWT_SECRET=<at least 32 random bytes>        # required
STRIDE_EMAIL_ENCRYPTION_KEY=<stable random secret>  # recommended if IMAP is used
STRIDE_OPENAI_API_KEY=sk-...
STRIDE_BRAVE_API_KEY=...
STRIDE_TELEGRAM_BOT_API_KEY=...
STRIDE_FIRECRAWL_API_KEY=...
STRIDE_MCP_INTERNAL_TOKEN=...                        # STRIDE_MCP_<NAME>_TOKEN
```

Generate one with `agenix`/`sops-nix`, or for a quick test:
`echo "STRIDE_JWT_SECRET=$(head -c32 /dev/urandom | base64)" > stride.env`.

## Updating the frontend lockfile

`nix/frontend.nix` pins the offline pnpm store by hash. After changing
`server/frontend/pnpm-lock.yaml`, set `pnpmDepsHash` to `lib.fakeHash`, run
`nix build .#stride-frontend`, and copy the `got:` value back in.
