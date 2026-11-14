# Installation & Service Setup

## Prerequisites

* **Rust Toolchain**: Cargo 1.80 or higher (recommended installation via [rustup.rs](https://rustup.rs/)).
* **C Compiler**: Required for SQLite bindings (MSVC build tools on Windows, `build-essential` on Linux/Debian, or Xcode command line tools on macOS).
* **Perl** (Linux/macOS only): required to build the vendored OpenSSL used for self-contained Unix packages. Windows builds use SChannel and do not need OpenSSL or Perl.

## Building from Source

Clone the repository and compile all workspace members in release mode:

```bash
cargo build --release
```

The compiled binaries will be generated in `target/release/`:

* `mellowmeshd` (`.exe` on Windows) — the background daemon.
* `mellowmesh` (`.exe` on Windows) — the CLI client.

## Environment & Path Setup

To run `mellowmesh` from any directory, add the release folder to your system PATH:

### Windows (PowerShell)

```powershell
[System.Environment]::SetEnvironmentVariable(
    "Path",
    [System.Environment]::GetEnvironmentVariable("Path", [System.EnvironmentVariableTarget]::User) + ";<path-to-repo>\target\release",
    [System.EnvironmentVariableTarget]::User
)
```

### macOS / Linux

```bash
export PATH="$PATH:/path/to/mellowmesh/target/release"
# Add this line to your ~/.bashrc or ~/.zshrc for persistence
```

## Running as a Persistent System Service

The CLI client **automatically launches** the daemon if it is not running, so a service is optional. Administrators can still configure the daemon to run persistently at startup:

### Windows (Task Scheduler)

```powershell
Register-ScheduledTask -TaskName "MellowMeshDaemon" -Action (New-ScheduledTaskAction -Execute "C:\path\to\mellowmeshd.exe" -Argument "--require-auth") -Trigger (New-ScheduledTaskTrigger -AtStartup) -Settings (New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries) -User "NT AUTHORITY\SYSTEM"
```

### Linux (systemd)

Create `/etc/systemd/system/mellowmesh.service` (the `.deb` package installs
the same unit from [`debian/mellowmesh.service`](../debian/mellowmesh.service)):

```ini
[Unit]
Description=MellowMesh Intelligent Coordination Daemon
After=network.target

[Service]
DynamicUser=yes
StateDirectory=mellowmesh
StateDirectoryMode=0700
WorkingDirectory=/var/lib/mellowmesh
Environment=MELLOWMESH_DB=/var/lib/mellowmesh/mellowmesh.db
Environment=MELLOWMESH_NOTIFICATIONS=off
ExecStart=/usr/local/bin/mellowmeshd --require-auth --db /var/lib/mellowmesh/mellowmesh.db
Restart=on-failure
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
LockPersonality=yes
MemoryDenyWriteExecute=yes

[Install]
WantedBy=multi-user.target
```

A system service is reachable by every local user, so it always runs with
`--require-auth`. `DynamicUser=yes` makes systemd allocate an unprivileged
account; no user needs to be created. On first start the daemon writes the
owner token to `/var/lib/mellowmesh/owner.token` (mode 0600); read it as root
and mint narrower tokens with `mellowmesh token create`.

```bash
sudo systemctl enable mellowmesh
sudo systemctl start mellowmesh
```

## Packaging & Installer Generation

MellowMesh generates platform-native installers using `cargo-packager`. Packaging scripts live in the workspace root:

### Windows Installer (.msi)

* **Prerequisite**: none beyond Rust — `cargo-packager` downloads (and hash-verifies) the WiX 3 toolset it needs.
* **Run**: `.\package-msi.ps1` (add `-RequireSignature` to fail when no signing certificate is configured)
* **Output**: `target/release/mellowmesh_<version>_x64_en-US.msi`

The MSI adds the install directory to `PATH`; it does not register any URL protocol handler.

**Code signing.** The script never generates a self-signed certificate and never
touches the machine's trust stores. It signs only when a real code-signing
certificate is supplied through the environment; otherwise it builds an unsigned
MSI and prints a warning:

| Variable | Meaning |
| :--- | :--- |
| `MELLOWMESH_SIGN_CERT_THUMBPRINT` | Thumbprint of a code-signing certificate already in `Cert:\CurrentUser\My` (e.g. hardware-token or HSM backed) |
| `MELLOWMESH_SIGN_PFX_BASE64` / `MELLOWMESH_SIGN_PFX_PASSWORD` | Base64 PFX + password (e.g. CI secrets); imported with a non-exportable key for the build and removed afterwards |
| `MELLOWMESH_SIGN_TIMESTAMP_URL` | Timestamp server (default `http://timestamp.digicert.com`) |
| `MELLOWMESH_SIGN_TSP` | `1` if the timestamp server speaks RFC 3161 |

All packaging scripts install a pinned `cargo-packager` version with `--locked`
(override with `CARGO_PACKAGER_VERSION`) and build with `--locked`.

### Debian & Ubuntu Installer (.deb)

* **Run**: `./package-deb.sh`
* **Output**: `target/release/mellowmesh_<version>_amd64.deb`

Includes CLI and daemon binaries and automated systemd unit registration.

### macOS Installer (.dmg)

* **Run**: `./package-dmg.sh`
* **Output**: `target/release/mellowmesh_<version>_x64.dmg`
* To distribute without Gatekeeper warnings, the binaries and DMG must be codesigned and notarized with an Apple Developer ID.

### Automated CI/CD Releases

Pushing a release tag (`v*`) triggers the GitHub Actions workflow [release.yml](../.github/workflows/release.yml), which runs a multi-platform matrix build and attaches the three native installers, each with a `.sha256` checksum file, as build artifacts. Verify a download with `sha256sum -c <installer>.sha256` (or `shasum -a 256 -c` on macOS).
