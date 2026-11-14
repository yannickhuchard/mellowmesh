# UX Research — Native Apps

Research basis for the native MellowMesh apps (Windows, macOS, iOS, Android).
Companion documents: [native-apps.md](../native-apps.md) (architecture) and
[design-language.md](design-language.md) (visual & motion system).

## Product framing

MellowMesh is **agentic messaging on your own network**. The starting point is
never a feed or a directory: it is *my mesh* — the agents I own, what they are
doing right now, and what they need from me. From there, the same
conversation gesture reaches one agent, a squad of agents, people, mixed
groups, and organizations — and the apps they publish.

Core loop: **see → talk → decide → done.**

1. *See* my agents and their state at a glance.
2. *Talk* to one, several, or a mix of agents and people.
3. *Decide* when an agent needs a human signature — in one native,
   biometric-confirmed gesture, wherever I am.
4. *Done*: the result comes back as a message or a card.

## Six personas

### 1. Léa — the solo AI-native founder
*34, runs a one-person company with 9 agents (coding, ops, support, finance).*

* **Devices**: MacBook (main), iPhone (always), occasionally a Windows PC.
* **Goals**: run the company from anywhere; never be the bottleneck; know
  instantly which agent is stuck.
* **Pains**: approvals buried in terminals and chat apps; no single view of
  "who is doing what"; anxiety about agents acting without sign-off.
* **Key jobs**: approve a deploy from a café (iPhone, Face ID); glance at the
  mesh in the menu bar; ask the support agent for today's summary.
* **Design implications**: the Mesh home and the Signals queue must answer
  "is anything blocked on me?" in under 2 seconds. Live Activity for long
  tasks. Decisions reachable from the lock screen.

### 2. Karim — the indie developer & open-source maintainer
*27, Windows laptop + Android phone, Steam user, runs local coding agents.*

* **Devices**: Windows laptop (gaming + dev), Android phone.
* **Goals**: let agents triage issues and draft PRs overnight; stay in
  control of what gets merged.
* **Pains**: fragmented tools; heavy setup; cloud lock-in.
* **Key jobs**: install from Steam, pair his phone with a QR code in under a
  minute; open a Squad conversation for a repo; approve merges with Windows
  Hello at the desk and BiometricPrompt on the bus.
* **Design implications**: first-run must be 3 steps (install → my mesh is
  already there → add phone). Fluent-native on Windows, Material 3
  Expressive on Android — no "ported" feel.

### 3. Sofia — the engineering lead in a scale-up
*41, leads 12 engineers and ~30 team agents across 4 squads.*

* **Devices**: MacBook, iPhone; company Windows VDI for some tools.
* **Goals**: coordinate humans and agents on the same threads; see
  throughput and bottlenecks; keep an audit trail.
* **Pains**: agents and people talk in separate channels; no shared
  context; unclear accountability.
* **Key jobs**: run a Group conversation with two engineers and three
  agents; reassign a reclaimed task; review the decision history of a
  release.
* **Design implications**: Group and Squad conversations show *who is human
  and who is an agent* at a glance; task and lease state inline; the decision
  record (who, via which device) one tap away.

### 4. Marc — the non-technical independent professional
*52, consultant; uses agents for research, writing and invoicing.*

* **Devices**: iPhone only (plus an iPad at home).
* **Goals**: delegate in plain language; get results as clean, readable
  cards; never see a topic name or a token.
* **Pains**: technical jargon; fear of "breaking something"; too many apps.
* **Key jobs**: "Prepare my weekly client report" to his writing agent;
  approve sending an invoice; read a summary card.
* **Design implications**: zero protocol vocabulary in the default UI
  (topics, leases, tokens hidden behind "Details"); generous type and
  touch targets; MeshUI cards must read like native iOS content.

### 5. Aïcha — the organization admin (IT & security)
*38, governs agents for a 400-person company.*

* **Devices**: Windows workstation, Android work phone.
* **Goals**: decide which agents exist, who owns them, what they can
  touch; revoke a device or a token immediately; prove compliance.
* **Pains**: shadow agents; unclear permissions; no device inventory.
* **Key jobs**: review the Organization space; revoke a lost phone's token
  from her desktop; see every decision approved this week and on which
  device.
* **Design implications**: an Organization view with members, agents,
  devices and policies; destructive actions double-confirmed; audit export.
  Information density is welcome on desktop.

### 6. Tomás — the agent builder
*30, builds agents and their in-chat apps for clients and for the
MellowMesh community.*

* **Devices**: Mac + Android test phone.
* **Goals**: ship an agent whose app (MeshUI card or MCP App) looks great on
  every platform; debug message flow quickly.
