# Design Language — "Living Mesh"

The visual and motion system for the native MellowMesh apps. It extends the
[brand kit](../../branding/brand_kit.md) and serves the personas in
[ux-research.md](ux-research.md).

## Context: what leads interface design right now

* **Depth is back, used with restraint.** Apple's Liquid Glass (iOS/macOS
  26) moved away from flat design toward layered, translucent material with
  subtle depth; Microsoft's Fluent leans more colorful and dimensional;
  Google's Material 3 Expressive made **spring physics** a theme-level motion
  scheme.
* **2D interfaces with a 3D accent**: interactive 3D objects sit inside
  otherwise calm 2D layouts — a hero object, not a 3D world.
* **Motion that explains**: animation confirms actions and shows state
  rather than decorating.
* **Agents get a visible "presence"**: AI products show agent state with a
  living object (the ubiquitous "orb" with idle/thinking/speaking states),
  plus streaming text and confidence cues.

MellowMesh takes these currents and gives them **one signature that only
MellowMesh can own: the mesh itself**. The logo — an "M" drawn as nodes and
edges in a spectrum gradient — becomes the interface.

## Signature elements

### 1. The Constellation (Mesh home)
A 2.5D graph with depth: **me** at the center (a small "M" mesh core), **my
agents** on the first ring, squads/people/organizations further out and
deeper (smaller, dimmer, blurred). The core computes `x, y, z`; each shell
projects with a gentle perspective and parallax (device tilt on phones,
pointer on desktop). Drag to orbit, pinch to zoom, tap to enter.

### 2. Nodelets (agent avatars)
Each agent is a small **wireframe polyhedron** — nodes as dots, edges as
light lines — generated from its id (`avatar_seed`) and tinted with its
brand accent. Its motion *is* its state:

| State | Nodelet motion |
| :--- | :--- |
| Idle | slow rotation |
| Thinking | vertices breathe, edges shimmer |
| Working | an orbit ring fills with task progress |
| Waiting for me | amber halo pulse + gentle bob |
| Done | one emerald flash, then idle |
| Offline | flattens to 2D, desaturated |

**Humans are never polyhedra**: they are round photo/initial avatars with a
thin mesh ring. Human vs agent is readable by shape alone.

Rendering: plain vector math (rotate 3D points, project, stroke) on the
platform's 2D canvas — SwiftUI `Canvas`/`TimelineView`, Compose `Canvas`,
Win2D / Composition. No 3D engine, crisp at any size, cheap on battery.

### 3. Light threads
Edges between nodes carry **light pulses** when messages flow: a message
from my agent to a squad is a spark traveling along that edge in the
spectrum gradient. Bursty traffic = busy mesh, at a glance.

### 4. Spectrum seam
A 1–2 px line in the logo gradient marks **identity and focus**: the active
conversation header, the focused composer, the edge of a Signal card. Used
sparingly — it is the brand's signature, not a decoration.

### 5. Signal cards (decisions)
A decision rises above the mesh as a glass card with depth and a soft glow.
Options show their outcome color. **Press-and-hold** to confirm fills a ring,
then the OS biometric prompt appears; on success an emerald thread runs
from me to the agent's nodelet.

### 6. Mesh morph (navigation)
Tapping a node zooms the camera into it; the nodelet becomes the
conversation's header avatar (matched geometry / shared element). Back
reverses the flight. Navigation feels like moving *through* my network.

## Color

From the logo gradient: indigo → pink → amber → emerald → cyan.

| Token | Dark | Light | Use |
| :--- | :--- | :--- | :--- |
| `bg` | `#0B0F19` Obsidian | `#F6F7FB` | app background |
| `surface` | `#161C2D` @ 65% + blur | `#FFFFFF` @ 72% + blur | glass panels |
| `surface-raised` | `#1E2640` | `#FFFFFF` | sheets, cards |
| `stroke` | `#FFFFFF` @ 8% | `#0B0F19` @ 8% | hairlines |
| `ink` | `#F3F4F6` | `#0B0F19` | primary text |
| `ink-muted` | `#9CA3AF` | `#5B6475` | secondary text |
| `primary` | `#6366F1` Indigo | `#4F46E5` | primary actions, me |
| `cyan` | `#06B6D4` | `#0891B2` | live / streaming |
| `pink` | `#EC4899` | `#DB2777` | reject, errors |
| `amber` | `#F59E0B` | `#B45309` | waiting for me |
| `emerald` | `#10B981` | `#047857` | approved, done, online |
| `spectrum` | `linear 135°: #6366F1 → #EC4899 → #F59E0B → #10B981 → #06B6D4` | same | seam, me-core, onboarding |

