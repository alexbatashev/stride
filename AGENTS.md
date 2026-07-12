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

## Task completion protocol

Complete the user's initial request end-to-end. Passing tests, compiling, or fixing
one reported symptom does not complete a task when the requested product behavior is
still missing or broken.

Before editing, turn the initial request into an acceptance ledger containing every
explicit requirement, reference behavior, required integration, and verification
step. Keep the ledger current when the user adds requirements or corrects the work.
Do not silently narrow, replace, postpone, or reinterpret an item.

For every implementation task:

- Trace the real control point before changing code.
- Implement the complete behavior, including consumers and integration boundaries.
- Treat regressions introduced by the change as part of the same task.
- Exercise the exact end-to-end workflow requested by the user on the real
  application surface. Use the supplied server, account, route, persisted data,
  viewport, reload sequence, and interaction path when provided.
- For visual work, inspect the rendered result at the reference viewport and verify
  mechanics as well as appearance. Source inspection and component tests are not
  visual verification.
- For data, streaming, or event work, verify initial load, reload, intermediate
  updates, completion, errors, duplicate consumers, and request/payload behavior as
  applicable.
- Run focused regression tests and the repository acceptance checks after live
  behavior works. Tests supplement end-to-end verification; they do not replace it.
- Review the final diff against every acceptance-ledger item and remove accidental
  scope, temporary workarounds, dead code, and dependency pivots unless the user
  explicitly requested them.

Do not ask the user to restart, inspect, take a screenshot, or finish verification
that the agent can perform. Do not revert to the old behavior and call the task
recovered when the requested feature remains incomplete. Continue repairing the
implementation until the original outcome works.

Only claim `Done`, `fixed`, `complete`, or `verified` when every acceptance-ledger
item has direct evidence. If work is blocked, state the exact blocker, the evidence
for it, and every unfinished ledger item. Never convert partial success or an
untested inference into a completion claim.

For JavaScript/TypeScript projects use pnpm instead of npm.

Web frontend is developed with Argon library. See existing code for usage examples.

Argon frontend rules:

- Form or transient state belongs in component `state()`; shared state belongs in
  `server/frontend/src/stores/`. Do not use window events, WeakMaps, host-property
  blobs, or module-level mutable state for app state.
- Use `key` on `.map()` output that renders composed components. Treat key warnings
  as review blockers.
- HTML is escaped by default. Use `unsafeHtml` only directly below a visible
  sanitizer call.
- Use `emit()` and `on:event-name` bindings for component events. Window listeners are
  only for real window concerns or third-party contracts.
- Do not cross shadow boundaries from parent/page code. Set child props or store
  fields instead.
- Components on first-paint paths should be SSR by default. CSR-only components need
  an explicit reason.
- Do not hand-patch generated Argon output. If the compiler cannot express a needed
  UI, file it upstream and mark any temporary workaround as
  `ARGON-WORKAROUND(<issue>)`.

Skip pleasantries and filler words (I'm going to..., apologies, etc). Instead be direct:
Done, fixed, understood. Use simpler words.

Avoid descriptive comments in the code. Make algorithm easy to read. Split functions over
300-400 lines into functions, give functions descriptive verbal names.

Do not try to hide issues from us. Present controversies cleanly and give us chance to
clarify things for you.
