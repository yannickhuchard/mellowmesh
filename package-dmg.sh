#!/bin/sh
set -e

# 1. Check and install cargo-packager
if ! command -v cargo-packager >/dev/null 2>&1; then
    echo "cargo-packager is not installed. Installing..."
    cargo install cargo-packager
else
    echo "cargo-packager is already installed."
fi

# 2. Build release binaries
echo "Building MellowMesh workspace in release mode..."
cargo build --release --workspace

# 3. Package as .dmg installer
echo "Packaging MellowMesh into .dmg installer..."
cargo packager --release -p mellowmesh-cli --formats dmg

echo "macOS packaging completed successfully!"
