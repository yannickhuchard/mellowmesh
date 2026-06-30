---
type: pattern
title: Context & Memory Systems
description: Short-term context management, episodic logs, and long-term vector-based memory.
tags: [agent, memory, vector-db, episodic]
timestamp: 2026-06-18T00:00:00Z
---
# Context and Memory Systems

Memory allows agents to persist information across multiple turns, interactions, or task lifecycles.

## Dimensions of Agent Memory

1. **Short-Term Context**: The active conversation window. Limited by the LLM context size.
2. **Episodic Memory**: Detailed history of past task runs and tool execution steps.
3. **Long-Term Semantic Memory**: Embeddings stored in vector databases containing facts and guidelines. This is heavily integrated with [Retrieval-Augmented Generation](rag.md).

For complex task executions requiring planning loops ([Reasoning & Planning](planning.md)), memory is vital to prevent agents from looping infinitely.

Back to [Main Index](index.md).