Agent accents rotate through indigo, cyan, pink, amber, emerald
(deterministic from the id). Status is always doubled by shape or label,
never color alone. Light-theme values are darkened to keep text contrast
≥ 4.5:1.

## Typography

| Role | Apple | Android | Windows |
| :--- | :--- | :--- | :--- |
| Body & UI | SF Pro (Dynamic Type) | Roboto Flex (font scaling) | Segoe UI Variable |
| Display (titles, metrics) | **Outfit** 600/800 | **Outfit** 600/800 | **Outfit** 600/800 |
| Developer mode | JetBrains Mono | JetBrains Mono | JetBrains Mono |

System fonts for reading (native feel, accessibility scaling); Outfit
carries the brand in headlines and numbers.

## Shape & material

* Radii: 10 (controls), 16 (cards), 24 (sheets), full (avatars, chips).
* Material is native per platform — Liquid Glass (Apple), Material 3
  tonal surfaces with brand-harmonized dynamic color (Android), Mica /
  Acrylic (Windows). The brand lives in content (mesh, nodelets, seam), the
  chrome stays native.

## Motion

Spring tokens, one definition mapped per platform:

| Token | Feel | SwiftUI | Compose | WinUI (Composition) |
| :--- | :--- | :--- | :--- | :--- |
| `snap` | taps, toggles | `.spring(response: 0.28, dampingFraction: 0.86)` | `spring(dampingRatio = 0.86f, stiffness = 700f)` | `NaturalMotion` damping 0.86, period 45 ms |
| `glide` | navigation, sheets | `.spring(response: 0.5, dampingFraction: 0.9)` | `spring(0.9f, 300f)` | damping 0.9, period 80 ms |
| `bloom` | success, nodelet flash | `.spring(response: 0.45, dampingFraction: 0.7)` | `spring(0.7f, 400f)` | damping 0.7, period 70 ms |
| `drift` | constellation idle | linear, 40–90 s per revolution | same | same |

Rules: interruptible always (gestures can catch any animation); opacity
fades 150–250 ms; nothing loops faster than 1 Hz except "thinking"; **Reduce
Motion** → constellation becomes a static 2D map, morphs become crossfades,
pulses become badges.

## Platform signatures

| | iOS | macOS | Android | Windows |
| :--- | :--- | :--- | :--- | :--- |
| Navigation | Glass tab bar: Mesh · Chats · Signals · Apps; floating Ask | 3-column split view; toolbar Ask | M3E nav bar; FAB Ask; predictive back | NavigationView; Mica; command bar Ask |
| Mesh | full-screen, tilt parallax | sidebar-sized + full window mode, pointer parallax | full-screen, tilt parallax | full window, pointer parallax |
| Decisions | detented sheet + Face ID; lock-screen actions | sheet + Touch ID; notification actions | bottom sheet + BiometricPrompt; notification actions | ContentDialog + Windows Hello; toast buttons |
| Ambient | Live Activity / Dynamic Island: agent working | Menu bar mesh (mini constellation) | Live Update notification; bubbles | Tray mesh; Widgets board |

## MeshUI cards

Agent apps render with native components but share MellowMesh styling: a
16-radius surface, title in Outfit, agent nodelet + name in the header, the
spectrum seam on the left edge when the card belongs to the active task.
Block types: text, key/value, progress, metric, list, chart, image,
actions. Actions use native buttons; destructive actions are pink and
confirm.

## Deliverables

* Claude Design file: four platform clients (Windows, macOS, iOS, Android)
  — Mesh, Chats (agent / squad / person / group / organization), Signal
  sheet, Apps.
* Token export per platform (Swift asset catalog, Compose theme, WinUI
  resource dictionary) — generated from this table when the shells start.
