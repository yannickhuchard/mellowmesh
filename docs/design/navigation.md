# Android — Navigation graph and interruption audit

Every screen, every way in and out, and every place the walk used to stop.
Companion to [design-language.md](design-language.md) and
[ux-research.md](ux-research.md). The rule being audited: **no screen may be
a dead end** — wherever you are, the next useful step is one tap away.

## The graph

```mermaid
flowchart TD
    Welcome["Welcome\n(tour + connect)"] -->|paired| Plaza

    subgraph tabs [Tab bar]
        Plaza
        Chats
        Todo["To do"]
        Shops
    end

    Plaza -->|map: To-do desk| Todo
    Plaza -->|map: Shops| Shops
    Plaza -->|map: other places| Chats
    Plaza -->|map: Teams / network card| Network
    Plaza -->|helper blob| Helper
    Plaza -->|master board| Board
    Plaza -->|roadmap| Roadmap
    Plaza -->|gear| Settings
    Plaza -->|Help pill| Help

    Chats -->|row| Conversation
    Chats -->|empty state: Meet your helpers| Shops

    Shops -->|Say hello| Conversation
    Shops -->|shop window| Helper

    Todo -->|All tasks| Board
    Todo -->|Roadmap| Roadmap

    Conversation -->|@helper| Helper
    Conversation -->|team chip| Team
    Helper -->|Chat| Conversation
    Helper -->|its board| Board

    Network -->|space| SpaceEdit
    Network -->|team| Team
    Team -->|room| Conversation
    Team -->|board| Board
    Board -->|Roadmap| Roadmap

    Notice["Tap on the shoulder\n(overlay, any screen)"] -->|Now: decision| Todo
    Notice -->|Now: message| Conversation
```

## Interruptions found, and what was done

| # | Interruption | State |
| :-- | :-- | :-- |
| I1 | **Chats, empty, was a dead end**: "Say hello to one of your helpers" with nothing to tap. | **Fixed** — "Meet your helpers" button leads to Shops, where *Say hello* starts the conversation. |
| I2 | **Helpers came from nowhere**: `builder` and `scout` appeared with no origin. | **Fixed** — every shop window names its origin: a demo badge ("from `mellowmesh demo`") or *Brought by \<owner\>*. |
| I3 | **The Connect screen was a wall of cramped pills.** | **Fixed** — centered hero, one dominant *Scan the code* action, the other ways in as an airy list under "Not in front of the computer?". |
| I4 | **Discovery lived only on the computer**: the phone could not see which AI platforms run there. | **Fixed** — *Your network* shows "Spotted on your computer", refreshed while open, with an animated arrival banner when a platform appears. |
| I5 | *Say hello* / *Chat* silently disabled when a helper's id is malformed (no conversation topic can exist). | **Open (rare)** — needs a one-line explanation instead of a dead button. |
| I6 | No **Devices** screen on Android (design names one: paired phones, last seen, Remove). | **Open** — the dashboard and `mellowmesh devices` cover it meanwhile. |
| I7 | **Welcome is a hard gate**: without a hub there is nothing to try. | **Open** — design direction: a "look around first" sample plaza before pairing. |
| I8 | **Inviting a platform from the phone**: the Spotted list points at the computer's dashboard instead of offering the paste-invitation itself. | **Open** — phone-side invite (share sheet with the pasteable message) is designed, not yet built. |

## Rules this graph keeps

* Every pushed screen has **Back** (`BackHeader`), and Back never loses tab
  state (`goTab` saves/restores).
* The **tap on the shoulder** interrupts politely from anywhere and lands
  exactly where the answer happens (To-do desk or the conversation), never
  on a hub page.
* **Empty states carry the next step** — never prose alone (I1 is the rule's
  origin story).
* A deep link (`mellowmesh-pair:` or a pending route) lands after
  Welcome/MainScaffold decide which world the user is in.
