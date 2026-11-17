# UX Research — Native Apps

Research basis for the native MellowMesh apps (Windows, macOS, iOS, Android).
Companion documents: [native-apps.md](../native-apps.md) (architecture) and
[design-language.md](design-language.md) (Mesh Mall: visual, motion and
interaction system).

## Product framing

MellowMesh is **agentic messaging on your own network**, presented as a
**mall of helpers**. The starting point is never a feed or a directory: it
is the **Plaza** — my own place, with the helpers (AI agents) I own, what
they are doing right now, and what they need from me. From there, the same
conversation gesture reaches one helper, a meeting room of helpers, people,
mixed groups and organizations — and the apps they bring.

The bar: **someone who knows nothing about AI can discover everything on
their own**, guided, with plain words and polite interruptions.

Core loop: **see → talk → decide → done.**

1. *See* my helpers and what needs me at a glance.
2. *Talk* to one, several, or a mix of helpers and people, in my own words.
3. *Decide* when a helper needs a human OK — with a plain-language preview
   of what happens, confirmed natively with biometrics, wherever I am.
4. *Done*: the result comes back as a message or a card.

## Six personas — two of them first-timers

### 1. Marc — the first-timer who fears breaking something · NEW TO AI
*58, independent consultant. Has never used an AI assistant.*

* **Devices**: iPhone only (an iPad at home).
* **Wants**: help writing client reports and invoices — if it's not
  complicated.
* **Worries**: "I don't know what AI is; I'm afraid it will send things in
  my name or that I'll break something."
* **Key moment**: first evening — Mello shows him around, he asks the Writer
  for a report, reads a preview and approves sending it.
* **The app must**: explain each new word where it appears, preview every
  consequence ("if you say yes / not now"), never act without his clear OK,
  use large type and generous touch targets, hide technical vocabulary.

### 2. Nadia — the first-timer drowning in notifications · NEW TO AI
*21, retail team lead and evening student. First time using AI.*

* **Devices**: Android phone (budget), shared Windows PC at the shop.
* **Wants**: plan shifts, revise for her course, find cheaper train
  tickets.
* **Worries**: "Too many notifications already; I don't want another app
  yelling at me."
* **Key moment**: window-shops helpers in Shops, tries the Tutor, then sets
  "only real requests may ring" and quiet hours from the notification
  itself.
* **The app must**: make discovery feel like window-shopping (floors, shop
  windows, "try saying…" examples, safety labels), keep interruptions polite
  and batched, and make tuning them one tap away.

### 3. Léa — the solo AI-native founder
*34, runs a one-person company with 9 helpers (coding, ops, support,
finance).*

* **Devices**: MacBook, iPhone.
* **Wants**: run the company from anywhere without being the bottleneck.
* **Worries**: approvals buried in terminals and chat apps.
* **Key moment**: approves a deploy from a café — Plaza → "Walk me there" →
  To-do desk → hold → Face ID.
* **The app must**: answer "does anything need me?" in two seconds.

### 4. Karim — the indie developer
*27, Windows laptop + Android, Steam user, runs local coding helpers.*

* **Wants**: helpers triage issues and draft PRs overnight; he controls
  merges.
* **Worries**: heavy setup; apps that feel ported.
* **Key moment**: installs from Steam, pairs his phone with a QR code in a
  minute.
* **The app must**: feel native on Windows and Android; three-step first run.

### 5. Sofia — the engineering lead
*41, 12 engineers and ~30 team helpers.*

* **Wants**: people and helpers in the same meeting rooms, with a record.
* **Worries**: unclear accountability between humans and helpers.
* **Key moment**: a release review with two engineers and three helpers.
* **The app must**: show who is human (photo/initials) and who is AI (drawn,
  "AI" badge) at a glance; tasks and requests inline; decision record.

### 6. Aïcha — the organization admin
*38, governs 120 helpers for a 400-person company.*

* **Wants**: know which helpers exist, who owns them, what they can touch.
* **Worries**: helpers without owners; lost devices.
* **Key moment**: spots an unowned helper on the campus map and assigns it;
  revokes a lost phone in two clicks.
* **The app must**: provide an organization campus map, device inventory,
  audit export; density is welcome on desktop.

*Extended persona (not in the core six):* **Tomás, agent builder** — needs a
developer mode (traces, raw payloads, MeshUI previews on all four renderers)
that stays out of everyone else's way.

## Feature research — the network notices your AI platforms

