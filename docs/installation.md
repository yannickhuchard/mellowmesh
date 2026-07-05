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
Register-ScheduledTask -TaskName "MellowMeshDaemon" -Action (New-ScheduledTaskAction -Execute "C:\path\to\mellowmeshd.exe") -Trigger (New-ScheduledTaskTrigger -AtStartup) -Settings (New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries) -User "NT AUTHORITY\SYSTEM"
```

### Linux (systemd)

Create `/etc/systemd/system/mellowmesh.service`:

```ini
[Unit]
Description=MellowMesh Intelligent Coordination Daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/mellowmeshd
Restart=always
User=mellowmesh
Environment=MELLOWMESH_DB=/var/lib/mellowmesh/mellowmesh.db

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable mellowmesh
sudo systemctl start mellowmesh
```

## Packaging & Installer Generation

MellowMesh generates platform-native installers using `cargo-packager`. Packaging scripts live in the workspace root:

### Windows Installer (.msi)

* **Prerequisite**: [WiX Toolset](https://wixtoolset.org/) (v3 or v4).
* **Run**: `.\package-msi.ps1`
* **Output**: `target/release/mellowmesh_<version>_x64_en-US.msi`

The MSI also registers the `mellowmesh://` custom protocol handler (see the [API guide](api.md)).

### Debian & Ubuntu Installer (.deb)

* **Run**: `./package-deb.sh`
* **Output**: `target/release/mellowmesh_<version>_amd64.deb`

Includes CLI and daemon binaries and automated systemd unit registration.

### macOS Installer (.dmg)

* **Run**: `./package-dmg.sh`
* **Output**: `target/release/mellowmesh_<version>_x64.dmg`
* To distribute without Gatekeeper warnings, the binaries and DMG must be codesigned and notarized with an Apple Developer ID.

### Automated CI/CD Releases

Pushing a release tag (`v*`) triggers the GitHub Actions workflow [release.yml](../.github/workflows/release.yml), which runs a multi-platform matrix build and attaches the three native installers as build artifacts.
