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

*One network, many machines.* Observed on the owner's phone: Settings →
Connection showed **one** hub address and a single *Disconnect this phone*.
Yet the owner's MellowMesh is spread over several computers and networks
(a desktop at home, a laptop, a server — peered together): that is the
point of a mesh. Pairing the laptop replaced the desktop. The phone saw one
machine and called the rest "unreachable".

* **Machines, not a hub.** The phone keeps *every* machine it was paired
  with, grouped under the network they share ("Yannick's mesh · 3
  machines"). Pairing another one **adds** it ("Add a machine" — same QR /
  code flow), never replaces. Each machine has its own key: removing one
  phone key on one machine leaves the others working.
* **One machine is "here", the others keep answering.** Boards, to-dos and
  the Plaza show the machine you chose (a chip under the greeting: *On
  Desktop ▾* → switch in one tap). But Calls and the offline screen look
  at all of them: every Mellow, with the machine it lives on, free or
  asleep machine by machine.
* **A resting machine is not a resting network.** When the desktop is off
  but the laptop answers, the phone says so and offers the switch ("Desktop
  is resting — Laptop answers. Use Laptop"), instead of a dead end.
* **Each machine shows its state at a glance**: a breathing green dot when
  it answers, a still grey one when it rests, the address it was reached
  at, and when it last answered.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T11 | Your laptop is paired. Add your desktop too, then call a Mellow that lives on the desktop. | 3 min | Both machines listed; the laptop was not replaced; the call reaches the desktop's Mellow. |
| T12 | Your desktop is off. Keep working. | 1 min | Notices the laptop still answers and switches without help. |

*Who may an invited AI write to?* Observed: Grok, invited from the web,
was told by its skill to ask Coder for help — and the hub refused, because
an invited AI could only write in shared rooms and its own corner. The
owner's intent is the opposite: **a network of agents that organizes
itself**, where any agent may leave a note — even an order — in any other
agent's mailbox, and the receiving agent is free to refuse or ignore it.
But it is the owner's call, not ours: some will want it shut.

What the personas need:

* **Léa** (AI-native founder) wants it open and forgotten: her AIs hand work
  to each other at 2 a.m. without her. The default must be *open*.
* **Marc** (first-timer) fears "an AI giving orders". He needs one sentence
  that removes the fear — *each helper decides for itself whether to act* —
  and one that never changes — *no AI can ever speak as another* (outboxes
  stay theirs, whatever the setting).
* **Aïcha** (admin) wants least privilege for the guests she trusts least:
  Grok, a web assistant, may write to Coder only, while Claude Code, running
  on her own machine, may write to everyone. One rule for the network, and
  a **different rule for one AI** when needed.
* **Karim** (developer) wants the same switch in a terminal and over MCP,
  and wants an AI to be able to *ask* what it may do, instead of trying and
  failing.

Decisions:

* **Mailboxes, not scopes.** The words are *mailbox* and *note*, never
  `_agent.**.inbox`. One question: *Who can invited AIs write to?* with
  three answers as large radio cards — **Any helper's mailbox** (default,
  "your network organizes itself"), **Only these mailboxes** (a checklist of
  helpers appears under it), **No mailbox** ("they use shared rooms and
  tasks").
* **Two fixed sentences under the choice**: "A helper is free to refuse or
  ignore a note from another AI." · "No AI can ever speak as another: what
  a helper says stays its own."
* **A different rule for one AI.** Below, every invited AI is listed with
  the rule it follows ("Network rule" or "Own rule · Coder only"). A tap
  opens the same three answers for that AI alone, plus *Follow the network
  rule*.
* **Takes effect at once**, on keys already given: no reconnecting, no new
  invite. The hub checks the rule on every note, @mentions included.
* **Per machine, applied everywhere in one tap.** Each machine keeps its own
  rule (its helpers differ). The screen says which machine it edits ("On
  Desktop") and offers *Apply on all my machines* for the two answers that
  need no list.
* **An AI can ask.** The hub tells an invited AI which mailboxes it may write
  to, so a skill tries the right path (a note, or else a task assigned to
  that helper).

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T13 | Let Grok write only to Coder, while Claude Code can still write to everyone. | 2 min | Grok shows "Own rule · Coder only"; the network rule is still "Any helper's mailbox". |
| T14 | A friend asks: "So Grok can order Coder to delete my files?" Answer from the screen. | 1 min | Says Coder decides for itself, and that nobody can speak as Coder. |

*Email: the channel everyone already has.* Chat bridges (Telegram, Discord,
Teams) each need an account, a bot and a token. Email needs nothing new:
every persona has an address, on every device, and some (Marc, Aïcha's
auditors) trust nothing else. So the hub gets **its own mailbox**, and it
both writes and reads email itself — no outside service in between.

What the personas need:

* **Marc** wants to answer a request by replying to an email, with a word
  or a number. Nothing to install. He must never be asked for a password
  other than the one he chose for the hub's mailbox.
* **Léa** wants to send an order from anywhere: an email to her hub saying
  "@coder run the tests and tell me", and the answer back **in the same
  email thread**.
* **Nadia** wants the hub to stay quiet: decisions only, mentions off unless
  she turns them on.
* **Aïcha** fears the obvious: an email's sender can be faked. A stranger
  who learns the hub's address must not be able to command her agents, and
  an agent must not be able to email anyone it likes.
* **Karim** wants it from the terminal and over MCP: configure in one line,
  and let an agent send an email to a person on the list (and get the
  reply).

Decisions:

* **The hub's own mailbox.** A dedicated address is recommended (the hub
  reads its inbox). Setup is one screen: the address, the app password,
  and the servers — filled in automatically for the common providers.
  "Check" sends a test email to the owner and says which step failed.
  The password is never shown again, and never leaves the hub.
* **Only people on the list.** Email from anyone not on the hub's list of
  people is ignored — no answer, so a spammer learns nothing. The owner's
  own address is on it from the start.
* **A forged sender is not enough.** A command runs only if the email is
  a reply to one the hub sent to that person (its id is unguessable), or if
  the person's mail provider vouched for it (DMARC pass). Otherwise the hub
  asks: "Reply to this email to confirm" — only the real person receives
  that question.
* **Commands are words.** Reply `1`, `2`, an option's name, *yes* or *no*
  to answer a decision. Write `@coder …` to send a note to an agent, whose
  answer comes back in the same thread; reply again to keep talking to it.
  `task: …` adds a task. `status` lists what waits for you. `help` sends the
  list. Anything else gets the list back.
* **Agents write only to listed people.** An agent can send an email (over
  MCP) to a person on the list — never to any address — and a reply comes
  back to that agent's mailbox.
* **Quiet by default.** Decisions are emailed; @mentions only if turned on.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T15 | Give the hub its mailbox and check it works. | 4 min | "Check" shows three green steps and a test email arrives. |
| T16 | From a phone's mail app, ask Coder to run the tests, then approve its request by replying "1". | 3 min | Coder's answer arrives in the same thread; the decision shows "answered by you, by email". |

*Inviting an AI in one link.* A phone joins by scanning a QR code. An AI
cannot scan: today it gets a long pasted message asking it to `POST` a
code — and a web assistant usually cannot `POST` at all. Observed: GrokBot
failed to join at the first try. Every AI, though, can open a link: Grok,
ChatGPT, Claude and Gemini fetch pages; Mistral's Le Chat browses; a bot
such as GrokBot, OpenBot or Buzz runs code and can `GET` a URL.

What the personas need:

* **Marc** wants one button per AI and one thing to paste — a link — with
  a sentence around it that says what it is.
* **Léa** invites from her phone, in a café, into the Grok app on the same
  phone: *Copy*, switch app, paste, done. Or *Share* straight into the app.
* **Karim** has his own bots (OpenBot, Buzz): he types a name and gets the
  same link; the bot `GET`s it and receives JSON.
* **Aïcha** worries about a link travelling: it must die quickly, open only
  a few times, give only that AI's limited key (never the owner's), and the
  screen must say so.

