# Native Apps — Architecture

MellowMesh ships **fully native clients** on every device a person uses —
Windows, macOS, iPhone, Android — all connected to *their own* MellowMesh
network, with the user's agents at the center of the experience.

Status: **design + shared core**. The shared Rust core
(`crates/mellowmesh-app-core`) exists; the platform shells are next. UX and
visual design live in [design/ux-research.md](design/ux-research.md) and
[design/design-language.md](design/design-language.md).

## Principles

1. **Native per OS, shared logic once.** Each platform gets its own UI built
   with the platform's own toolkit, so it feels like it belongs there (gestures,
   motion, accessibility, system integrations). Everything that is not drawing
   — networking, E2E, sync, state, view models, graph layout, validation —
   lives in one Rust core.
2. **My network first.** Every app opens on the user's own mesh: *my* agents,
   what they are doing, and what they need from me. Other people,
   organizations and their agents come second.
3. **Messaging is the universal surface.** Talking to one agent, a squad of
   agents, a person, a group of people and agents, or an organization is the
   same gesture: open a conversation. Agent "apps" appear inside
   conversations as native cards.
4. **Humans decide, agents act.** Approving a decision is always a native,
   biometric-gated human action on a human token — never something an agent
   or an embedded web view can trigger.

## Architecture

```text
                 ┌──────────────────────────────────────────────┐
                 │  mellowmesh-app-core  (Rust)                  │
                 │  session · relay + E2E · offline sync         │
                 │  view models (home, chats, decisions, tasks)  │
                 │  live events · MeshUI validation · graph      │
                 │  layout · pairing                             │
                 └──────────────┬───────────────────────────────┘
                       UniFFI   │  generated bindings
     ┌──────────────────┬───────┴──────────┬────────────────────┐
     ▼                  ▼                  ▼                    ▼
  SwiftUI            SwiftUI         Jetpack Compose        WinUI 3 (C#)
  iPhone / iPad      Mac             Android                Windows
  XCFramework        XCFramework     cargo-ndk .so + JNA    uniffi-bindgen-cs
```

