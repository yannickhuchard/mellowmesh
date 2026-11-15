#!/usr/bin/env bash
# Upload staged content (steam/content/{windows,macos}) with SteamPipe.
# Requires steamcmd on PATH, STEAM_USERNAME, and a cached Steam Guard login
# (config.vdf) for that build account. Use a dedicated build account with
# only the "Edit App Metadata" + "Publish App Changes To Steam" permissions.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
: "${STEAM_USERNAME:?set STEAM_USERNAME}"
for d in windows macos; do
  [ -n "$(ls -A "$here/content/$d" 2>/dev/null)" ] || { echo "content/$d is empty" >&2; exit 1; }
done
"$here/scripts/render-vdf.sh"
mkdir -p "$here/output"
steamcmd +login "$STEAM_USERNAME" +run_app_build "$here/build/app_build.vdf" +quit
