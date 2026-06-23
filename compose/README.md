# Stride — Compose Multiplatform client

A Jetpack Compose / Compose Multiplatform port of the Stride client (the SwiftUI
app lives in `../apple/Stride`). Targets **Android** and **Desktop (JVM)** from a
single `shared` module and follows [Material 3](https://m3.material.io/).

This first cut implements the **threads** feature end to end:

- Sign in / register against a Stride cloud server (server URL + credentials).
- Adaptive **list ↔ detail** layout (`ListDetailPaneScaffold`): a single navigable
  pane on a phone, two panes side by side on tablets and desktop.
- Thread list with search, pull-to-refresh, avatars and a "New" FAB.
- Live conversation over the thread-event **WebSocket**: streaming agent text with
  a blinking cursor, a reasoning disclosure, tool chips/output, a typing indicator,
  and inline **approval** and **quiz** prompts.
- Markdown rendering for agent messages.
- Material You dynamic color on Android 12+, with an indigo brand fallback on
  desktop and older Android; edge-to-edge and predictive back on Android.

## Layout

- [`shared/commonMain`](./shared/src/commonMain/kotlin/me/batashev/stride) — all UI
  and logic:
    - `data/` — serializable models, `ThreadEvent` stream, `Session`, the Ktor
      `StrideClient`.
    - `ui/` — `StrideTheme`, the adaptive `MainScreen`, `auth/`, `threads/`, `chat/`.
    - `Platform.kt` — `expect` for settings storage, dynamic color and the back
      handler; actuals in `androidMain` / `jvmMain`.
- [`androidApp`](./androidApp) / [`desktopApp`](./desktopApp) — thin hosts that call
  `App()`.

## Running

- Android: `./gradlew :androidApp:assembleDebug` (or run from the IDE).
- Desktop: `./gradlew :desktopApp:run`

On first launch, enter your Stride server URL (e.g. `https://stride.example.com`
or `http://10.0.2.2:8080` from the Android emulator) plus your username and
password. The session is persisted per platform.

## Tests

- Desktop: `./gradlew :shared:jvmTest`
- Android host: `./gradlew :shared:testAndroidHostTest`

## Notes / follow-ups

- `compileSdk` is pinned to 36 (AGP 9.0.1), which caps `material3-adaptive` at
  `1.2.0` and the markdown renderer at `0.40.2`. Bump both once the toolchain
  moves to AGP 9.1 / SDK 37.
- Projects sidebar, files and automations from the Apple app are not yet ported;
  the shell is structured so they can be added as further top-level destinations.
- Desktop session storage uses `java.util.prefs`; Android uses `SharedPreferences`.
