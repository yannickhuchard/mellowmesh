# Design Language — "Mesh Mall"

The visual, motion and interaction system for the native MellowMesh apps
(Windows, macOS, iOS, Android). It extends the
[brand kit](../../branding/brand_kit.md) and serves the personas in
[ux-research.md](ux-research.md).

> **v2.** v1 ("Living Mesh") used abstract wireframe polyhedra for agents.
> Testing the idea against first-time users showed it felt disconnected and
> technical. v2 grounds everything in a place everyone already knows how to
> use — a mall — and in characters that read like the staff of a serious
> management game.

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

## 2. Helpers are staff, not shapes

Every AI helper is an **illustrated character** — a management-sim staff
portrait: semi-flat, soft lighting, friendly, readable at 24 px, never
cartoonish.

* **Honesty rule:** helpers are drawn and always wear an **"AI" staff
  badge** (with the logo spectrum stripe). **People are never drawn** — they
  are real photos or initials. Human vs AI is obvious by kind of image, not
  only by color.
* **Role props:** each role carries its tool in a corner badge — code
  brackets (Coder), magnifier (Reviewer), rocket (Ops), compass (Scout),
  coins (Ledger), speech bubble (Herald), pen (Writer), book (Tutor), plane
  (Trips), map pin (Mello, the guide).
* **Mood is state:**

| Mood | Pose | Meaning |
| :--- | :--- | :--- |
| Idle | relaxed smile, slow breathing, blinks | ready |
| Thinking | raised brow, thought bubble with 3 dots | working out an answer |
| Working | focused, progress ring around the tool | doing a task (percentage) |
| Raising a hand | waves, soft amber frame pulse | needs you |
| Done | big smile, two sparkles | finished |
| Away | greyed, "z z" | offline / sleeping |
| Talking | mouth animates | speaking (guide, voice) |

**Production:** characters are authored once as layered vector rigs with a
state machine (Rive is a good fit: idle/think/work/raise-hand/celebrate/away
blend states). Runtimes render them natively on iOS/macOS and Android; on
Windows use the vector fallback (SVG/Win2D layers driven by the same state
names) unless a Rive runtime is validated for WinUI. Diversity: skin tones,
hair styles and outfits vary by helper; the guide is consistent everywhere.

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

From the logo gradient: indigo → pink → amber → emerald → cyan, on a calm
"night mall" obsidian.

| Token | Dark | Light | Use |
| :--- | :--- | :--- | :--- |
| `bg` | `#0B0F19` | `#F6F7FB` | background |
| `surface` | `#161C2D` | `#FFFFFF` | cards, sheets |
| `ink` / `ink-muted` | `#F3F4F6` / `#B4BACB` | `#0B0F19` / `#5B6475` | text |
| `primary` (you) | `#6366F1` | `#4F46E5` | your actions, "You are here" |
| `needs-you` | `#F59E0B` | `#B45309` | To-do desk, raised hands |
| `yes` / `done` | `#10B981` | `#047857` | approvals, completion |
| `no` / people | `#EC4899` | `#DB2777` | "not now", Café |
| `live` | `#06B6D4` | `#0891B2` | meeting rooms, streaming |
| guide | `#EEF2FF` bubble, `#7C3AED` desk | same | Mello's callouts |

Type: **Outfit** for signs, titles and numbers; the **system font** for
reading (SF Pro, Roboto Flex, Segoe UI Variable) with Dynamic Type / font
scaling; JetBrains Mono only in developer mode. Text contrast ≥ 4.5:1;
status never conveyed by color alone (mood pose + label + badge).

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
