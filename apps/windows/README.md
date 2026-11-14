# MellowMesh for Windows

The native Windows client of MellowMesh: a WinUI 3 desktop app (Windows App
SDK, Fluent, Mica) for your own MellowMesh network — your AI helpers, the
people you work with, and the requests only you can answer.

It follows the "Mesh Mall in daylight" design language
([docs/design/design-language.md](../../docs/design/design-language.md)):
paper and ink, helpers drawn as clay blobs with an "AI" tag, people as
initials, polite interruptions, Mello the guide.

| Place | What you do there |
| :--- | :--- |
| **Plaza** | Greeting, *Your network*, the amber *Needs you* card, the isometric campus map with the glowing walkway to the next stop, your helpers in their current mood. |
| **Chats** | Helpers, meeting rooms, people, groups, organizations and **team rooms** in one list. MeshUI cards render natively; their buttons send replies (never approvals). |
| **To do** | Your board: requests only you can answer (with "what happens if…") on top, confirmed with **Windows Hello**; then your tasks as a kanban (drag cards between columns). |
| **Shops** | The helpers on your hub, window-shopping style. A helper's page shows its task board and three numbers. |
| **Master board** | Every task on the network as swimlanes (per helper/person or per team), with a live activity strip. |
| **Roadmap** | Live multi-lane timeline: striped = waiting, indigo = working, green = done, amber outline = critical path, dependency arrows, a moving *now* line, zoom (hour/day/week). |
| **Your network** | Name and default (Closed/Open), spaces with lock / key / open padlock, teams (members, roles, team room, board, roadmap). Owners edit; everyone else reads. |
| **Settings** | "How loud can helpers be?" (Glance · Whisper · Tap · Knock · Alarm), quiet hours (21:00–08:00 by default), daily digest, connection, Disconnect. |
| **Help · Mello** | Learn chips, the mall passport, "how do I…?". |

Live updates come over one WebSocket subscription; new requests arrive as
Windows notifications with buttons, as loud as your ladder allows.

## Requirements

* Windows 10 version 1809 (build 17763) or later, or Windows 11 — x64 or ARM64.
  Mica appears on Windows 11; Windows 10 shows a plain background.
* [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0) (8.0.100 or newer; a newer SDK works too).
* Optional: Visual Studio 2022 (17.8+) with the **Windows application development**
  workload (or ".NET desktop development" + "Windows App SDK C# Templates").
* A running MellowMesh hub (`mellowmeshd`), on this PC or reachable on your network.

No MSIX tooling, certificates or Developer Mode are needed: the app is
**unpackaged** (`WindowsPackageType=None`) and carries the Windows App SDK
runtime with it (`WindowsAppSDKSelfContained=true`).

## Build

From the repository root, in PowerShell or `cmd`:

```powershell
dotnet build apps\windows\MellowMesh.App -c Release -p:Platform=x64
```

Use `-p:Platform=ARM64` on ARM PCs. In Visual Studio, open
`apps\windows\MellowMesh.sln`, pick `x64` (or `ARM64`) and press F5.

The one-shot script restores, runs the Core tests, builds and publishes:

```powershell
.\apps\windows\build.ps1            # x64, Release → apps\windows\dist\MellowMesh-win-x64\MellowMesh.exe
.\apps\windows\build.ps1 -Platform ARM64 -Zip
```

### Tests

The UI-free library (`MellowMesh.Core`: hub client, models, conversation
rules, MeshUI parser, interruption ladder, Plaza/campus model, boards,
roadmap, task-event reducer) has xUnit tests that run anywhere .NET 8 runs —
no hub and no network needed (a fake `HttpMessageHandler` and a scripted
WebSocket):

```powershell
dotnet test apps\windows\MellowMesh.Core.Tests
```

## Run

```powershell
dotnet run --project apps\windows\MellowMesh.App -c Release -p:Platform=x64
```

or start the built executable:

```
apps\windows\MellowMesh.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64\MellowMesh.exe
```

## Publish (a folder you can zip and copy)