* **Pains**: designing four UIs; no way to preview on each OS; opaque
  traffic.
* **Key jobs**: preview a MeshUI card on all four platform renderers; watch
  the live mesh while his agent runs; enable a trace for 15 minutes.
* **Design implications**: a developer mode (traces, raw payloads, topic
  paths) that stays out of everyone else's way; a MeshUI preview gallery.

## Persona × platform matrix

| | Windows | macOS | iOS | Android |
| :--- | :---: | :---: | :---: | :---: |
| Léa (founder) | ○ | ● | ● | |
| Karim (indie dev) | ● | | | ● |
| Sofia (eng lead) | ○ | ● | ● | |
| Marc (professional) | | | ● | |
| Aïcha (admin) | ● | | | ● |
| Tomás (builder) | ○ | ● | ○ | ● |

● primary · ○ secondary

## Jobs to be done (prioritized)

1. **Know if anything needs me** — Signals queue, badges, notifications.
2. **Approve/reject safely from anywhere** — native sheet + biometrics.
3. **Talk to my agent(s) in plain language** — Agent and Squad chats.
4. **See my mesh** — who is online, busy, stuck; what moves.
5. **Work with people and agents together** — Group chats, Organizations.
6. **Use the apps agents publish** — MeshUI cards, MCP Apps.
7. **Add a device / manage access** — QR pairing, per-device tokens.

## Information architecture

Four destinations on every platform — same names, native navigation:

| Destination | Purpose |
| :--- | :--- |
| **Mesh** | Home. 2.5D constellation: me at the center, my agents around me, then squads, people, orgs. "Now" strip: what needs me, what is running. |
| **Chats** | Unified conversations; filter chips: *All · Agents · Squads · People · Groups · Orgs*. |
| **Signals** | Everything waiting for me: decisions first, then reclaimed tasks, mentions. Badge = count of decisions. |
| **Apps** | Apps published by my agents, people and organizations (MeshUI + MCP Apps), pinned or recent. |

Profile/settings (devices, networks, developer mode) live behind the user's
avatar. Compose is a persistent action: "Ask…" with an agent picker.

Platform mapping:

* **iOS**: Liquid Glass tab bar with the four destinations; floating "Ask"
  button; decision sheet as a detented bottom sheet.
* **macOS**: three-column split view (destinations · list · detail);
  menu bar extra with the Now strip; global hotkey for "Ask".
* **Android**: Material 3 Expressive navigation bar; FAB "Ask"; predictive
  back; agent chats as conversation bubbles.
* **Windows**: NavigationView (left), Mica backdrop; tray icon with the Now
  strip; toast decisions; global hotkey for "Ask".

## Key flows

### F1 — First run (all platforms)
Install → the app starts or finds the local hub → **Mesh** shows *my*
agents (already registered by CLI/MCP) → banner "Add your phone" → QR. On a
phone: scan → verify fingerprint → done. Target: < 60 s.

### F2 — Approve from anywhere
Notification "Coder wants to deploy auth-service v2" → expand → options
with outcome colors (approve = emerald, reject = pink, neutral = slate) →
choose → biometric → live confirmation → the agent's node pulses emerald on
the Mesh. Never possible from a widget, web view or agent app.

### F3 — Talk to a squad
Mesh → tap the *claims-api* squad → conversation with three agents and their
current tasks pinned at the top → "@Reviewer check the last PR" → the
reviewer's node lights up; its answer arrives as a MeshUI card.

### F4 — Recover a stuck task
Signals → "Lease expired: Implement OAuth2" → see the last progress →
reassign to another agent (or claim it as a human) in one action.

## Usability principles

* **2-second answer**: from any app launch, "does anything need me?" is
  answered without scrolling.
* **One gesture per intent**: open a chat, decide, ask.
* **Plain language first**: protocol terms only in Details/developer mode.
* **Human vs agent is always visible**: shape and label, not only color.
* **Motion explains state** (thinking, working, waiting, done) — never
  decoration only; everything honors Reduce Motion.
* **Accessibility**: Dynamic Type / font scaling, VoiceOver / TalkBack /
  Narrator labels on every node of the mesh, contrast ≥ 4.5:1 for text,
  status never conveyed by color alone.

## Research plan (to validate with real users)

1. 6 × 45-min interviews (one per persona archetype) on current agent
   workflows and approval habits.
2. Clickable prototypes per platform (the Claude Design file) — task-based
   tests on F1–F4; success rate, time on task, SUS.
3. Diary study (1 week) with 5 founders/devs using the CLI + iPhone build:
   when and where decisions happen.
4. Iterate tokens and motion with Reduce Motion / large-type participants.
