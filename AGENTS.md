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
