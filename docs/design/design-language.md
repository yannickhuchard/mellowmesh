# Design Language — "Mesh Mall in daylight"

The visual, motion and interaction system for the native MellowMesh apps
(Windows, macOS, iOS, Android). It extends the
[brand kit](../../branding/brand_kit.md) and serves the personas in
[ux-research.md](ux-research.md).

> **v3 — "in daylight".** v1 used abstract wireframe polyhedra; v2 used
> drawn staff portraits on a dark, neon UI. Both read as dated or busy.
> v3 follows today's best interface work (restraint, calm paper
> backgrounds, one strong ink, tactile materials, big numerals, playful
> clay characters): light screens, black primary actions, and helpers drawn
> as soft clay companions on a clay-diorama map.

## The idea

Walking into MellowMesh should feel like walking into a well-run mall: you
always know where you are, friendly staff greet you, someone shows you
around, and nobody shouts.

Someone who knows nothing about AI must be able to discover everything —
helpers, conversations, requests, organizations, apps — by wandering, being
guided, and learning words only when they need them.

## 1. A map you already understand

Home is the **Plaza**: an isometric 2.5D map (the look of a simulation game,
not a sci-fi graph) with "You are here" at the fountain — the MellowMesh "M".

| Place | What it is | Technical term (shown only in "Learn") |
| :--- | :--- | :--- |
| **Plaza** | Home. The next useful stop is lit with a glowing walkway. | — |
| **Your helpers** | The AI helpers you own, at work or resting. | agents |
| **Meeting rooms** | Several helpers (and people) on one project. | squads / topics |
| **Café** | People you talk to, one-to-one or in groups. | humans, groups |
| **Offices** | Organizations you belong to and their helpers. | org spaces |
| **To-do desk** | Things only you can answer. | decisions |
| **Shops** | Discover new helpers and the little apps they bring. | agent directory, MeshUI / MCP Apps |
| **Help desk** | Mello, the guide: tours, explanations, "how do I…?" | — |

Navigation tabs use the same plain names on every platform: **Plaza ·
Chats · To do · Shops**, plus the **Help** button (Mello) always in reach.

The map is computed by the core (`graph` layout) and drawn by each shell;
buildings carry badges ("2" at the To-do desk) and helpers stand at their
doors in their current mood.

## 2. Helpers are clay companions

Every AI helper is a **soft clay blob**: one rounded shape, glossy
highlight, two expressive eyes, a color. Simple shapes stay premium at every
size (24 px in a list, 160 px on a welcome screen) where detailed faces look
amateur.

* **Honesty rule:** helpers are blobs and carry an **"AI" tag** next to
  their name; **people are never drawn** — round photos or initials. Human vs
  AI is obvious by shape.
* **Color = identity:** violet (Mello, the guide), indigo, sky, amber, mint,
  pink. Grey means asleep.
* **Mood is state:**

| Mood | Look | Meaning |
| :--- | :--- | :--- |
| Ready | blinks, breathes | idle |
| Thinking | eyes look up, three dots | working out an answer |
| Working | ring around the blob fills with progress | doing a task |
| Needs you | raises a little hand and waves | waiting for a human |
| Done | happy closed eyes (^ ^) | finished |
| Asleep | grey, eyes closed | offline |

**Production:** one vector rig (body, gloss, cheeks, eyes, hand) with a state
machine; on device the shapes are drawn natively (SwiftUI `Canvas` / Compose
`Canvas` / Win2D) or with a Rive file sharing the same state names.

## 3. Interruptions: polite by default

Helpers climb an **interruption ladder** only as far as they need to:

| Level | Motion | When |
| :--- | :--- | :--- |
| 1 · Glance | a dot appears on the place/tab | FYI, no sound |
| 2 · Whisper | quiet strip slides in, fades in 4 s | "Coder finished the login page" |
| 3 · Tap on the shoulder | helper peeks from the screen edge, waves, *Now / Later* | needs you soon, not now |
| 4 · Knock | system notification with the options | a request only you can answer, with a deadline |
| 5 · Alarm | persistent, time-sensitive | money or security at risk; rare, owner-approved, explains why |

* The user picks the **loudest allowed level** (default: Knock). Quiet hours
  (default 21:00–08:00) let only Alarms through; a **daily digest** collects
  everything below Knock.
* Every notification offers **"Quieter…"** — tuning happens where the
  annoyance happens.
* Requests always wait safely at the To-do desk; lowering the volume never
  loses anything.

## 4. Guided, never lost

