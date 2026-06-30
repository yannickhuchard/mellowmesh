---
type: pattern
title: Reasoning & Planning Patterns
description: Core planning loops including ReAct, Chain-of-Thought, and Tree of Thoughts.
tags: [agent, planning, react, reasoning]
timestamp: 2026-06-18T00:00:00Z
---
# Reasoning and Planning

Planning enables an agent to break down a high-level task into actionable sub-goals.

## Leading Planning Frameworks

*   **Chain-of-Thought (CoT)**: Prompting the model to generate intermediate reasoning steps before answering.
*   **ReAct (Reasoning + Acting)**: Alternating between thought steps ("Thought") and action steps ("Action" / "Observation"). Used for tool invocation.
*   **Tree of Thoughts (ToT)**: Exploring multiple reasoning pathways simultaneously, using the LLM to evaluate candidates and backtracking when necessary.

Planning often relies on [Context & Memory](memory.md) to track state, and is coordinated in complex workflows via [Multi-Agent Orchestration](multi_agent.md).

Back to [Main Index](index.md).
