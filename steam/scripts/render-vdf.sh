#!/usr/bin/env bash
# Render the SteamPipe VDF templates into steam/build/ using environment
# variables. Required: STEAM_APP_ID, STEAM_DEPOT_WINDOWS, STEAM_DEPOT_MACOS.
# Optional: STEAM_BRANCH (default: beta — never "default" from CI),
# BUILD_DESC (default: git describe).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
: "${STEAM_APP_ID:?set STEAM_APP_ID}"
: "${STEAM_DEPOT_WINDOWS:?set STEAM_DEPOT_WINDOWS}"
: "${STEAM_DEPOT_MACOS:?set STEAM_DEPOT_MACOS}"
export STEAM_BRANCH="${STEAM_BRANCH:-beta}"
export BUILD_DESC="${BUILD_DESC:-$(git -C "$here" describe --tags --always 2>/dev/null || echo dev)}"
for v in STEAM_APP_ID STEAM_DEPOT_WINDOWS STEAM_DEPOT_MACOS; do
  [[ "${!v}" =~ ^[0-9]+$ ]] || { echo "$v must be numeric" >&2; exit 1; }
done
[[ "$STEAM_BRANCH" =~ ^[A-Za-z0-9_-]+$ ]] || { echo "invalid STEAM_BRANCH" >&2; exit 1; }
BUILD_DESC="$(printf '%s' "$BUILD_DESC" | tr -cd 'A-Za-z0-9._ -')"
export BUILD_DESC
mkdir -p "$here/build"
for t in app_build depot_windows depot_macos; do
  # Values are validated above (digits / [A-Za-z0-9._ -]), so plain sed is safe.
  sed -e "s|\${STEAM_APP_ID}|$STEAM_APP_ID|g" \
      -e "s|\${STEAM_DEPOT_WINDOWS}|$STEAM_DEPOT_WINDOWS|g" \
      -e "s|\${STEAM_DEPOT_MACOS}|$STEAM_DEPOT_MACOS|g" \
      -e "s|\${STEAM_BRANCH}|$STEAM_BRANCH|g" \
      -e "s|\${BUILD_DESC}|$BUILD_DESC|g" \
      "$here/$t.vdf.template" > "$here/build/$t.vdf"
done
echo "Rendered VDF files into $here/build (branch: $STEAM_BRANCH)"