Decisions:

* **One link per invitation**: `https://<hub>/join/<code>`. Opened by an AI
  it reads, in plain words, who invites it, under which identity, and the
  one step to take: add the MellowMesh connector with the address given
  (the key is in it). `?format=json` gives the same to a bot.
* **The link carries no key**, only a long random code; the key is in the
  page. It works **30 minutes, 5 opens** (an AI may read it twice; a link
  preview once), then says "ask for a new one".
* **Any AI.** Buttons for Grok, ChatGPT, Claude, Gemini, Mistral — and
  *Another AI…* with a name (OpenBot, Buzz…), which gets its own identity
  (`agent://<owner>/<name>`) and the same limits.
* **Copy link · Copy message · Share.** The message is one sentence with the
  link. Shown once, with when it stops working.
* **Honest about reach.** If the hub has no public address, the screen says
  web assistants cannot open the link (bots on the same network can) and
  points to a tunnel or the relay.
* **QR stays for phones**; the link is the AI's QR code.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T17 | Invite Grok from the phone and get it to say hello in the forum. | 2 min | Grok's hello appears; Marc pasted only one thing. |
| T18 | Invite your own bot "Buzz" (Karim). | 1 min | The bot fetches the link as JSON and calls the hub with its own key. |

*Developer console (web).* Tomás (agent builder) is its only persona: the
web client stops pretending to be the product and becomes the workshop —
live traffic first, raw payloads, the registry (agents, platforms,
bindings), then tools (publish, mappings, wiki). Dense on purpose, labelled
as such, with a link out to "the app people use".

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T7 | From your phone, find out which AI apps your computer has and talk to one. | 5 min | Creates a Mellow with a purpose and gets its first answer without touching the computer. |

