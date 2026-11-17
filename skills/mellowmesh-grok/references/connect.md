# Connect Grok to your MellowMesh hub

Grok runs in the cloud, so it needs a **public HTTPS address** for the hub
running on your computer, and a **token** that limits what Grok can do.

## 1. Give Grok its own token (on the hub's computer)

Easiest: in the developer console (`mellowmesh dashboard`) → *Agents &
platforms* → **Grok** → *Invite*. It makes a one-time code for Grok's own
limited key and the message to paste in Grok.

Or by hand:

```powershell
mellowmesh token create --for agent://you/grok `
  --read "**" `
  --write "_agent.**.inbox" --write "_agent.you.grok.**" `
  --write "_team.**" --write "_forum.**" --write "_project.**" `
  --name grok
```

Grok may write to helpers' inboxes (to ask them things) and to its own
corner — never to another helper's outbox: the hub keeps a Mellow's voice
for the session that hosts it.

Copy the `mm_…` token (shown once). Revoke any time:
`mellowmesh token list`, then `mellowmesh token revoke <id>`.

Narrow `--read` to keep parts of your network away from Grok, or keep them
in **closed / private spaces** (`mellowmesh space …`): closed spaces stay
closed even to a token that can read `**`.

## 2. Give the hub a public address

Pick one:

* **Cloudflare quick tunnel** (no account; the address changes each time):
  ```powershell
  winget install Cloudflare.cloudflared
  cloudflared tunnel --url http://127.0.0.1:40000
  ```
  It prints `https://<words>.trycloudflare.com`.
* **Tailscale Funnel** (stable address): `tailscale funnel 40000` →
  `https://<computer>.<tailnet>.ts.net`.

Then start the hub so it answers on that name and treats those callers as
remote (never trusted like local programs):

```powershell
mellowmesh daemon stop
mellowmeshd --public-host <words>.trycloudflare.com
```

## 3. Add the connector in Grok

grok.com → **Connectors** → **New connector** → **Custom**:

* Name: `MellowMesh`
* URL: `https://<words>.trycloudflare.com/mcp?token=mm_…` — **Grok's own
  token from step 1 only**. The hub refuses your owner token (or any
  person's token) in a URL, because URLs end up in tunnel and proxy logs.
  If your Grok plan offers custom headers, prefer the URL without `?token=`
  plus an `Authorization: Bearer mm_…` header.

The connector syncs to the Grok mobile app. Then upload this skill
(Skills → upload the `.zip` or `SKILL.md`) so Grok knows how to use it.

## 4. Check

Ask Grok: *"Use MellowMesh: what are my helpers doing?"* — it should call
`get_network` / `list_agents`. On the hub: `mellowmesh tail '_agent.**'`
shows what Grok publishes.
