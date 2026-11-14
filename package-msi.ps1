<#
.SYNOPSIS
    Builds the MellowMesh Windows installer (.msi) with cargo-packager.

.DESCRIPTION
    The MSI is signed only when a real code-signing certificate is supplied
    through environment variables. Without one, an UNSIGNED installer is
    produced and a warning is printed. This script never creates a
    self-signed certificate and never modifies the machine's trust stores.

    Signing environment variables (all optional):

      MELLOWMESH_SIGN_CERT_THUMBPRINT
          SHA-1 thumbprint of a code-signing certificate that is already in
          Cert:\CurrentUser\My (for example backed by a hardware token or a
          cloud HSM key storage provider).

      MELLOWMESH_SIGN_PFX_BASE64 / MELLOWMESH_SIGN_PFX_PASSWORD
          A base64-encoded PFX and its password (e.g. from CI secrets). It is
          imported into Cert:\CurrentUser\My with a NON-exportable private
          key for the duration of the build and removed afterwards.

      MELLOWMESH_SIGN_TIMESTAMP_URL
          Timestamp server (default: http://timestamp.digicert.com).

      MELLOWMESH_SIGN_TSP
          Set to 1 if the timestamp server speaks RFC 3161 (TSP).

      CARGO_PACKAGER_VERSION
          Overrides the pinned cargo-packager version.

.PARAMETER RequireSignature
    Fail instead of producing an unsigned MSI when no certificate is provided.
#>
param (
    [switch]$RequireSignature
)

$ErrorActionPreference = "Stop"

# Pinned packager version. Keep in sync with the release workflow and the
# package-*.sh scripts.
$PackagerVersion = if ($env:CARGO_PACKAGER_VERSION) { $env:CARGO_PACKAGER_VERSION } else { "0.11.8" }

# 1. Check and install the pinned cargo-packager
$PackagerInstalled = $false
try {
    $InstalledVersion = (& cargo packager --version 2>$null | Out-String)
    if ($LASTEXITCODE -eq 0 -and $InstalledVersion -like "*$PackagerVersion*") { $PackagerInstalled = $true }
} catch {
    $PackagerInstalled = $false
}
if ($PackagerInstalled) {
    Write-Host "cargo-packager $PackagerVersion is already installed." -ForegroundColor Green
} else {
    Write-Host "Installing cargo-packager $PackagerVersion..." -ForegroundColor Yellow
    cargo install cargo-packager --locked --version $PackagerVersion
    if ($LASTEXITCODE -ne 0) { throw "cargo install cargo-packager failed" }
}

# 2. Build release binaries (exactly the dependency versions in Cargo.lock)
Write-Host "Building MellowMesh workspace in release mode..." -ForegroundColor Cyan
cargo build --release --workspace --locked
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

# 3. Resolve an optional, externally provided signing certificate
$Thumbprint = $null
$ImportedThumbprint = $null

if ($env:MELLOWMESH_SIGN_PFX_BASE64) {
    if (-not $env:MELLOWMESH_SIGN_PFX_PASSWORD) {
        throw "MELLOWMESH_SIGN_PFX_BASE64 is set but MELLOWMESH_SIGN_PFX_PASSWORD is not"
    }
    $PfxPath = Join-Path ([System.IO.Path]::GetTempPath()) ("mellowmesh-sign-" + [guid]::NewGuid().ToString() + ".pfx")
    try {
        [System.IO.File]::WriteAllBytes($PfxPath, [System.Convert]::FromBase64String($env:MELLOWMESH_SIGN_PFX_BASE64))
        $SecurePassword = ConvertTo-SecureString -String $env:MELLOWMESH_SIGN_PFX_PASSWORD -AsPlainText -Force
        # No -Exportable: the private key cannot be exported from the store.
        $Imported = Import-PfxCertificate -FilePath $PfxPath -CertStoreLocation "Cert:\CurrentUser\My" -Password $SecurePassword
        $ImportedThumbprint = ($Imported | Where-Object { $_.HasPrivateKey } | Select-Object -First 1).Thumbprint
        $Thumbprint = $ImportedThumbprint
    } finally {
        if (Test-Path $PfxPath) { Remove-Item -Force $PfxPath }
    }
} elseif ($env:MELLOWMESH_SIGN_CERT_THUMBPRINT) {
    $Thumbprint = ($env:MELLOWMESH_SIGN_CERT_THUMBPRINT -replace '\s', '').ToUpperInvariant()
}

if ($Thumbprint) {
    $Cert = Get-Item -Path "Cert:\CurrentUser\My\$Thumbprint" -ErrorAction SilentlyContinue
    if (-not $Cert) { throw "Signing certificate $Thumbprint not found in Cert:\CurrentUser\My" }
    if (-not $Cert.HasPrivateKey) { throw "Signing certificate $Thumbprint has no private key" }
    if ($Cert.NotAfter -lt (Get-Date)) { throw "Signing certificate $Thumbprint expired on $($Cert.NotAfter)" }
    $CodeSigningOid = "1.3.6.1.5.5.7.3.3"
    if (-not ($Cert.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq $CodeSigningOid })) {
        throw "Certificate $Thumbprint is not valid for code signing"
    }
    if ($Cert.Subject -eq $Cert.Issuer) {
        throw "Certificate $Thumbprint is self-signed; refusing to sign release artifacts with it"
    }
}

