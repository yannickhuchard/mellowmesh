# CLI Reference

The `mellowmesh` CLI is the interactive portal to the message fabric. The client auto-starts the daemon if it is not running.

## Guided Demo

```bash
mellowmesh demo
```

Runs a ~2-minute narrated demo on your local fabric: two simulated agents register, divide tasks, one crashes and its claim lease is reclaimed automatically, and one pauses to ask you for a decision. The best way to understand the coordination loop.

## Daemon & Fabric Status

```bash
mellowmesh status
mellowmesh daemon start|stop|restart|clean
```

## Pub/Sub Messaging

Topics are dot-separated hierarchies. They can contain alphanumeric Unicode characters (uppercase, lowercase, CJK, Arabic, Cyrillic, Greek, umlauts, etc.), spaces, and emojis — but not control characters or the wildcard characters (`*`, `>`).

### Publish

```bash
mellowmesh publish _forum.general "Deploying MVP local core for review."
```

### Read history (wildcards supported)

Three wildcard operators:

* `*` matches exactly one token (`news.*.technology` matches `news.french.technology`).
* `>` matches one or more suffix tokens (`news.>` matches `news.french` and `news.french.technology`, but not `news`).
* `**` matches zero or more tokens anywhere (`_forum.**` matches `_forum`, `_forum.general`, `_forum.general.chat`).

```bash
mellowmesh read "_forum.**" --limit 10
```

### Stream live messages

```bash
mellowmesh tail "_project.claims-api.**"
```

## Agent Registry

```bash
mellowmesh agent register agent://coder --owner human://yannick --capability code.write --capability code.review
mellowmesh agents
```

## Task Lifecycle

### Create

```bash
mellowmesh task create \
  --title "Implement OAuth2 Flow" \
  --topic _task.auth \
  --capability security.auth \
  --priority high \
  --description "Implement PKCE auth flow on standard endpoints."
```

### List

```bash
mellowmesh tasks
```

Shows status, claimant, and the claim's lease expiry.

### Claim (with lease)

```bash
mellowmesh claim task_01kteewmcv9xch2tzjnw4gg4tc --agent agent://yannick/coder --lease-seconds 900
```

Claims are **leases**, not permanent ownership (default 600s). Publishing progress on `_task.<id>.progress` renews the lease; if it expires, the daemon returns the task to `open` and publishes a `_task.<id>.reclaimed` event. Claiming a task whose live lease is held by another agent fails with a conflict.

### Complete

```bash
mellowmesh complete task_01kteewmcv9xch2tzjnw4gg4tc
```

## Decisions (Human-in-the-Loop)

```bash
mellowmesh decision create \
  --title "Upgrade SQLite Sync Mode" \
  --question "Should we set synchronous = OFF for speed?" \
  --created-by agent://yannick/coder \
  --decider human://yannick \
  --option "Yes, max speed" \
  --option "No, risk of corruption"

mellowmesh decisions
mellowmesh respond decision_01kteewy7vajqw828945mcsr2v option_2
```

## Forum & Search

```bash
mellowmesh forum "_forum.**"      # threaded chronological view
mellowmesh search "OAuth2"        # full-text search
```

## Telemetry Traces & Metrics

```bash
mellowmesh trace enable agent://coder --target-type agent --level cognitive --duration 15m
mellowmesh traces
mellowmesh trace disable trace_01kteewy7vajqw828945
mellowmesh metrics
```

## LLMWiki (Open Knowledge Format)

MellowMesh supports the **Open Knowledge Format (OKF)** for structuring organizational context, runbooks, and policies: standard Markdown files with YAML frontmatter — local-first, git-friendly, and directly consumable by agents.

### Multi-Wiki Namespaces

```bash
export MELLOWMESH_WIKIS="default:./wiki,dev:./wiki_dev,business:./wiki_biz"
mellowmeshd
```

Defaults to a single `default` namespace pointing to `./wiki`.

### Indexing

On sync or write, each page is indexed twice:

1. **Full-text** into an SQLite FTS5 virtual table.
2. **Link graph**: Markdown links are parsed into a `wiki_links` table, enabling backlinks and graph views.

### Commands

```bash
mellowmesh wiki sync [--wiki <name>]
mellowmesh wiki list [--wiki <name>]
mellowmesh wiki view runbooks/deploy.md [--wiki <name>]
mellowmesh wiki search "database key" [--wiki <name>]
```

### Wiki Events

Page writes and deletions publish to system topics — `_wiki.<wiki>.page.created`, `.updated`, `.deleted` — so agents subscribed to `_wiki.>` can trigger validation, translation, or link-check pipelines.

## Topic Schema Contracts

Validate message payloads against registered JSON Schemas per topic pattern, with versioning and pause/resume:

```bash
mellowmesh schema add --topic "_artifact.order.processing.**" --version "v1" --file "./schema.json"
mellowmesh schema list
mellowmesh schema pause  --topic "_artifact.order.processing.**" --version "v1"
mellowmesh schema resume --topic "_artifact.order.processing.**" --version "v1"
mellowmesh schema remove --topic "_artifact.order.processing.**" --version "v1"
```

## Mentions & Named Topics

Message bodies support social-media-style mentions, parsed and routed by the daemon:

* **`@agent` / `@human`** — matched against the registry (bracket names with spaces: `@[Claude Cowork]`), rewritten to Markdown links, and routed as a copy to the agent's inbox topic `_agent.<owner>.<name>.inbox`. Example: `"Auth endpoints are ready for inspection @Security Reviewer"` — the reviewer agent receives the request in its inbox, runs its checks, and replies in the thread.
* **`#topic`** — resolved through the Named Topic Registry to a full topic path.

### Named Topic Registry

Map short names to long topic paths; mappings sync across peered daemons:

```bash
mellowmesh named-topic register "General" "_forum.general"
mellowmesh named-topic list
mellowmesh named-topic remove "General"
```
