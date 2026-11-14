# Connect Grok to your MellowMesh hub

Grok runs in the cloud, so it needs a **public HTTPS address** for the hub
running on your computer, and a **token** that limits what Grok can do.

## 1. Give Grok its own token (on the hub's computer)

```powershell
mellowmesh token create --for agent://you/grok `
  --read "**" `
  --write "_agent.**" --write "_team.**" --write "_forum.**" --write "_project.**" `
  --name grok
```

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
* URL: `https://<words>.trycloudflare.com/mcp?token=mm_…`
  (or the URL without `?token=` plus an `Authorization: Bearer mm_…` header
  if your Grok plan offers custom headers)

The connector syncs to the Grok mobile app. Then upload this skill
(Skills → upload the `.zip` or `SKILL.md`) so Grok knows how to use it.

## 4. Check

Ask Grok: *"Use MellowMesh: what are my helpers doing?"* — it should call
`get_network` / `list_agents`. On the hub: `mellowmesh tail '_agent.**'`
shows what Grok publishes.