```powershell
dotnet publish apps\windows\MellowMesh.App -c Release -r win-x64 -p:Platform=x64 -p:PublishSingleFile=false --self-contained true -o apps\windows\dist\MellowMesh-win-x64
Compress-Archive apps\windows\dist\MellowMesh-win-x64\* apps\windows\dist\MellowMesh-win-x64.zip
```

The folder is self-contained (.NET and the Windows App SDK runtime included):
unzip it anywhere and run `MellowMesh.exe`. Windows SmartScreen may warn the
first time because the build is not signed.

## Connect

The first screen is Mello's three-step tour and a **Connect** card with three
ways in.

### The hub on this PC

1. Start the hub: `mellowmeshd` (or the MellowMesh service/installer).
2. Choose **This PC** → **Connect**.

The app reads the hub's owner token from the same place the hub writes it:
next to its database, i.e. `%APPDATA%\mellowmesh\mellowmesh\data\owner.token`
(or the folder of `%MELLOWMESH_DB%` when that variable is set). The token is
read each time from there; nothing is copied. If the hub runs in open mode
without an owner token, the app connects without a key.

### A hub on another computer (LAN, tunnel)

On the hub's computer, listen on the network and name the address you will use:

```bash
mellowmeshd --require-auth --listen 0.0.0.0 --public-host 192.168.1.20:40000
mellowmesh pair --name "DARKSEID" --url http://192.168.1.20:40000
```

`mellowmesh pair` prints a link
`mellowmesh-pair:?v=2&url=…&token=…&name=…&principal=…`. In the app choose
**Pairing link**, paste it (or click *Paste*), and **Connect**. For a hub on
the internet, use the tunnel's `https://…` address as `--url` (see
[docs/network.md](../../docs/network.md)).

You can also choose **Address and key** and type the hub URL and a token
created with `mellowmesh token create`.

### Where the key is kept

Tokens are stored in the **Windows Credential Locker** (`PasswordVault`),
falling back to DPAPI encryption for your Windows user if the locker is
unavailable — never in plain text and never logged. Settings (hub address,
interruption ladder, read markers) live in
`%LOCALAPPDATA%\MellowMesh\settings.json`. **Settings → Disconnect** forgets
the key on this PC; revoke it on the hub with `mellowmesh token revoke <id>`.

## Notifications and Windows Hello

* The app registers for Windows notifications at start (unpackaged
  registration). A request for you arrives as a notification with one button
  per option plus **Quieter…**. A button never answers by itself: it opens the
  To-do desk on that request, which shows what happens and asks for **Windows
  Hello** (face, fingerprint or PIN). Without Hello, a confirmation dialog is
  shown instead.
* Quiet hours let only alarms through; everything quieter than a knock can be
  collected into one daily digest. Requests always wait at the To-do desk.
* There is no tray icon yet.

## Layout

```
apps/windows/
├─ MellowMesh.sln
├─ MellowMesh.Core/          net8.0, no UI: MeshClient (REST + WebSocket), models, pairing,
│                            conversations, MeshUI, interruption ladder, Plaza & campus,
│                            boards, roadmap, task-event reducer
├─ MellowMesh.Core.Tests/    xUnit
├─ MellowMesh.App/           WinUI 3 app (Views/, Controls/, Services/, Themes/)
└─ build.ps1
```

## Troubleshooting

* **"This version of the Windows App SDK requires Microsoft.Windows.SDK.NET.Ref …"** —
  the project pins `WindowsSdkPackageVersion` 10.0.19041.38; update the .NET 8
  SDK if restore cannot find it.
* **The app opens on the welcome screen every time** — the saved key could not
  be read (e.g. the Credential Locker was cleared). Connect again.
* **"Can't reach your hub"** — check that `mellowmeshd` runs and that the
  address in Settings is right; on another PC the hub needs `--listen` and
  `--public-host` (see above).
* **No notifications** — check *Settings → System → Notifications* for
  MellowMesh, Focus/Do not disturb, and the ladder in the app's Settings.
