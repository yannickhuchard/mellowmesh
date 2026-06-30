---
type: pattern
title: Multi-Agent Orchestration
description: Protocols for collaboration, supervisor patterns, and task handoffs.
tags: [agent, multi-agent, supervisor, collaboration]
timestamp: 2026-06-18T00:00:00Z
---
# Multi-Agent Orchestration

Multi-Agent Systems (MAS) break down large, monolithic tasks into specialized roles (e.g. coder, tester, reviewer).

## Collaboration Patterns

*   **Supervisor-Worker**: A central controller schedules, assigns, and aggregates task results.
*   **Choreographed Handoff**: Agents pass task tokens directly to one another based on state (e.g., Coder -> Tester -> Reviewer).
*   **Shared Blackboard**: Agents write proposals and review updates on a shared fabric.

Orchestration models rely on task pipelines and consensus protocols:
- Learn about [Consensus Protocols](consensus.md).
- Integration utilizes planning loops ([Reasoning & Planning](planning.md)) and memory persistence ([Context & Memory Systems](memory.md)).

Back to [Main Index](index.md).
