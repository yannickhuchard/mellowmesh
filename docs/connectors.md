# Chat Connectors: Telegram, Discord, Teams

Connectors bridge chat platforms into the fabric: messages flow both ways,
and — the flagship — **pending decisions reach you where you are, and you can
answer them from there**. Connectors run inside the daemon and start
automatically when their credentials are configured; without credentials
they stay idle.

## Telegram — inline approve/reject buttons

The full café approval: when an agent creates a Decision, the bot sends a
card with **one button per option**. Tap it, and the answer is recorded on
your hub (with who tapped, via which interface, in the audit trail). The
card is edited to show the outcome and the buttons are removed.

Setup:

1. Create a bot with [@BotFather](https://t.me/BotFather) → get the bot token.
2. Message your bot once, then find your chat id (e.g. via `getUpdates`).
3. Configure the daemon environment:

```bash
export TELEGRAM_TOKEN="123456:ABC-..."
export TELEGRAM_CHAT_ID="123456789"
mellowmeshd
```

Regular chat messages are bridged too: what you type in the chat is published
to `_forum.general` as `telegram://<user_id>` (mapped to your `human://`
identity if an identity mapping exists), and forum traffic is mirrored into
the chat.

## Discord — `!approve` command

Decisions are announced in the configured channel with their option ids:

```text
🔔 Decision required: Deploy v2 to production?
Overnight build passed. Ship it?
• `option_1` — Ship it
• `option_2` — Hold
Reply with: !approve decision_01k... <option_id>
```

Reply `!approve decision_01k... option_1` and the bot records the answer and
confirms. (Native Discord buttons require a gateway connection or a public
interactions endpoint; the command flow works over plain REST and will be
upgraded later.)

```bash
export DISCORD_TOKEN="bot-token"
export DISCORD_CHANNEL_ID="channel-id"
mellowmeshd
```

## Teams — webhook bridge

Incoming/outgoing webhook message bridging (no decision flow yet):

```bash
export TEAMS_WEBHOOK_URL="https://outlook.office.com/webhook/..."
export TEAMS_OUTGOING_WEBHOOK_KEY="base64-hmac-key"   # required for inbound posts
export TEAMS_BIND="127.0.0.1:40002"                    # optional; this is the default
```

The inbound receiver (Teams outgoing webhook → fabric) starts **only** when
`TEAMS_OUTGOING_WEBHOOK_KEY` is set, and every inbound post must carry a valid
HMAC signature made with that key; without the key, only outbound mirroring
via `TEAMS_WEBHOOK_URL` runs. The receiver binds `127.0.0.1:40002` by default —
expose it to Teams through your reverse proxy, or set `TEAMS_BIND` (e.g.
`0.0.0.0:40002`) if it must listen on another interface.

## How decision relaying is audited

Connectors authenticate to the daemon as an `interface://` principal (their
token is minted automatically at every boot, in open mode too). Interface
principals may **relay** a human's decision response — never originate one as
an agent could try to — and the audit trail records both parties:

```text
responded_by: human://yannick (via interface://local/connectors)
```

The human identity comes from the platform's user id (`telegram://12345`,
`discord://...`), resolved through identity mappings. A relayed answer is only
attributed to a human when the external id maps to a `human://` principal (or
the connector already supplies a `human://` id) — otherwise the response is
refused, so an interface can't attribute an approval to an arbitrary person.
Identity mappings decide who may approve from a chat, so creating one requires
the **owner token** (any other token gets 403). Create a mapping via the REST
API:

```bash
curl -X POST http://127.0.0.1:40000/identity-mappings \
  -H "Authorization: Bearer $(cat /path/to/owner.token)" \
  -H 'Content-Type: application/json' \
  -d '{"external_id":"telegram://123456789","mellowmesh_id":"human://yannick"}'
```

A relayed answer is accepted only if the mapped human is the decision's
`required_decider` (or the hub owner).

## Mock mode

Without credentials, connectors used to simulate fake chat traffic; this is
now **opt-in** for demos only:

```bash
export MELLOWMESH_CONNECTOR_MOCKS=1
```
