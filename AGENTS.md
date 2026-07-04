# Stride

Stride is a semi-autonomous agentic system that can handle tasks on behalf of
its users. The goal of the project is to provide seamless experience for using
LLMs to handle any day-to-day tasks, be it going through emails, planning the
week in calendar, finding best deals online or coding.

## Current status

The project is in very early stage. There's some core infrastructure and a
prototype of a web interface for interacting with the agent.

## Some thoughts from the author

The project is supposed to re-shape how we think about interfacing with computers.
Rather than trying to throw money and AI tokens at the problem we're trying to fix
the issue at the next level by building proper programmatic interfaces, sandboxes
and safeguards to provide the best possible experience.

Most AI use cases today are inefficient. We're taking pragmatic approach. Sometimes
this requires building things from scratch, like out WASM sandbox for confident
computing or new tools that attack problem in a systematic way.

A small glossary for you:
- You - the agent reading this document
- Me/we/us - the humans contributing to Stride
- Users - people who will interact with Stride
- Core - library in the libs/agent containing building blocks for Stride agents
- Agents - LLM-based systems built within Stride infrastructure
- Cloud agent - client-server interface inside server directory

## General code guidelines

Minimal acceptance criteria:

- All code properly formatted
- `cargo clippy --all-targets -- -D warnings` passes
- All tests pass

For JavaScript/TypeScript projects use pnpm instead of npm.

Web frontend is developed with Argon library. See existing code for usage examples.

Skip pleasantries and filler words (I'm going to..., apologies, etc). Instead be direct:
Done, fixed, understood. Use simpler words.

Avoid descriptive comments in the code. Make algorithm easy to read. Split functions over
300-400 lines into functions, give functions descriptive verbal names.

Do not try to hide issues from us. Present controversies cleanly and give us chance to
clarify things for you.

## Cursor Cloud specific instructions

The VM snapshot already has the toolchain installed: Rust stable, `cargo-nextest`,
`capnproto`, `libssl-dev`, `cmake`, Node 22 + pnpm, and Ollama. The startup update
script only refreshes deps (`rustup default stable`, `pnpm --dir server/frontend install`).

- **Rust toolchain**: the crates use edition 2024, which needs rustc >= 1.85. The base
  image pins an older default (1.83); `rustup default stable` fixes it (done in the
  update script).
- **Build/lint/test** (workspace root): commands live in the guidelines above and in
  `.github/workflows/` — `cargo build --workspace`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo nextest run --workspace`. Frontend
  (`server/frontend`): `pnpm check` (tsc), `pnpm test`, `pnpm run build`. The server's
  `build.rs` builds the frontend automatically during `cargo build`, so Node + pnpm must
  be present.
- **Running the server**: `STRIDE_JWT_SECRET` (>= 32 chars) is required or startup aborts.
  The `server` crate has two binaries, so run `cargo run --bin server -p server -- -c <config>`.
  Default listen addr is `0.0.0.0:3000`; DB defaults to embedded SQLite (no external service).
- **Local LLM via Ollama** (non-obvious): configure the provider as OpenAI-compatible, not
  the native Ollama kind:

  ```toml
  [providers.ollama]
  kind = "OpenAI"
  url = "http://127.0.0.1:11434/v1"
  token = "ollama"
  [models.llama]
  slug = "llama3.2:1b"
  provider = "ollama"
  ```

  Gotchas found during setup: (1) the native `kind = "Ollama"` path hits `/api/chat` and
  its streaming duplicated every token with `llama3.2:1b`; the OpenAI-compatible `/v1`
  path streams cleanly. (2) Use `127.0.0.1`, not `localhost` — the server failed to
  connect when the host resolved to IPv6. (3) The server auto-registers a `default` model
  from the first configured model if none is named `default`.
- **Ollama daemon**: no systemd; start with `ollama serve` (background/tmux). Model
  `llama3.2:1b` is pulled. The latest Ollama (0.31.x) segfaulted on this CPU during model
  warmup; version 0.6.8 works.
