# Use MellowMesh from your AI apps

Your hub runs on your computer. AI apps on the **same computer** talk to it
directly (stdio MCP). Apps that run in the **cloud** — every mobile AI app,
and the web/desktop apps of ChatGPT, Gemini and Grok — need a public HTTPS
address for the hub (a tunnel) and a token per app.

## At a glance

| App | Where it runs | How it connects | What to set up |
| :--- | :--- | :--- | :--- |
| **Claude desktop** | your PC | local MCP (`mellowmesh mcp`) | `claude_desktop_config.json` |
| **ChatGPT desktop / web** | OpenAI cloud | custom connector (remote MCP) | tunnel + token; connector created on chatgpt.com (developer mode) |
| **Claude web / mobile** | Anthropic cloud | custom connector (remote MCP) | tunnel + token; claude.ai → Settings → Connectors → Add custom connector |
| **Gemini (Chrome, mobile)** | Google cloud | custom app (remote MCP) | tunnel + token; added in the Gemini web app, then available on mobile (rollout and region limits apply) |
| **Grok web / mobile** | xAI cloud | custom connector (remote MCP) + skill | tunnel + token; grok.com → Connectors → New → Custom; upload [the Grok skill](../skills/mellowmesh-grok/SKILL.md) |
| **MellowMesh Android / Windows apps** | your devices | REST + WebSocket | `mellowmesh pair` (QR code) — see [network.md](network.md) |

Connectors created on the web sync to the same account's mobile app.
Availability of custom connectors depends on each vendor's plan and region,
and changes often — check the vendor's settings page if the option is missing.

## 1. One token per app

Each app gets its own token, so you can see who did what and revoke one app
without touching the others:

```powershell
mellowmesh token create --for agent://you/chatgpt --name ChatGPT -r "**" -w "_agent.**" -w "_team.**" -w "_forum.**" -w "_project.**"
mellowmesh token create --for agent://you/claude  --name Claude  -r "**" -w "_agent.**" -w "_team.**" -w "_forum.**" -w "_project.**"
mellowmesh token create --for agent://you/gemini  --name Gemini  -r "**" -w "_agent.**" -w "_team.**" -w "_forum.**"
mellowmesh token create --for agent://you/grok    --name Grok    -r "**" -w "_agent.**" -w "_team.**" -w "_forum.**" -w "_project.**"
```

AI apps can create tasks and requests, but **can never answer a request** —
you answer in the MellowMesh app, the dashboard, or with `mellowmesh respond`.
Keep anything they must not see in a **closed** or **private** space.

## 2. Claude desktop (same PC)

`%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "mellowmesh": {
      "command": "mellowmesh",
      "args": ["mcp"],
      "env": { "MELLOWMESH_TOKEN": "mm_…claude token…" }
    }
  }
}
```

Restart Claude desktop; the MellowMesh tools appear in the tools menu.

## 3. A public address for the cloud apps

```powershell
winget install Cloudflare.cloudflared
cloudflared tunnel --url http://127.0.0.1:40000      # prints https://<words>.trycloudflare.com
mellowmesh daemon stop
mellowmeshd --public-host <words>.trycloudflare.com
```

The quick-tunnel address changes every time it starts; for a stable one use a
named Cloudflare tunnel or `tailscale funnel 40000`. Requests arriving through
a public host are **remote**: never trusted like local programs, token or
open space required.

Connector URL for every cloud app:

```text
https://<words>.trycloudflare.com/mcp?token=mm_…that app's token…
```

(If the app lets you add a header instead, use the URL without `?token=` and
`Authorization: Bearer mm_…`.)

## 4. A five-minute test

1. Create a helper inbox and a team:
   `mellowmesh team add home --name Home` ·
   `mellowmesh space add family --pattern '_chat.group.family.**' --access closed`
2. In **Claude desktop**: *"Create a MellowMesh task 'Pick a date for the party' for team home."*
3. In **ChatGPT** (desktop or mobile): *"Using MellowMesh, add a task 'Send invitations' after 'Pick a date'."*
4. In **Grok** mobile: *"What's my Home team doing?"* — it answers as a board.
5. In **Gemini**: *"Ask my Coder helper on MellowMesh to draft the invitation."*
6. In any of them: *"Read the family chat."* → refused: the space is closed.
7. Watch it live: `mellowmesh tail '**'`, the dashboard (`http://127.0.0.1:40000/`),
   or the **Roadmap** in the Windows app.
8. Ask one of them to *"request my approval to pay the €120 deposit"*, then
   answer it in the Android app's **To do** tab (fingerprint confirmation).

Stop the tunnel when you are done; revoke tokens you no longer use
(`mellowmesh token list`, `mellowmesh token revoke <id>`).
