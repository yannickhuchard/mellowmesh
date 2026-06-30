param (
    [switch]$Sign,
    [string]$PublisherName = "Yannick Huchard"
)

$ErrorActionPreference = "Stop"

# 1. Check and install cargo-packager
$PackagerInstalled = Get-Command cargo-packager -ErrorAction SilentlyContinue
if (-not $PackagerInstalled) {
    Write-Host "cargo-packager is not installed. Installing..." -ForegroundColor Yellow
    cargo install cargo-packager
} else {
    Write-Host "cargo-packager is already installed." -ForegroundColor Green
}

# 2. Build release binaries
Write-Host "Building MellowMesh workspace in release mode..." -ForegroundColor Cyan
cargo build --release --workspace

# 3. Handle Optional Code Signing
if ($Sign) {
    Write-Host "Code signing enabled. Setting up certificate for '$PublisherName'..." -ForegroundColor Cyan
    
    # Locate or create a self-signed certificate
    $Subject = "CN=$PublisherName"
    $Cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -like "*$Subject*" -and $_.Type -eq "CodeSigningCert" } | Select-Object -First 1

    if (-not $Cert) {
        Write-Host "Certificate not found. Creating a new self-signed certificate..." -ForegroundColor Yellow
        $Cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -KeyUsage DigitalSignature -FriendlyName "MellowMesh Code Signing Certificate" -CertStoreLocation "Cert:\CurrentUser\My"
    }

    $Thumbprint = $Cert.Thumbprint
    Write-Host "Using certificate with thumbprint: $Thumbprint" -ForegroundColor Green

    # Attempt to trust the certificate locally (requires admin, so we try/catch)
    try {
        Write-Host "Attempting to import certificate to Trusted Root..." -ForegroundColor Cyan
        $TempPath = "$env:TEMP\mellowmesh_test.cer"
        Export-Certificate -Cert $Cert -FilePath $TempPath | Out-Null
        Import-Certificate -FilePath $TempPath -CertStoreLocation "Cert:\LocalMachine\Root" | Out-Null
        Write-Host "Successfully trusted the certificate locally!" -ForegroundColor Green
    } catch {
        Write-Host "Warning: Could not add certificate to Trusted Root (requires Administrator privileges). The installer will still compile and be signed, but local UAC prompt will show warning unless run as Admin." -ForegroundColor Yellow
    }

    # Temporarily inject certificate-thumbprint into mellowmesh-cli Cargo.toml
    $CargoPath = "crates/mellowmesh-cli/Cargo.toml"
    $OriginalCargoContent = Get-Content $CargoPath -Raw

    try {
        $SigningConfig = "`n[package.metadata.packager.windows]`ncertificate-thumbprint = `"$Thumbprint`"`n"
        Add-Content -Path $CargoPath -Value $SigningConfig
        
        # Package binaries with signing
        Write-Host "Packaging MellowMesh into .msi installer with automatic signing..." -ForegroundColor Cyan
        cargo packager --release -p mellowmesh-cli --formats wix
    }
    finally {
        # Restore original Cargo.toml content
        [System.IO.File]::WriteAllText((Resolve-Path $CargoPath), $OriginalCargoContent)
    }
} else {
    # Package binaries without signing
    Write-Host "Packaging MellowMesh into .msi installer (unsigned)..." -ForegroundColor Cyan
    cargo packager --release -p mellowmesh-cli --formats wix
}

Write-Host "Windows packaging completed successfully!" -ForegroundColor Green
