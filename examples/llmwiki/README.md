# LLMWiki Namespaces & Operations Example

This example demonstrates how to set up, synchronize, and query multiple isolated **LLMWiki** knowledge bases based on the **Open Knowledge Format (OKF)** using the MellowMesh fabric.

---

## 📂 Pre-populated Knowledge Graphs

This folder contains three distinct OKF-structured knowledge directories:

1.  **Quantum Particles (`./quantum`)**:
    *   Fermions, Bosons, Higgs, Quarks, Leptons.
    *   Highly interconnected graph mapping physical classifications.
2.  **Agentic Architecture & Patterns (`./agents`)**:
    *   Reasoning & Planning (ReAct, CoT), Memory Systems, RAG, Multi-Agent systems, and Human-in-the-Loop Consensus.
3.  **One Piece Devil Fruits (`./onepiece`)**:
    *   Paramecia, Zoan, Logia, and the legendary Hito Hito no Mi Model: Nika.
    *   Demonstrates cross-references and historical renaming relationships.

---

## 🚀 How to Run the Demo

We provide automated scripts to run the demo. They will:
1.  Configure the `MELLOWMESH_WIKIS` variable to load all three directories.
2.  Start the MellowMesh daemon `mellowmeshd` in the background.
3.  Execute initial sync endpoints for each namespace.
4.  Run various search, list, and view commands to show the CLI interface.
5.  Clean up and stop the daemon.

### Windows (PowerShell)
```powershell
.\run_demo.ps1
```

### Linux / macOS (Bash)
```bash
chmod +x run_demo.sh
./run_demo.sh
```

---

## 🛠️ Step-by-Step Manual Operations

If you want to run operations manually, follow these commands:

### 1. Launch the Daemon with Namespaces
Start the coordinator daemon, passing the directories mapped to their respective namespace names:
```bash
# Windows (PowerShell)
$env:MELLOWMESH_WIKIS="quantum:./quantum,agents:./agents,onepiece:./onepiece"
mellowmeshd

# Linux / macOS
export MELLOWMESH_WIKIS="quantum:./quantum,agents:./agents,onepiece:./onepiece"
mellowmeshd
```

### 2. Synchronize Filesystem to Database
In another terminal, force synchronization to index the Markdown files and build the link graph:
```bash
# Sync all three wikis
mellowmesh wiki sync --wiki quantum
mellowmesh wiki sync --wiki agents
mellowmesh wiki sync --wiki onepiece
```

### 3. Query the Wikis

#### A. List Pages
List all documents loaded in a specific wiki namespace:
```bash
mellowmesh wiki list --wiki quantum
mellowmesh wiki list --wiki agents
```

#### B. Full-Text Search (FTS5)
Search for terms inside titles or body contents. Since namespaces are isolated, searching in one won't return results from others:
```bash
# Matches Nika and Gomu Gomu fruits in the onepiece wiki
mellowmesh wiki search "Nika" --wiki onepiece

# Matches planning patterns in the agents wiki
mellowmesh wiki search "Thought" --wiki agents
```

#### C. View Document & Graph Link Details
View a specific document. MellowMesh displays the YAML metadata headers, the rendered markdown body, and all parsed outgoing link paths:
```bash
mellowmesh wiki view nika.md --wiki onepiece
```

---

## 🌐 Web Console Visualizer

Open the Web Console Dashboard at `http://127.0.0.1:40000/ui` while the daemon is running.
1.  Navigate to the **LLMWiki** tab.
2.  Select a namespace (e.g., `onepiece`, `quantum`, or `agents`) from the dropdown.
3.  Click on any page in the list to view its contents and render its **interactive force-directed connection graph** in the canvas below! You can drag nodes and click on them to navigate the wiki.
