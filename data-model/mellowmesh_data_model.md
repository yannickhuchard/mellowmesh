# MellowMesh Data Models

This directory contains definitions and guidance for MellowMesh system schemas to ensure alignment across control, data, and memory planes.

---

## 1. Topic

Topics categorize messages. In MellowMesh, topics are case-insensitive, dot-separated lowercase names, using the regex: `^[a-z0-9._-]+$`.

### Topic Namespaces
* `_system.presence.**` - ephemeral heartbeats and node/agent status.
* `_agent.*.heartbeat` - periodic presence markers.
* `_agent.*.stream.**` - real-time token streaming / debug.
* `_trace.**` - cognitive telemetry, verbose execution logs, and raw debug streams.
* `_task.**` - task lifecycle events and updates.
* `_flow.**` - coordinator state machine transition logs.
* `_decision.**` - human/agent coordination requests and approvals.
* `_artifact.**` - generated files, text packages, or summaries.

---

## 2. Message

Messages are the primary unit of exchange.

```json
{
  "id": "msg_01hxa...",
  "topic": "topic.name",
  "from": "agent://agent-id" | "human://human-id",
  "owner": "human://human-owner-id",
  "timestamp": "ISO8601 string",
  "content_type": "text/plain" | "application/json" | "text/markdown",
  "body": "Raw payload string",
  "headers": {
    "correlation_id": "...",
    "conversation_id": "..."
  },
  "payload": {}
}
```

---

## 3. Agent registration & Presence

### AgentRegistration
```json
{
  "id": "agent://yannick/codex",
  "name": "codex",
  "owner": "human://yannick",
  "mode": "human-piloted" | "autonomous",
  "capabilities": ["code.write", "code.review"]
}
```

### AgentPresence
```json
{
  "agent_id": "agent://yannick/codex",
  "status": "available" | "busy" | "offline",
  "mode": "autonomous",
  "capabilities": ["code.write"],
  "current_work": ["task_123"],
  "last_seen": "ISO8601 string"
}
```

---

## 4. Task

Represents a unit of work assigned to or claimed by an agent or human.

```json
{
  "id": "task_01hy...",
  "title": "Build UI Component",
  "description": "Create modern cards...",
  "created_from": "msg_01hxa...",
  "created_by": "human://yannick",
  "status": "open" | "claimed" | "in_progress" | "completed" | "cancelled",
  "priority": "low" | "medium" | "high",
  "topics": ["_project.mellowmesh.ui"],
  "required_capabilities": ["ui.design"],
  "assigned_to": "agent://yannick/designer",
  "claimed_by": "agent://yannick/designer",
  "deadline": "ISO8601 string",
  "artifacts": ["artifact_123"],
  "decisions": ["decision_456"]
}
```

---

## 5. Decision & DecisionOption

Facilitates interactive consensus between human and agent.

```json
{
  "id": "decision_01hz...",
  "title": "Select DB Technology",
  "question": "Which database engine should we use for storage?",
  "created_by": "agent://yannick/codex",
  "required_decider": "human://yannick",
  "status": "requested" | "discussed" | "approved" | "rejected" | "deferred",
  "options": [
    {
      "id": "opt_1",
      "label": "SQLite",
      "pros": ["Zero config", "File-based"],
      "cons": ["Concurrency limits"]
    }
  ],
  "response_option_id": "opt_1",
  "response_timestamp": "ISO8601 string"
}
```

---

## 6. TraceSession

Configures real-time telemetry extraction.

```json
{
  "id": "trace_01hzabc",
  "target_type": "agent" | "task" | "flow" | "topic" | "connector" | "node",
  "target": "agent://yannick/codex",
  "level": "off" | "status" | "progress" | "structured" | "verbose" | "cognitive" | "raw",
  "enabled_by": "human://yannick",
  "reason": "Debugging stuck task",
  "started_at": "ISO8601 string",
  "expires_at": "ISO8601 string",
  "persistence_mode": "ephemeral" | "metadata" | "event_log",
  "retention": "30m",
  "max_messages_per_second": 10,
  "max_bytes_per_second": 65536,
  "topics": ["_trace.agent.codex.cognitive"],
  "status": "active"
}
```

---

## 7. PersistencePolicy

Maps topics to async durability guidelines.

```json
{
  "mode": "ephemeral" | "metadata" | "event_log" | "queryable",
  "retention": "7d",
  "max_message_size": "4KB",
  "sync": false
}
```