| Platform | UI toolkit | Core binding | Distribution |
| :--- | :--- | :--- | :--- |
| iOS / iPadOS | SwiftUI (Liquid Glass) | UniFFI Swift → XCFramework | App Store, TestFlight |
| macOS | SwiftUI + AppKit where needed | same XCFramework | Mac App Store, notarized DMG, **Steam** |
| Android | Jetpack Compose (Material 3 Expressive) | UniFFI Kotlin + `cargo-ndk` | Google Play, APK |
| Windows | WinUI 3 / Windows App SDK (Fluent, Mica) | `uniffi-bindgen-cs` (C#) | MSIX / Microsoft Store, **Steam** |

iOS and macOS share one SwiftUI codebase (multiplatform target), so two of
the four devices come from one shell.

### What lives in the core

| Area | Responsibility |
| :--- | :--- |
| Session | Connect to the local daemon or a remote hub through the relay; E2E on by default for remote hubs. |
| Sync | Local SQLite cache per device; catch-up with `since`/replay after sleep or offline. Instant app start. |
| View models | `home()`, conversations, decisions, tasks, agents — plain records the UI renders directly. |
| Live events | One subscription, mapped to typed events (message, decision created/resolved, task claimed/reclaimed/completed, presence). |
| MeshUI | Validation of declarative cards agents attach to messages. |
| Graph | Deterministic force-directed layout (x, y, z) of me → my agents → topics/people/peers, for the Mesh view. |
| Pairing | Parsing/validation of device-pairing QR codes. |

The shells only render, animate, and integrate with the OS.

## Connecting every device to *my* network

### Device pairing

1. On a device that already has access (desktop app or `mellowmesh` CLI),
   *Add device* shows a QR code: `mellowmesh-pair:?relay=…&hub=…&code=…&fp=…`.
2. The new device scans it, verifies the hub key fingerprint (`fp`), and
   redeems the one-time `code` for its **own human token** — scoped,
   named after the device, revocable on its own.
3. The token is stored in the OS vault: Keychain (Apple), Android Keystore,
   Windows Credential Manager. It never leaves the device in clear: all
   traffic through the relay is end-to-end sealed.

Server side, this needs a pairing endpoint on the daemon (one-time codes,
short TTL, owner-approved). *To build.*

### Push notifications

A phone in the background does not keep a WebSocket open. The relay gets a
small **push gateway** (APNs, FCM, WNS): it sends an *encrypted wake-up*
("something needs you"), the app wakes, fetches and decrypts the detail
through the core, and shows a rich notification locally. The relay never sees
content. *To build.* (Also a natural feature of the hosted relay.)

### Offline first

Each device keeps a local cache; the UI always renders from it and the core
reconciles in the background. Actions taken offline (messages) are queued;
**decision answers are never queued** — they require a live, confirmed round
trip.

## The super app: agentic messaging

All interaction is a conversation. The core derives conversation kinds from
topics (conventions documented in the core's README):

| Kind | Who | Example |
| :--- | :--- | :--- |
| **Agent** | me ↔ one agent | "Coder, rebase the auth branch" |
| **Squad** | me + several agents on one topic/project | `_project.claims-api` |
| **Person** | me ↔ a human | direct message |
| **Group** | humans + agents together | design review with two people and the reviewer agent |
| **Organization** | an org's shared space, its people and agents | `_org.acme` |

### Apps inside conversations

Agents, people and organizations can publish **apps**:

1. **MeshUI cards (native first).** A small declarative schema — text,
   key/values, progress, metric, list, chart, image, actions — validated by
   the core and rendered with *native* components on each OS. An agent's
   deployment report looks like a SwiftUI card on iPhone and a Fluent card on
   Windows.
2. **MCP Apps (compatibility).** HTML apps (`ui://…` resources) render in the
   platform web view (WKWebView, Android WebView, WebView2), sandboxed, with
   no access to the token or the native bridge beyond the MCP Apps protocol.

Neither can answer a decision. Card actions publish a reply message into the
conversation; approvals are only performed by the native decision sheet.

## Platform integrations

| | iPhone | Mac | Android | Windows |
| :--- | :--- | :--- | :--- | :--- |
| One-gesture decision | Actionable notification + Face ID | Notification + Touch ID | Notification actions + BiometricPrompt | Toast buttons + Windows Hello |
| Work in progress | Live Activity / Dynamic Island | Menu bar extra | Live Update notification | Tray icon |
| Glanceable | Widgets | Widgets | Glance widgets, Quick Settings tile | Widgets board |
| Quick ask | App Intents / Siri / Shortcuts | Spotlight, global hotkey | Conversation bubbles, share target | Global hotkey, share target |
| Secrets | Keychain | Keychain | Android Keystore | Credential Manager |

## Security rules for all shells

* Human tokens only; one token per device; revocable from any other device.
* Decision answers: native sheet + biometric confirmation + live round trip.
* Untrusted content (message text, MeshUI, MCP Apps) is sanitized/validated
  by the core and sandboxed by the shell. Links open externally after
  confirmation.
* No secrets in logs, analytics or crash reports.

## Distribution

* **App stores**: App Store (iOS, macOS), Google Play, Microsoft Store.
* **Direct**: notarized DMG (macOS), signed MSIX (Windows), APK (Android).
* **Steam** (category: Software → Productivity) for the Windows and macOS
  desktop apps — see [steam.md](steam.md). Steam handles updates for Steam
  installs; the in-app updater is disabled there.

## Keeping four native apps consistent (AI-assisted development)

* **One contract**: the UniFFI types. Features change in the core first;
  each shell is updated against a compiled contract, so drift is a build
  error.
* **Shared scenarios**: fixture files ("3 agents, 1 pending decision, 1
  reclaimed lease") replayed by every shell's snapshot tests — visual
  conformance, like the protocol conformance kit.
* **Shared tokens**: colors, type scale, radii, motion springs defined once
  in [design-language.md](design/design-language.md) and exported per
  platform.

## Roadmap

1. Core (done: first version) → pairing endpoint + push gateway in
   daemon/relay.
2. SwiftUI shell (iPhone + Mac).
3. Compose shell (Android).
4. WinUI 3 shell (Windows) + Steam release of the desktop apps.
