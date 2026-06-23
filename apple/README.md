# Stride — Apple Platform Client

A native SwiftUI client for the Stride cloud service, built with
[The Composable Architecture](https://github.com/pointfreeco/swift-composable-architecture)
(TCA). Runs on **iOS, iPadOS and macOS** (and visionOS) from a single codebase.

The design is inspired by Apple Mail: a three-column layout (mailboxes →
message list → reading view) mapped onto Stride's domain, and it adopts the
Liquid Glass material throughout.

> The Xcode project is still named **Stride** (the scaffold it grew from). The
> product is branded **Stride** in-app. To rename the bundle, set
> `INFOPLIST_KEY_CFBundleDisplayName = Stride` in the target build settings.

## Mail → Stride mapping

| Apple Mail            | Stride                                   |
| --------------------- | ---------------------------------------- |
| Mailboxes / Favorites | Projects + "All Conversations"           |
| Message list          | Threads in the selected project          |
| Reading view          | The conversation (streaming chat)        |

## Features

- **Sign in / create account** against any Stride server (URL is configurable
  and remembered).
- **Three-column navigation** (`NavigationSplitView`) that collapses to a stack
  on iPhone.
- **Agentic conversations** with live **token streaming** over WebSocket, plus
  reasoning ("thinking") disclosure, tool-call chips and tool-output cards.
- **Approval prompts** and **multiple-choice quizzes** rendered inline when the
  agent asks for input mid-run.
- **Markdown rendering** — headings, lists, blockquotes, fenced code (with copy),
  inline emphasis/links and images, tolerant of partially-streamed text.
- **Run control**: cancel an in-flight run; optimistic message echo; automatic
  reconnect to the event stream.
- **Liquid Glass** surfaces (composer, cards, buttons) and system components
  that adopt the material automatically (toolbars, sidebar, sheets, search).

## Architecture

Unidirectional, feature-scoped reducers composed into a single root store.

```
AppFeature                      ← root: auth gate
├── AuthFeature                 ← sign-in / register
└── HomeFeature                 ← split-view coordinator (projects, threads, selection)
    └── ChatFeature             ← one conversation: history, streaming, approvals, quizzes
```

```
Stride/
  App/            AppFeature, RootView, StrideApp (entry)
  Core/           Session, StrideClient (+Live), API models, ThreadEvent decoder
  DesignSystem/   Metrics, colors, Liquid Glass helpers
  Markdown/       Block parser + SwiftUI renderer
  Features/
    Auth/         AuthFeature + AuthView
    Home/         HomeFeature + HomeView (sidebar, thread list, empty state)
    Chat/         ChatFeature + ChatView, message components, composer & cards
```

### Networking

`StrideClient` is a struct-of-closures dependency. The live implementation wraps
`URLSession` for REST and `URLSessionWebSocketTask` for the event stream, exposed
to the reducer as an `AsyncThrowingStream<ThreadEvent, Error>`. Credentials
(server URL + bearer token) live in `Session` and are persisted to
`UserDefaults`.

### Event model

The chat reducer mirrors the server's WebSocket protocol: `Snapshot`,
`AgentDelta`, `ThinkingDelta`, `ToolStarted/Finished`, `WaitingForApproval`,
`WaitingForQuiz`, `RunFinished/Failed/Cancelled`, etc. Streaming deltas render
live; committed messages are reloaded from the REST history so the list stays
authoritative.

## Building

Requires **Xcode 27 or newer** (the project uses the format-110 file). Open
`apple/Stride/Stride.xcodeproj`, select the *Stride* scheme and a destination
(My Mac, an iPhone/iPad simulator, etc.), then run.

Swift Package dependencies (TCA and its transitive packages) are already pinned
in `Package.resolved`; Xcode resolves them on first open.

On first launch, enter your Stride server URL (e.g. `https://stride.example.com`)
and sign in.

## Not yet covered

Parity with the web UI is intentionally scoped to the core experience. Still to
do: workspace file browser / attachments, automations, settings panes, and
push notifications.
