#!/bin/bash

# Save original directory
ORIG_DIR=$(pwd)

# Set location to script directory
cd "$(dirname "$0")"

DAEMON_PATH="../../target/debug/mellowmeshd"
CLI_PATH="../../target/debug/mellowmesh"

# Check if daemon binary exists
if [ ! -f "$DAEMON_PATH" ] && [ ! -f "${DAEMON_PATH}.exe" ]; then
    echo "Error: Daemon binary not found at $DAEMON_PATH. Please run 'cargo build' in the root first."
    cd "$ORIG_DIR"
    exit 1
fi

if [ -f "${DAEMON_PATH}.exe" ]; then
    DAEMON_PATH="${DAEMON_PATH}.exe"
    CLI_PATH="${CLI_PATH}.exe"
fi

echo "Starting MellowMesh Daemon in the background..."
export MELLOWMESH_WIKIS="quantum:./quantum,agents:./agents,onepiece:./onepiece"

# Start daemon
"$DAEMON_PATH" > /dev/null 2>&1 &
DAEMON_PID=$!

# Give it 2 seconds to start up
sleep 2

# Check if running
if ! kill -0 $DAEMON_PID > /dev/null 2>&1; then
    echo "Error: Daemon failed to start."
    cd "$ORIG_DIR"
    exit 1
fi

echo "Daemon running (PID: $DAEMON_PID)"
echo ""

# Run operations
echo "--- 1. Synchronizing Wiki Namespaces ---"
"$CLI_PATH" wiki sync --wiki quantum
"$CLI_PATH" wiki sync --wiki agents
"$CLI_PATH" wiki sync --wiki onepiece
echo ""

echo "--- 2. Listing Pages in 'quantum' Wiki ---"
"$CLI_PATH" wiki list --wiki quantum
echo ""

echo "--- 3. Searching for 'Nika' in 'onepiece' Wiki ---"
"$CLI_PATH" wiki search "Nika" --wiki onepiece
echo ""

echo "--- 4. Viewing 'planning.md' in 'agents' Wiki ---"
"$CLI_PATH" wiki view planning.md --wiki agents
echo ""

echo "Stopping daemon..."
kill $DAEMON_PID

cd "$ORIG_DIR"
echo "Demo finished!"