# 4. Package
$CargoPath = "crates/mellowmesh-cli/Cargo.toml"
try {
    if ($Thumbprint) {
        $TimestampUrl = if ($env:MELLOWMESH_SIGN_TIMESTAMP_URL) { $env:MELLOWMESH_SIGN_TIMESTAMP_URL } else { "http://timestamp.digicert.com" }
        $Tsp = if ($env:MELLOWMESH_SIGN_TSP -eq "1") { "true" } else { "false" }
        Write-Host "Signing with certificate $Thumbprint ($($Cert.Subject))" -ForegroundColor Green

        # Temporarily inject the signing settings into the packager metadata.
        $OriginalCargoContent = Get-Content $CargoPath -Raw
        try {
            $SigningConfig = "`n[package.metadata.packager.windows]`ncertificate-thumbprint = `"$Thumbprint`"`ndigest-algorithm = `"sha256`"`ntimestamp-url = `"$TimestampUrl`"`ntsp = $Tsp`n"
            Add-Content -Path $CargoPath -Value $SigningConfig

            Write-Host "Packaging MellowMesh into a signed .msi installer..." -ForegroundColor Cyan
            cargo packager --release -p mellowmesh-cli --formats wix
            if ($LASTEXITCODE -ne 0) { throw "cargo packager failed" }
        } finally {
            # Restore original Cargo.toml content
            [System.IO.File]::WriteAllText((Resolve-Path $CargoPath), $OriginalCargoContent)
        }
    } else {
        if ($RequireSignature) {
            throw "-RequireSignature was given but no signing certificate was provided (see MELLOWMESH_SIGN_* variables)"
        }
        Write-Warning "No code-signing certificate provided (MELLOWMESH_SIGN_CERT_THUMBPRINT or MELLOWMESH_SIGN_PFX_BASE64). Building an UNSIGNED installer; Windows SmartScreen will warn users who run it."
        Write-Host "Packaging MellowMesh into .msi installer (unsigned)..." -ForegroundColor Cyan
        cargo packager --release -p mellowmesh-cli --formats wix
        if ($LASTEXITCODE -ne 0) { throw "cargo packager failed" }
    }
} finally {
    # Remove a certificate this script imported from CI secrets.
    if ($ImportedThumbprint) {
        Remove-Item -Path "Cert:\CurrentUser\My\$ImportedThumbprint" -DeleteKey -ErrorAction SilentlyContinue
    }
}

Write-Host "Windows packaging completed successfully!" -ForegroundColor Green
