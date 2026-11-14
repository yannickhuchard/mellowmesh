# MellowMesh for Android

The native Android client of MellowMesh: your own mall of AI helpers, on your
phone. It connects to **your** hub (`mellowmeshd`, REST + WebSocket on port
40000) and shows:

- **Plaza** — greeting, your network (open / closed, spaces, teams), the amber
  "Needs you" card, an isometric clay map of the places with the next stop lit,
  and your helpers as clay blobs whose mood follows what they are doing.
- **Chats** — helpers, meeting rooms, people, groups, teams and offices in one
  list; conversations with native MeshUI cards; replies go to the right topic.
- **To do** — requests only you can answer ("what happens if yes / not now /
  how to undo"), confirmed with your fingerprint, face or screen lock; below
  them, your own tasks as a kanban board (long-press a card to move it).
- **Shops** — every helper on your network, by floor (Popular, Study, Writing,
  Money, Work, Everything).
- **All tasks** (master board with filters), **Roadmap** (live multi-lane
  timeline with waiting / running bars, dependencies, critical path and a
  "now" line; pinch and drag to move around), helper and team boards.
- **Network** — spaces (private / closed / open), teams and their rooms; the
  owner can change them, everyone else sees them read-only.
- **Settings** — "How loud can helpers be?" (Glance, Whisper, Tap, Knock,
  Alarm), quiet hours (21:00–08:00 by default), daily digest, connection.

Live updates come over the WebSocket while the app is open (with automatic
reconnect); requests raise a notification that respects your settings.

## Layout

```
apps/android/
├── settings.gradle.kts      root build: includeBuild("core") + :app
├── build.gradle.kts         plugin versions
├── gradlew, gradlew.bat     Gradle 8.14.3 wrapper
├── build-apk.ps1            Windows helper: checks JDK/SDK, builds the APK
├── core/                    pure Kotlin/JVM library (standalone Gradle build)
│   └── src/main/kotlin/io/mellowmesh/core/
│       Models, MeshClient (OkHttp REST + WebSocket), Pairing (link v2),
│       Conversations, Chat, MeshUi, MiniMarkdown, Ladder, HomeModel,
│       Shops, TaskBoard (board, roadmap, live reducer)
└── app/                     the Android app (Jetpack Compose, Material 3)
    └── src/main/java/io/mellowmesh/app/
        data/ (secure config, repository), notify/ (ladder notifications),
        ui/ (theme, clay blob, screens)
```

Everything that is not drawing lives in `core/` and is unit-tested on any
machine with a JDK — no Android SDK needed:

```bash
cd apps/android/core
./gradlew test          # Windows: gradlew.bat test
```

## Requirements

- **Android Studio Ladybug (2024.2) or newer** — simplest: open
  `apps/android/` and press Run.
- or, on the command line: **JDK 17+** and the **Android SDK** with platform
  **android-35** and build-tools, pointed to by `ANDROID_HOME` (or
  `ANDROID_SDK_ROOT`), or by a `local.properties` file in `apps/android/`:

  ```properties
  sdk.dir=C\:\\Users\\you\\AppData\\Local\\Android\\Sdk
  ```

Versions: Gradle 8.14.3, Android Gradle Plugin 8.11.1, Kotlin 2.2.0, Compose
BOM 2025.06.01, minSdk 26 (Android 8.0), target/compile SDK 35.

## Build the APK

Windows (PowerShell or cmd), from `apps\android`:

```powershell
.\gradlew.bat assembleDebug
# or, with checks and a friendly summary:
powershell -ExecutionPolicy Bypass -File .\build-apk.ps1
```

macOS / Linux:

```bash
cd apps/android
./gradlew assembleDebug
```

The APK is written to:

```
app/build/outputs/apk/debug/app-debug.apk
```

## Install it on your phone

- With a USB cable and USB debugging on:
  `adb install -r app/build/outputs/apk/debug/app-debug.apk`
- Or copy the APK to the phone (cable, cloud drive, e-mail) and open it; allow
  "install unknown apps" for the app you open it with when Android asks.

## Pair the phone with your hub

On the PC that runs the hub (replace `192.168.1.20` with the PC's address on
your Wi-Fi — `ipconfig` on Windows shows it as "IPv4 Address"):

```bash
# 1. Start the hub so the phone can reach it (auth is required off-loopback).
mellowmeshd --require-auth --listen 0.0.0.0 --public-host 192.168.1.20:40000

# 2. Mint a key for the phone; this prints a QR code and a pairing link.
mellowmesh pair --name Phone --url http://192.168.1.20:40000
```

On the phone: open MellowMesh → follow Mello's three-step tour → **Connect to
my network** → **Scan the code**. You can also paste the
`mellowmesh-pair:?v=2&url=…&token=…` link, or type the address and key by
hand.

The pairing link contains a secret key: show it only to the phone you are
pairing. To remove a phone later, run `mellowmesh token revoke <id>` on the PC
(and "Disconnect this phone" in the app's Settings).

### Windows firewall

Windows blocks incoming connections by default. Allow inbound TCP port 40000
on **Private** networks only (your home Wi-Fi), from an administrator
PowerShell:

```powershell
New-NetFirewallRule -DisplayName "MellowMesh hub" -Direction Inbound -Protocol TCP -LocalPort 40000 -Action Allow -Profile Private
```

Make sure your Wi-Fi is set to a **Private** network in Windows settings, and
that the phone is on the same Wi-Fi.

## Security notes

- The key (token) is stored with `EncryptedSharedPreferences` (Android
  Keystore), excluded from backups, and never logged.
- Plain `http://` is allowed so phones can pair over home Wi-Fi
  (`res/xml/network_security_config.xml`). The app classifies every address
  and shows a clear warning when plain http would cross the internet; use
  https (for example through a tunnel) outside your home network.
- Answering a request always needs your fingerprint / face / screen lock and a
  live round trip to the hub; answers are never queued. Without a screen lock
  the app refuses to answer.
- MeshUI cards are data, never code: they are validated with the same limits
  as the Rust core, links ask before opening, remote images are not loaded
  automatically, and card buttons only send a reply message — they can never
  answer a request.