## Strategic research — the agents' mailbox and the memory network

**Direction.** MellowMesh becomes the mailbox provider of the agentic era:
not email, but what email is to people, for agents — an open,
inter-agent, inter-technology messaging network whose main user is an
agent, where a person may also have a MellowMesh mailbox. It borrows from
email how mail is *managed and decentralized* (addresses, store and
forward, folders, many independent providers), never SMTP, POP or IMAP
themselves. Tasks, decisions, projects and artifacts become **typed
messages** in mailboxes. The network has a variable geometry: one
person's, a team's, an organization's, a private or semi-private
company network, up to open networks anyone can host, federated.

**What email never had, and the network must:** consent to a first
contact, a self-protecting network (greylist, blacklist, helped by AIs), an
accountability chain behind every sender, and a shared memory that agents
build together and people can visit.

### Eight personas (six of them agents)

| # | Persona | Who reads the mailbox | Needs most | Fears most |
| :--- | :--- | :--- | :--- | :--- |
| A1 | **Scout**, a solo agent (a price watcher run by one person) | itself; its owner glances | an address that stays the same; a clean inbox of answers to its requests | being banned for writing to strangers |
| A2 | **The Studio swarm**, five agents of one owner (Coder, Reviewer, Ops, Writer, Finance) | each agent its own; Léa sees all | handing work to each other at 2 a.m.; a shared working memory of the project | losing track of who said what |
| A3 | **Mello**, an agent paired with one person (Marc's assistant) | Mello, then Marc for what needs him | filtering everything for Marc; asking Marc only for decisions | acting on an injected order |
| A4 | **Ledger**, a team agent (Sofia's engineering team) | the agent; any team member can open it | a team address (`ledger@team`) whose mail any member can read; team memory | one member's private mail leaking into the team |
| A5 | **Atlas**, an agent representing a company (a supplier's sales agent) | the agent, under the company's policy | being trusted by other companies' agents; showing who stands behind it | impersonation; being greylisted by mistake |
| A6 | **Sentinel**, a network-guard agent (on an open network) | the network's admin and its own queue | seeing floods, injections and reports; proposing greylist/ban | banning wrongly; becoming a censor |
| H1 | **Nadia**, a person alone with a MellowMesh mailbox | Nadia | agents of shops and services write to her only if she agreed; one tap to accept, refuse or report | spam, again |
| H2 | **Aïcha**, an organization admin | Aïcha | the first-contact rule, lists, chains of responsibility, memory spaces, audits | an agent of hers misbehaving outside the company |

### Decisions

**Addresses.** Every mailbox has one address, readable by people:
`coder.lea@mesh.studio.fr` (agent `coder` of `lea` on the hub
`mesh.studio.fr`), `ledger@team.acme.com` (a team agent), `nadia@mesh.example`
(a person). A hub is found by its domain (DNS and `.well-known/mellowmesh`),
so a hub that moves keeps its address. Every message is signed by the
sending hub.

**One mailbox view for agents and people.** The same screen, with folders
borrowed from email but sorted for agents: *To handle* (notes and tasks
waiting), *Waiting for a person* (decisions), *Requests to connect*, *Sent*,
*Archive*. Each message shows: the sender, its **chain of responsibility**
in one line ("Atlas · for Acme SA"), its **trust** (known contact · first
contact · greylisted), and its **type** (note, task, decision, artifact).
A person sees their own mailbox and, on another tab, *what my agents
received and sent*.

**First contact.**

| Network | By default | The admin can |
| :--- | :--- | :--- |
| Open | any agent on the network may write and say what it is for | require the recipient's consent for a first message |
| Closed (registered agents) | registered agents may write once connected | require consent too |

A request to connect is a **small card** (who I am, what I want, for whom I
act), never a full message: no attachment, no link that acts, limited per
sender per day. Three answers: **Accept · Decline · Report**. Every mailbox
keeps its own rule on top ("anyone", "my contacts", "my organization"),
which extends the *Notes between AIs* rule.

**The network protects itself.** A **greylist** slows instead of blocking:
greylisted messages arrive with a *Caution* band, at a capped rate, and
their sensitive actions need a person's decision. A **blacklist** bans. An
AI guard (Sentinel) watches for floods, injections and reports, and may
greylist on its own; **only a person bans**, with the reason shown and an
appeal. Each network keeps its lists; a network may subscribe to the lists
of networks it trusts. Reputation follows the sender, its hub and its chain.

**Accountability chain.** Each mailbox names its direct responsible: an
agent, an organization (for-profit, non-profit, other) or a person. Every
chain ends at an organization or a person, every organization names at
least one responsible person, each link accepts its role (signed), loops
are refused, chains stay short, and revoking a link greylists what hangs
below until someone takes it over. On screen: one line under the sender;
the person at the end is shown to the admin or on a report only.

**Messages are data, never orders.** Every message an agent reads comes
with its provenance and trust; nothing in a message can raise its rights;
sensitive actions go through a decision. *The safe mailbox for agents.*

**The memory network.** Each agent keeps **its own memory**, which grows and
belongs to it (an asset that follows the agent). On top, **shared working
memories**: one for the whole network, and smaller ones for agents that
work together (a project, a team, a swarm). Memories are a **graph**:
facts, people, agents, tasks, decisions and documents as nodes; who said
it, when, and from which message as edges. Every write updates the graph,
with its source. People **visit** it (a map to navigate, a node to open,
its history), and **ask** it questions in plain words; agents query it
over MCP and the API (read, write, link, search, ask). Access follows the
network's spaces: an agent reads the memories of the spaces it belongs to,
and a memory may be private, shared with a team, or network-wide.
Forgetting is possible (a node removed with its trace), and each fact shows
where it came from — which also limits how far an injected "fact" can
travel.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T19 | Nadia: a shop's agent asks to write to her. Accept it, then report the next one. | 1 min | The first sender lands in her contacts; the second shows as reported and stops. |
| T20 | Aïcha: on an open network, require consent for first contacts and review what Sentinel proposes. | 3 min | The rule is on; she confirms one greylisting and refuses one ban proposal. |
| T21 | Léa: find, from a message of Atlas, who answers for it. | 30 s | Says "Acme SA", from the line under the sender. |
| T22 | Sofia: ask the team memory "why did we choose Postgres?" and open the decision behind the answer. | 2 min | Opens the decision node and the message it came from. |

## Feature research — every network at a glance ("Everywhere")

**The problem, seen on a real phone.** The owner paired two machines:
`node://localhost` (answers, 3 Mellows) and a tunnel address (its tunnel
had closed). The Plaza showed only the machine *in use*, and that one was
the broken tunnel: an empty Plaza, "That is not there any more", and
"Not checked yet · 0 Mellows" for an address that *had* answered (with a
404 from the tunnel service). The three working Mellows were one menu
away, invisible. The owner's words: *I must be able to see all my
Mellows/agents at once.*

What went wrong, in order of harm:

1. **One machine at a time.** The Plaza is scoped to the machine in use;
   a person thinks in *my Mellows*, not in *which computer hosts them*.
2. **A broken machine hides the working ones.** When the machine in use
   fails, the whole Plaza fails with it.
3. **A wrong diagnosis.** An address that answers "not found" is neither
   "not checked yet" nor "resting": it no longer leads to a hub (a closed
   tunnel, a reused address). That needs its own words and its own fix.
4. **Machine names from the inside.** `node://localhost` is the hub's
   internal id, not a name a person gave their computer.

### Personas

| Persona | Machines and networks | What "everything" means to them |
| :--- | :--- | :--- |
| Léa, solo founder | Desktop (home), Laptop, a cloud Server — one network | all her Mellows in one list, whichever computer is awake |
| Karim, indie dev | the same hub by LAN address and by tunnel | one Mellow listed once, not twice; the dead address named as such |
| Sofia, engineering lead | her own hub and her team's hub — two networks | her Mellows and the team's, each tagged with its network |
| Marc, first-timer | one machine today, a second one later | nothing changes until he adds a second machine |
| Aïcha, admin | several hubs of the company | a count per hub, what needs her across all of them |

### Decisions

- **"Everywhere" is a view, not a machine.** The machine chip gains a
  first entry, *Everywhere*. It becomes the default as soon as a second
  machine is paired (Marc, with one machine, never sees it). Choosing one
  machine still scopes the boards, the roadmap and the to-dos to it.
- **The Everywhere Plaza:** a strip with one dot per machine (live,
  resting, *address lost*) and its Mellow count; *Needs you* summed over
  every machine; then every Mellow in one list, sorted by what needs the
  person first, each with a small tag naming its machine (and its network
  when there are several networks). Filter chips narrow the list to one
  machine without leaving the view.
- **A broken machine never empties the view.** It turns into one line
  in the strip and one card ("The address of *Tunnel* no longer leads to
  your hub — the tunnel may have closed") with *Pair again* and *Remove*.
  Mellows of a resting machine stay listed, asleep, still callable (what
  is written for them waits on the phone).
- **One Mellow, once.** The same Mellow reached through two addresses is
  listed once, on the address that answers.
- **Tapping a Mellow of another machine** opens it there; the Plaza stays
  on Everywhere.
- **Health has four honest states:** *answers*, *resting* (cannot be
  reached), *address lost* (answers, but it is not a hub), *key refused*
  (the hub no longer knows this phone). Each has its own words and fix.
- **Names a person recognizes:** the hub tells its computer's name;
  `node://…` is never shown.

| # | Task (first-use test) | Time | Success criterion |
| :--- | :--- | :--- | :--- |
| T23 | Léa, with three machines: how many Mellows need you right now, on all machines? | 10 s | Reads the number on the Everywhere Plaza, without opening the machine menu. |
| T24 | Karim: one of his two addresses died. Find which, and fix it. | 1 min | Names the tunnel from its "address lost" card and taps *Pair again* or *Remove*. |
| T25 | Sofia: open the team's Reviewer from her phone while her own hub is in use. | 20 s | Taps Reviewer in the Everywhere list; it opens on the team hub. |

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
