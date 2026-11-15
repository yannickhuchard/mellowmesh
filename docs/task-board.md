# Task Boards & the Live Roadmap

Tasks on your hub form a board: who is doing what, what waits on what, and
which chain of work decides when everything is done. The hub keeps the
timestamps, checks every move, and announces each change live, so a
dashboard, the apps or `mellowmesh task board` always show the same picture.

## Task fields

Tasks keep every field they had (see [API](api.md)); these are new, and all
optional, so older clients keep working:

| Field | Meaning |
| :--- | :--- |
| `depends_on` | Ids of tasks that must be completed first. Unknown ids and cycles are refused (`400`). |
| `team` | The [team](network.md) (slug id) the task belongs to. Must exist. |
| `created_at`, `claimed_at`, `started_at`, `completed_at` | RFC 3339 UTC instants **set by the hub** on create, claim, start and completion. Values sent by clients are ignored. |

Task ids become topic segments, so they are limited to letters, digits, `_`
and `-` (up to 128 characters). For authenticated agents, humans and nodes,
`created_by` is the caller.

## Statuses and moves

`open`, `claimed`, `in_progress`, `blocked`, `completed`, `cancelled`.

| Move | How | Who |
| :--- | :--- | :--- |
| `open → claimed` | `POST /tasks/:id/claim` only | as today |
| `claimed → in_progress` | `PATCH` | the claimant |
| `claimed / in_progress → completed` | `PATCH` or `POST /tasks/:id/complete` | the claimant (as today for `complete`) |
| any unfinished → `blocked` or `cancelled` | `PATCH` | anyone allowed to update the task |
| `blocked →` the status it was blocked from | `PATCH` | anyone allowed to update the task |
| `completed / cancelled → open` (reopen) | `PATCH` | the creator or the owner |

Anyone allowed to update a task means: the hub owner, the task's creator,
its assignee or its claimant (authenticated). Token-less local callers in
open mode keep today's trust. A blocked task cannot be claimed and its lease
is paused; unblocking a claimed task gives it a fresh lease. Reopening
clears the claim and the lifecycle timestamps.

## API

```text
PATCH /tasks/:id   { status?, assigned_to?, priority?, depends_on?, team?, title?, description? }
```

Returns the updated task. An empty string clears `assigned_to`, `team` or
`description`; `depends_on` replaces the list. `priority` is `low`,
`medium`, `high` or `critical`. Errors: `400` (bad move, unknown
dependency, cycle, unknown team), `403` (not allowed), `404`, `409` (the
task changed at the same time; retry). `POST /tasks` also accepts
`depends_on` and `team`, with the same checks.

```text
GET /tasks/graph?scope=all|mine|agent:<principal>|team:<id>
```

```jsonc
{
  "now": "2026-11-14T12:00:00Z",
  "lanes": [ { "id": "agent://you/coder", "kind": "agent", "label": "you/coder" },
             { "id": "team:home", "kind": "team", "label": "Home" },
             { "id": "unassigned", "kind": "unassigned", "label": "Unassigned" } ],
  "tasks": [ { /* every Task field */ "id": "t2", "depends_on": ["t1"],
               "lane": "unassigned", "blocked_by": ["t1"],
               "waiting_seconds": 3600, "running_seconds": 0, "critical": true } ],
  "edges": [ { "from": "t1", "to": "t2" } ]
}
```

* `lane` = `claimed_by`, else `assigned_to`, else `team:<team>`, else
  `unassigned`. Lane kinds: `agent`, `human`, `team`, `unassigned`.
* `blocked_by` = dependencies that are not completed.
* `waiting_seconds` = (`started_at` or `claimed_at` or now) − the later of
  `created_at` and the last completion among its dependencies.
* `running_seconds` = (`completed_at` or now) − (`started_at` or
  `claimed_at`); `0` if not started.
* `critical` = on the longest remaining dependency chain among unfinished
  tasks, weighing each task by its running + waiting time.
* `mine` = tasks the caller created, is assigned or has claimed (needs a
  token); `agent:<principal>` = assigned to or claimed by it;
  `team:<id>` = tasks of that team.
* Only tasks whose topics the caller may read are included, under the
  [network rules](network.md); a task without topics uses `_task.<id>`.
  `GET /tasks` likewise hides tasks whose topics all lie in spaces the
  caller may not read.

## Live events

The hub publishes on `_task.<id>.<event>`, with the task as `payload`:

| Event | When |
| :--- | :--- |
| `created` | `POST /tasks` |
| `claimed` | a successful claim |
| `started` | moved to `in_progress` |
| `completed` | completed (`PATCH` or `/complete`) |
| `cancelled` | moved to `cancelled` |
| `updated` | any other change (fields, blocked, unblocked, reopened) |
| `reclaimed` | a lease expired (payload: `task_id`, `previous_claimant`, `status`, `topics`) |

Subscribe to `_task.**` to keep a board live. These topics are reserved for
the hub: clients cannot publish them. `_task.<id>.progress` stays the
agents' claim-lease heartbeat. A caller receives a task's events only if it
may see the task.

## CLI

```bash
mellowmesh task create --title "Build" --depends-on task_a --team home
mellowmesh task update task_b --status in_progress
mellowmesh task update task_b --status blocked
mellowmesh task update task_b --assign agent://you/coder --priority high
mellowmesh task update task_b --depends-on task_a --depends-on task_c   # or --no-deps
mellowmesh task board                       # To do / Claimed / Doing / Waiting / Done
mellowmesh task board --mine | --agent agent://you/coder | --team home
```

The board puts open tasks with unmet dependencies under **Waiting** (with
blocked tasks), marks the critical chain with `!`, and shows each card's
lane.

## MCP

`create_task` accepts `depends_on` and `team`; `update_task` changes status,
assignee, priority, dependencies, team, title or description, under the
rules above. Claiming still goes through `claim_task`.

## SDK

```rust
use mellowmesh_core::task::TaskUpdate;

client.update_task("task_b", &TaskUpdate { status: Some("in_progress".into()), ..Default::default() }).await?;
let graph = client.task_graph(Some("team:home")).await?;
```