*Problem.* A machine often already runs AI coding assistants — Claude Code,
Codex, Gemini CLI, Antigravity, Cursor, Windsurf, Copilot, a local Ollama —
and MellowMesh cannot see any of them. Each one must be connected by hand
(find the right MCP command, open a terminal, paste it). The mall has
helpers inside, but nobody watches the street.

*Opportunity.* The hub can notice, **on its own machine and with no data
leaving it**, which platforms are installed and which are running right now
— per session: "Claude Code, 2 sessions, working right now". One glance
answers "what AI is active on my machines?", and one action invites a
platform into the network.

*Who needs it first.*

* **Karim (indie dev, primary)** — runs several coding assistants at once.
  Key moment: opens the dashboard, sees "Spotted on this machine: Claude
  Code (2 sessions), Codex (asleep)", taps *Invite* on Claude Code and
  reads the one-line command already filled in.
* **Sofia (eng lead)** — wants to know which assistants her team's hub
  machines actually run before a review. The list is an inventory, not a
  surveillance tool: processes only, never window contents or prompts.
* **Aïcha (admin)** — hygiene: an assistant running on a hub machine that
  is *not* connected (and so not governed) is worth noticing.
* **Marc / Nadia** — never see this card unless something was spotted, and
  then only in plain words ("Claude Code is an AI coding assistant that is
  working on this computer right now").

*Plain-words rule.* "Spotted / working right now / asleep / invite" — never
"process scan", "PID", "detection heuristic". The technical description
(process names matched, exact counts) lives behind the usual ⓘ.

*Privacy rule (sovereignty).* Discovery looks at local process names only,
on the hub's machine, locally; nothing is sent anywhere, and the list is
served only to the owner. No window titles, no file paths, no prompts.

*New job to be done* (slots between 4 and 5 below): **Know which AI
platforms are on my machines — and invite one in one action.**

*Flow F6 — Spot and invite a platform (Karim).* Dashboard (or Windows app
Plaza) → card "Spotted on this machine" → rows with name, state (working
right now · N sessions / asleep / not seen today) → *Invite* → the exact
connect command for that platform, copy button, and "what happens if you
run it" in one sentence → once the platform joins, the row turns mint:
"connected to your network".

*First-use test addition.*

| # | Task | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T6 | Find out which AI assistants are running on this computer, and connect one. | 4 min | Finds the card without the search box; can say afterwards what was shared (nothing left the machine). |

## Feature research — one client to pilot every AI app (v2 of "Spotted")

*What the first version got wrong.* It listed what was *running* (and a
detached hub saw nothing at all), pointed at the computer for every action,
and stopped at "invite". People asked a different question: **"What AI do I
have, and can I just talk to it from here?"**

*Jobs, in the user's words.*

1. "Show me what is **installed** on my machine(s), not only what happens
   to be open." Three plain states: *working right now* · *installed,
   asleep* · *not on this computer*.
2. "Let me **start a chat** with one — or several — from my phone." The
   phone (and the Windows app) is the single remote control: no terminal,
   no window switching.
3. "Let me **make a Mellow out of it, for a purpose**": a name and one
   sentence of mission ("review my pull requests", "plan my week"). It is
   registered on the network, shows up in Shops and Chats like any other,
   and its shop window says where it comes from ("Made from Claude Code on
   darkseid").

*Who.* Karim (indie dev) makes three Mellows from Claude Code, one per
project, and talks to them from the tram. Léa makes "Ops" from Codex and
approves its requests from the café. Marc never sees a platform name unless
he opens *Spotted*: he just meets a new Mellow in Shops.

*Rules.*

* **Nothing is created silently.** A platform becomes a Mellow only when
  the owner names it and gives it a purpose.
* **A Mellow made this way is a guest with no keys**: it answers in words.
  Anything that would touch files or run commands still comes back as a
  request on the To-do desk.
* **Honest states.** If a platform cannot be piloted from here (ChatGPT's
  desktop app has no remote door), the row says so and offers the
  paste-invitation instead — never a button that does nothing.
* **Sovereignty.** Detection reads program names and looks for known
  programs on the machine; conversations go phone → your hub → the
  platform on your computer. No third party is added.

*Flow F7 — From "what do I have?" to a conversation (Karim, phone).*
Plaza → Your network → *Spotted on your computer* → Claude Code ·
*installed* → **Make a Mellow** → name "Reviewer", purpose "Review my pull
requests and tell me what worries you" → the Mellow arrives with the
celebration → its conversation opens with its first words → Karim types.

*How a Mellow gets its voice (decided after the first build).* Running an
AI app's command line in the background on the owner's subscription is the
platform's rule to allow or not, and people are rightly wary of it. So the
default is the opposite direction: **the session the owner already has
open, signed in the normal way, comes and fetches the messages** (MellowMesh
MCP: `mellow_wait` → answer → `mellow_reply`, looping). One host message,
pasted once, makes a session host every Mellow of its platform — including
the ones made later from the phone. Background command-line turns remain
possible but are switched off unless the owner opts in on the hub.

*Connecting a platform: one click, not a ritual.* Every AI app has its own
way of learning about a tool server — a command here, a JSON file there, a
TOML table elsewhere, a settings screen for a fourth. Nobody outside the
trade will do that, and the trade is tired of it. Observed need: **"I have
it installed; connect it."**

* **Four honest states per platform**, never more: *Connected* · *Connect*
  (one click: MellowMesh writes that app's own setting for you) · *Copy the
  setup* (the app has a door but no file MellowMesh may write — paste once
  in its settings) · *Invite by message* (a web assistant: no local door at
  all).
* **Nothing is changed silently.** Discovery never connects on its own: the
  click is the consent. The result says, in one sentence, what was changed
  and where ("Added MellowMesh to Cursor's tool servers — restart Cursor to
  see it"), and the previous file is kept beside it.
* **Never break someone's setup.** If the app's file cannot be read as
  expected, MellowMesh refuses to touch it and falls back to *Copy the
  setup*.
* **From anywhere.** The click works from the phone too: the owner connects
  the Cursor on their desk from the tram.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T8 | Connect an AI app you have installed to your network. | 1 min | One tap; can say what changed and how to undo it. |

*When the network is out of reach — and calling a Mellow.* Observed on a
real phone: the Plaza turned into one pink sentence ("Cannot reach your
hub… Try again") and nothing else. The app went dark exactly when people
most need to know *what to do*, and it forgot every Mellow it had met.

* **Name the situation, then the fix, in places not protocols.** "Your
  network is resting" + three checks, each a sentence: *Is your computer
  on?* · *Is MellowMesh running on it?* (`mellowmesh daemon start`) · *Same
  Wi-Fi, or your tunnel address?* — and one *Try again*.
* **The app keeps working, deferred.** The Mellows the phone has met stay
  on screen. You can still call one and *leave a message*; it waits on the
  phone (encrypted with the rest of its secrets) and leaves by itself the
  moment the network answers — "2 messages will leave when you're back".
  Requests (decisions) never wait: they need the live network and a
  fingerprint, and the app says so.
* **Calls: a second door, beside the Plaza.** A quick-access strip of
  Mellows on the Plaza opens *Calls*: a night-sky dial where each Mellow is
  a glowing orb in its own color. Tapping one *calls* it: rings ripple out,
  "Calling Reviewer…", then it picks up and you speak. Motion is the
  message, so every state has its own movement:
  * **Reachable** — the orb breathes, rings pulse outward, two sparks
    orbit it.
  * **Away** (asleep, no session hosting it, or the network resting) — the
    orb drains to grey, eyes closed, sinks a little, a dashed ring stands
    still and "z z" float up. Its call card says *Away — leave a message*.
  * **Picked up** — a burst of light, the orb rises, the line becomes a
    conversation.
  Reduce Motion keeps the states (color, ring style, label) without the
  movement.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T9 | Your computer is off. Leave a message for a Mellow; then turn it on. | 3 min | Can say why it could not answer right away, and sees the message leave by itself. |
| T10 | Call the Mellow that is free right now. | 30 s | Picks the reachable one at a glance, without reading labels. |

*Developer console (web).* Tomás (agent builder) is its only persona: the
web client stops pretending to be the product and becomes the workshop —
live traffic first, raw payloads, the registry (agents, platforms,
bindings), then tools (publish, mappings, wiki). Dense on purpose, labelled
as such, with a link out to "the app people use".

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T7 | From your phone, find out which AI apps your computer has and talk to one. | 5 min | Creates a Mellow with a purpose and gets its first answer without touching the computer. |

## Persona × platform matrix

| | Windows | macOS | iOS | Android |
| :--- | :---: | :---: | :---: | :---: |
| Marc (first-timer) | | | ● | |
| Nadia (first-timer) | ○ | | | ● |
| Léa (founder) | | ● | ● | |
| Karim (indie dev) | ● | | | ● |
| Sofia (eng lead) | | ● | ● | |
| Aïcha (admin) | ● | | | ● |

● primary · ○ secondary

## Jobs to be done (prioritized)

1. **Know if anything needs me** — To-do desk badge, Plaza "next stop".
2. **Answer safely from anywhere** — preview, hold, biometrics.
3. **Talk to helpers in plain language** — chats and meeting rooms.
4. **Discover helpers** — Shops, guided by Mello.
5. **Control interruptions** — ladder, quiet hours, digest.
6. **Work with people and helpers together** — groups, organizations.
7. **Add a device / manage access** — QR pairing, per-device tokens.

## Information architecture

Four destinations + the guide, same names on every platform:

| Destination | Purpose |
| :--- | :--- |
| **Plaza** | Home: the mall map, "You are here", the next useful stop, "while you were away". |
| **Chats** | All conversations: helpers, meeting rooms, people, groups, organizations. |
| **To do** | Everything only I can answer (requests first), with previews. |
| **Shops** | Discover helpers and their apps by floor (Popular, Study, Writing, Money, Work…). |
| **Help (Mello)** | Tours, "Learn" explanations, "how do I…?", always one tap away. |

## Key flows

### F0 — First run for a first-timer (Marc)
Install → Mello: *a mall of helpers* → *talk like you text* (sample chat) →
*you stay in charge* (sample request) → "Show me around" (walkway to Shops)
or "I'll explore on my own". Passport shows 0/4. Target: understood in
< 2 minutes, no technical word seen.

### F1 — Discover and try a helper (Nadia)
Shops → floor *Study* → Tutor's shop window ("Quiz me on chapter 3", "Only
reads what you share") → Try → first chat; Mello's tip disappears after
"Got it". Passport stamp "Meet a helper".

### F2 — Answer a request from anywhere (Léa, Marc)
Knock notification (or Plaza "next stop") → To-do desk → "If you say yes /
if you say not now" + "can be undone" → choose → hold → biometrics → the
helper reacts (thanks, sparkles) → Passport stamp "Answer a request".

### F3 — Polite interruption in a conversation
Ops taps on the shoulder (peeks from the edge, waves): *Now* (opens the
To-do desk) or *In 10 min* (snoozes; no notification meanwhile).

### F4 — Tune interruptions (Nadia)
From any notification: "Quieter…" → choose the loudest allowed level, quiet
hours, daily digest. Mello explains the consequence of the chosen level.

### F5 — Organization hygiene (Aïcha)
Campus map → unowned helper flagged at the door → Assign; Devices → Revoke a
lost phone.

## Usability principles

* **Plain words first**; technical terms only in "Learn" chips and developer
  mode.
* **Two-second answer** to "does anything need me?".
* **Preview before consequence**: every request explains yes / not now /
  undo.
* **Human vs AI is always visible** by kind of image and the "AI" badge.
* **Polite by default**: the interruption ladder, quiet hours, digest.
* **Motion explains state** and honors Reduce Motion.
* **Accessibility**: Dynamic Type / font scaling, VoiceOver / TalkBack /
  Narrator labels for every place and character ("AI helper Ops, raising a
  hand — needs you"), contrast ≥ 4.5:1, touch targets ≥ 44 px.

## First-use test (Marc and Nadia, unaided)

Think-aloud, 30 minutes, on their own phone. Success = completed without
help *and* able to explain what happened.

| # | Task | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T1 | Find a helper that can write a letter. | 3 min | Reaches Shops → Writing without the search box. |
| T2 | Ask your first question in your own words. | 5 min | Sends a message; notices the helper is AI (badge). |
| T3 | A helper asks for your OK. Decide. | 5 min | Can say what will happen before answering; uses Face ID / fingerprint. |
| T4 | Make helpers quiet after 9 pm. | 4 min | Sets quiet hours from the notification or settings. |
| T5 | Find your way back to the Plaza. | 3 min | Uses the tab or "You are here" without looping on Back. |

Also measured: SUS score, words the participants don't understand
(collected → new "Learn" chips), moments of hesitation before a
consequence, and whether they can tell people from helpers.

## Research plan

1. Run the first-use test with 5 "Marc" and 5 "Nadia" profiles on the
   clickable prototypes; iterate the tour and the copy.
2. 6 × 45-min interviews with experienced users (Léa, Karim, Sofia, Aïcha
   archetypes) on approvals and interruption habits.
3. One-week diary study with first-timers: what helpers they tried, which
   interruptions annoyed them, when they answered requests.
4. Reduce Motion / large-type sessions to validate characters and the map.
