---
type: pattern
title: Retrieval-Augmented Generation (RAG)
description: Methods for searching, chunking, and injecting external knowledge.
tags: [agent, rag, embedding, chunking]
timestamp: 2026-06-18T00:00:00Z
---
# Retrieval-Augmented Generation (RAG)

RAG connects LLMs to custom external databases, eliminating the need to fine-tune models to inject domain facts.

## Processing Pipeline

1. **Ingestion**: Split documents into chunks.
2. **Embedding**: Generate vector representations for each chunk.
3. **Retrieval**: Perform cosine similarity searches based on user queries.
4. **Synthesis**: Combine the retrieved text with the prompt to generate responses.

Advanced agents dynamically update their RAG databases using write actions, storing their experience in [Context & Memory Systems](memory.md).

Back to [Main Index](index.md).
