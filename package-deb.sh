#!/bin/sh
set -e

# Pinned packager version. Keep in sync with the release workflow and the
# other package-* scripts.
PACKAGER_VERSION="${CARGO_PACKAGER_VERSION:-0.11.8}"

# 1. Check and install the pinned cargo-packager
if cargo packager --version 2>/dev/null | grep -qF "$PACKAGER_VERSION"; then
    echo "cargo-packager $PACKAGER_VERSION is already installed."
else
    echo "Installing cargo-packager $PACKAGER_VERSION..."
    cargo install cargo-packager --locked --version "$PACKAGER_VERSION"
fi

# 2. Build release binaries (exactly the dependency versions in Cargo.lock)
echo "Building MellowMesh workspace in release mode..."
cargo build --release --workspace --locked

# 3. Package as .deb installer
echo "Packaging MellowMesh into .deb installer..."
cargo packager --release -p mellowmesh-cli --formats deb

echo "Debian packaging completed successfully!"