* **Mello, the guide** (Help desk) greets first-timers with a 3-step tour —
  *a mall of helpers · talk like you text · you stay in charge* — and steps
  aside once the user knows the way. Always reachable from the Help button.
* **Walkways:** a glowing path leads from "You are here" to the next useful
  stop; *Walk me there* animates the camera along it.
* **"Learn" chips:** dotted-underlined words (helper, request, project,
  "checks in every 10 min") open a one-line explanation with a picture.
* **"What happens if…" previews:** before any answer, plain sentences say
  what happens if yes, what happens if not now, and how to undo.
* **Mall passport:** four stamps for first steps — meet a helper, ask a
  question, answer a request, set quiet hours. Then the tour retires.
* **Callouts** (speech bubbles anchored to a place or a control) replace
  modal pop-ups; they never block the screen.

## 5. Emotional motion, with manners

| Reaction | Motion | Spring |
| :--- | :--- | :--- |
| Greet | wave when you arrive at a helper's place | `bloom` |
| Listen | small nod when your message is received | `snap` |
| Think | thought bubble while working out an answer | 1.2 s loop |
| Ask | raised hand + soft amber pulse — never a siren | `glide` |
| Celebrate | smile and two sparkles — no confetti storms | `bloom` |
| Apologize | worried brows, then the next step ("I can retry or undo") | `glide` |

Spring tokens (one definition, mapped per platform):

| Token | SwiftUI | Compose | WinUI (Composition) |
| :--- | :--- | :--- | :--- |
| `snap` | `.spring(response: 0.28, dampingFraction: 0.86)` | `spring(0.86f, 700f)` | NaturalMotion damping 0.86, period 45 ms |
| `glide` | `.spring(response: 0.5, dampingFraction: 0.9)` | `spring(0.9f, 300f)` | damping 0.9, period 80 ms |
| `bloom` | `.spring(response: 0.45, dampingFraction: 0.7)` | `spring(0.7f, 400f)` | damping 0.7, period 70 ms |

Rules: every reaction lasts under a second, can be interrupted, and becomes a
still pose with **Reduce Motion**. Characters never move while the user is
reading or typing in front of them.

## 6. Color & type

Paper, ink, a little color. Black ink carries every primary action; color
belongs to helpers and to meaning.

| Token | Value | Use |
| :--- | :--- | :--- |
| `paper` | `#F4F3EF` | app background |
| `card` | `#FFFFFF` | cards, sheets (hairline `rgba(17,17,20,.06)`, soft shadow) |
| `ink` / `ink-2` / `ink-3` | `#111114` / `#55555C` / `#8A8A90` | text, primary buttons |
| `indigo` | `#4F46E5` | links, "you", progress |
| `amber` | `#F59E0B` (tint `#FFF6E0`) | needs you |
| `green` | `#10B981` (tint `#E7F8F0`) | yes, done |
| `rose` | `#E11D48` (tint `#FFF1F5`) | no, risk |

Type: **Inter** (system font on device) — large and tight for headlines and
numbers (−0.035 em), comfortable 15–17 px body. A dark theme mirrors the same
roles; it is not the default.

The mall map is a pastel **clay diorama**: white buildings with colored
roofs, soft shadows, round trees, a fountain with the MellowMesh "M", and an
amber walkway to the next stop.

## Platform signatures

| | iOS | macOS | Android | Windows |
| :--- | :--- | :--- | :--- | :--- |
| Navigation | Liquid Glass tab bar: Plaza · Chats · To do · Shops; Help pill | split view; sidebar with the same places | Material 3 Expressive nav bar; Help chip | NavigationView + Mica; "Campus map" for orgs |
| Requests | sheet with "what happens if", hold + Face ID | sheet + Touch ID | sheet + BiometricPrompt | dialog + Windows Hello |
| Interruptions | notification actions, Live Activity for work in progress | menu bar helper | notification actions, Live Updates, bubbles | toasts with buttons, tray |
| Discovery | Shops tab | Shops in sidebar | Shops floors (Popular · Study · Writing · Money · Work) | Shops + org catalog |

## Deliverables

* Claude Design file (private, shareable from its page): design language,
  personas & first-use test, Character and Mall-map components, iPhone
  (first run, Plaza, meeting room, To-do desk), Android (Shops, interruption
  settings), macOS (group of people + helpers), Windows (organization
  campus).
* Token export per platform (Swift asset catalog, Compose theme, WinUI
  resource dictionary) and the character rig when the shells start.
