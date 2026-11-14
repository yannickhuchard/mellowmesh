# Distributing on Steam

The MellowMesh desktop apps (Windows and macOS, see
[native-apps.md](native-apps.md)) are distributed on Steam as a **Software**
title in the **Productivity** category, alongside the app stores and direct
downloads. Steam gives the desktop apps reach to a large technical audience,
automatic updates, and cloud-free installs on any of the user's PCs and Macs.

Status: **pipeline ready, store page to create.** The build/upload tooling
lives in [`steam/`](../steam); the first Steam build ships with the native
desktop shells.

## One-time setup (Steamworks)

1. Join Steamworks as a developer (identity, tax and bank info) and pay the
   app credit fee for one app.
2. Create the app with type **Application / Software** (not Game). Store
   genres/tags: **Productivity**, Utilities, Software Development.
3. In *SteamPipe → Depots*, create two depots and restrict them by OS:
   * Windows (64-bit) depot
   * macOS depot (universal: Apple silicon + Intel)
4. *Installation → General*: launch options
   * Windows: `MellowMesh.exe` (WinUI 3 app, self-contained Windows App SDK)
   * macOS: `MellowMesh.app` (signed and notarized)
5. Create a **dedicated build account** with only the permissions needed to
   upload builds and set them live on non-default branches. Never use the
   owner account in automation.
6. Store page assets: header capsule, small/main/vertical capsules, library
   capsule, hero and logo, at least five screenshots, a trailer (the motion
   design is the selling point), short and long descriptions. Publish a
   *Coming Soon* page early — Valve requires it to be live for a while before
   release.
7. Submit the store page and the first build for Valve review (a few business
   days each).

## Uploading a build

Stage the signed desktop builds, then upload with SteamPipe:

```bash
# steam/content/windows/  ← MellowMesh.exe + Windows App SDK runtime + mellowmeshd.exe
# steam/content/macos/    ← MellowMesh.app (notarized; includes mellowmeshd)
export STEAM_APP_ID=…  STEAM_DEPOT_WINDOWS=…  STEAM_DEPOT_MACOS=…
export STEAM_USERNAME=<build account>   # login cached by a prior interactive steamcmd session
export STEAM_BRANCH=beta                # builds go live on "beta"; promote to default in Steamworks
steam/scripts/upload.sh
```

`render-vdf.sh` validates the ids and branch name and renders the VDF
templates into `steam/build/`. Builds are set live only on a non-default
branch; promotion to the public `default` branch is a manual step in the
Steamworks partner site after testing.

A CI job can run the same script from a protected environment once the
desktop shells build in CI (store the build account name and its cached
`config.vdf` as environment secrets; require manual approval).

## App behavior when installed from Steam

* **Updates come from Steam.** The app detects a Steam install (the
  `SteamAppId` environment variable set by the Steam client, or the install
  path under `steamapps/`) and disables its own updater.
* **No Steamworks SDK dependency** is required for v1 (no overlay,
  achievements or Steam login). MellowMesh accounts stay independent of
  Steam: the app pairs with the user's hub exactly as on other platforms.
* **Purchases.** The Steam build is free. Before selling anything inside the
  Steam build (e.g. a hosted relay plan), check the Steam Distribution
  Agreement and Steamworks rules on in-app purchases; the simplest compliant
  path is to keep paid plans out of the Steam build or sell them through
  Steam's own mechanisms.
* **Daemon.** The desktop app bundles `mellowmeshd` and starts it on demand,
  bound to `127.0.0.1`, the same as the CLI. Uninstalling through Steam
  removes the app files; the user's data directory (database, tokens) is
  kept unless the user deletes it from the app's settings.

## Checklist per release

- [ ] Windows build signed; macOS build signed + notarized + stapled.
- [ ] `steam/content/` staged from the release artifacts (no debug symbols).
- [ ] Uploaded to `beta`, installed and smoke-tested from Steam on both OSes
      (pairing, notifications, decision approval with Windows Hello / Touch ID).
- [ ] Promoted to `default` in Steamworks; release notes posted as a Steam
      event linking to [CHANGELOG.md](../CHANGELOG.md).
