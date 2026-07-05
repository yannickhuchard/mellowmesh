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
export TEAMS_OUTGOING_WEBHOOK_KEY="base64-hmac-key"   # verifies inbound posts
```

## How decision relaying is audited

Connectors authenticate to the daemon as an `interface://` principal (their
token is minted automatically at boot under `--require-auth`). Interface
principals may **relay** a human's decision response — never originate one as
an agent could try to — and the audit trail records both parties:

```text
responded_by: human://yannick (via interface://local/connectors)
```

The human identity comes from the platform's user id (`telegram://12345`,
`discord://...`), resolved through identity mappings when configured:

```bash
mellowmesh identity add telegram://123456789 human://yannick   # via REST: POST /identity-mappings
```

## Mock mode

Without credentials, connectors used to simulate fake chat traffic; this is
now **opt-in** for demos only:

```bash
export MELLOWMESH_CONNECTOR_MOCKS=1
```
