# Repository Guidelines

## Project Structure & Module Organization
This repository is a Bazel-first monorepo with Rust, Swift, and TypeScript code.

- `libs/`: shared Rust libraries (`core`, `llm`, `minisql`, `minisql-macros`).
- `server/backend/`: Rust backend (`friday-serve`) and integration tests.
- `server/components`: reusable UI components in TypeScript + Lit
- `server/frontend/`: TypeScript/Lit UI pages bundled with esbuild.
- `cli/`: Rust CLI binary (`friday`).
- `apple/Friday/`: Swift app targets (`FridaymacOS`, `FridayiOS`) and Swift tests.
- `vendor/quickjs/`: vendored third-party runtime; avoid editing unless required.

## Build, Test, and Development Commands
Use Bazel for day-to-day work.

- `bazel build //...`: build all configured targets.
- `bazel test //...`: run all tests.
- `bazel run //server/backend:friday-serve`: run the backend server.
- `bazel run //cli:friday`: run the CLI.
- `bazel test //libs/minisql:minisql_int_test`: run minisql integration suite.
- `bazel test //apple/Friday:FridayTests`: run Swift unit tests.
- `bazel run @rules_rust//:rustfmt`: format Rust code.

## Coding Style & Naming Conventions
- Rust uses `edition = "2024"` and is formatted via Bazel rustfmt integration.
- Rust code builds won't succed without proper formatting.
- Follow idiomatic naming: `snake_case` (Rust functions/modules), `PascalCase` (Rust/Swift types), `camelCase` (TS variables/functions).
- Keep modules focused and colocate tests with the owning package (`tests/` for integration, file-local/unit where appropriate).
- Prefer explicit Bazel target dependencies over broad globs unless already established in that package.

## Testing Guidelines
- Rust tests are defined with `rust_test` and suites in package `BUILD` files.
- Swift tests use `swift_test` in `apple/Friday/BUILD`.
- Add or update tests for behavioral changes, especially in `libs/core`, `libs/minisql`, and `server/backend`.
- Before opening a PR, run `bazel test //...`

## Commit & Pull Request Guidelines
- Follow the existing conventional style seen in history: `feat:`, `fix:`, `refactor:` (example: `feat: simple-ish server (#8)`).
- Keep commits scoped to one concern and include affected Bazel targets/tests in the PR description.
- PRs should include: summary, rationale, test evidence (`bazel test ...` output), and screenshots for UI changes (`apple/Friday` or frontend).
- Link related issue/PR numbers when available.
