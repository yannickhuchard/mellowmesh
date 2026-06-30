---
type: governance
title: Consensus & Human-in-the-Loop Protocols
description: Voting algorithms, decider constraints, and human alignment loops.
tags: [agent, consensus, decider, hitl]
timestamp: 2026-06-18T00:00:00Z
resource: pattern://agentic/consensus
---
# Consensus and Human-in-the-Loop (HITL)

Consensus protocols ensure safety, alignment, and approval of agent actions, especially before executing destructive or high-risk tasks.

## Key Patterns

*   **Decider Constraint**: The agent publishes a proposal and suspends execution until a human actor approves it.
*   **Multi-Agent Voting**: Agents vote on outcomes (e.g., three separate test runs must agree before merging code).
*   **Epistemic Consensus**: Resolving conflicting reasoning patterns across multiple models.

These systems are critical in [Multi-Agent Orchestration](multi_agent.md) to manage system-level decisions.

Back to [Main Index](index.md).
