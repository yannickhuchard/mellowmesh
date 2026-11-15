#Requires -Version 5.1
<#
.SYNOPSIS
    Builds MellowMesh for Windows: restores, runs the Core tests, builds the
    WinUI 3 app and publishes a self-contained folder to apps\windows\dist\.

.EXAMPLE
    .\apps\windows\build.ps1 -Pair      # build, start the hub, open "Connect a phone" (QR code)
    .\apps\windows\build.ps1 -Run       # build and open the app
    .\apps\windows\build.ps1
    .\apps\windows\build.ps1 -Platform ARM64 -Zip
    .\apps\windows\build.ps1 -SkipTests -Configuration Debug
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'ARM64')]
    [string]$Platform = 'x64',
    [ValidateSet('Release', 'Debug')]
    [string]$Configuration = 'Release',
    [switch]$SkipTests,
    [switch]$Zip,
    # Open the app when the build is done.
    [switch]$Run,
    # Make sure the hub on this PC runs (a version that can pair), then open
    # the app straight on "Connect a phone" with its QR code. Implies -Run.
    [switch]$Pair
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

# A simple (non-advanced) function: every argument, dashes included, is passed through to dotnet.
function Invoke-Dotnet {
    $dotnetArgs = @($args)
    Write-Host "> dotnet $($dotnetArgs -join ' ')" -ForegroundColor DarkGray
    & dotnet @dotnetArgs
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet $($dotnetArgs[0]) failed with exit code $LASTEXITCODE"
    }
}

# 1. The .NET 8 SDK (or newer) must be installed.
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    throw "The .NET SDK was not found. Install the .NET 8 SDK: https://dotnet.microsoft.com/download/dotnet/8.0"
}
$sdks = @(& dotnet --list-sdks)
$supported = $sdks | Where-Object { $_ -match '^(\d+)\.' -and [int]$Matches[1] -ge 8 }
if (-not $supported) {
    throw ".NET 8 SDK or newer is required. Installed SDKs: $($sdks -join ', ')"
}
Write-Host "Using .NET SDK $(& dotnet --version)" -ForegroundColor Cyan

$rid = if ($Platform -eq 'ARM64') { 'win-arm64' } else { 'win-x64' }
$app = Join-Path $root 'MellowMesh.App'
$tests = Join-Path $root 'MellowMesh.Core.Tests'
$dist = Join-Path $root "dist\MellowMesh-$rid"

Push-Location $root
try {
    # 2. Restore.
    Invoke-Dotnet restore $tests
    Invoke-Dotnet restore $app "-p:Platform=$Platform" "-p:RuntimeIdentifier=$rid"

    # 3. Core tests (no network, no UI).
    if (-not $SkipTests) {
        Invoke-Dotnet test $tests '-c' $Configuration '--nologo'
    }

    # 4. Build the app.
    Invoke-Dotnet build $app '-c' $Configuration "-p:Platform=$Platform" "-p:RuntimeIdentifier=$rid" '--nologo'

    # 5. Publish a self-contained folder (no .NET or Windows App SDK install needed on the target PC).
    if (Test-Path $dist) {
        Remove-Item $dist -Recurse -Force
    }
    Invoke-Dotnet publish $app '-c' $Configuration '-r' $rid "-p:Platform=$Platform" '-p:PublishSingleFile=false' '--self-contained' 'true' '-o' $dist '--nologo'

    $exe = Join-Path $dist 'MellowMesh.exe'
    if (-not (Test-Path $exe)) {
        throw "Publish finished but $exe is missing."
    }
    if ($Zip) {
        $zipPath = "$dist.zip"
        if (Test-Path $zipPath) {
            Remove-Item $zipPath -Force
        }
        Compress-Archive -Path (Join-Path $dist '*') -DestinationPath $zipPath
        Write-Host "Zip: $zipPath" -ForegroundColor Green
    }
    Write-Host ""
    Write-Host "MellowMesh is ready: $exe" -ForegroundColor Green
}
finally {
    Pop-Location
}

# ---------------------------------------------------------------- -Pair / -Run
function Find-MellowMeshCli {
    $onPath = Get-Command mellowmesh -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $repo = Resolve-Path (Join-Path $root '..\..')
    foreach ($p in @('target\release\mellowmesh.exe', 'target\debug\mellowmesh.exe')) {
        $candidate = Join-Path $repo $p
        if (Test-Path $candidate) { return $candidate }
    }
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        Write-Host "Building the hub (mellowmesh + mellowmeshd) with cargo..." -ForegroundColor Cyan
        Push-Location $repo
        try {
            & cargo build --release -p mellowmesh-cli -p mellowmesh-daemon
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
        }
        finally { Pop-Location }
        return (Join-Path $repo 'target\release\mellowmesh.exe')
    }
    return $null
}

function Get-HubNetwork {
    try { return Invoke-RestMethod -Uri 'http://127.0.0.1:40000/network' -TimeoutSec 3 } catch { return $null }
}

if ($Pair) {
    $Run = $true
    $network = Get-HubNetwork
    $canPair = $network -and $network.network -and ($network.network.PSObject.Properties.Name -contains 'lan_access')
    if (-not $canPair) {
        $cli = Find-MellowMeshCli
        if (-not $cli) {
            throw "The MellowMesh hub is not installed: put mellowmesh.exe and mellowmeshd.exe on your PATH (or install Rust so this script can build them), then run again."
        }
        if ($network) {
            Write-Host "The running hub is an older version: restarting it with $cli" -ForegroundColor Cyan
            & $cli daemon restart
        }
        else {
            Write-Host "Starting the hub ($cli daemon start)" -ForegroundColor Cyan
            & $cli daemon start
        }
        if ($LASTEXITCODE -ne 0) { throw "Could not start the MellowMesh hub (exit code $LASTEXITCODE)." }
    }
}

if ($Run) {
    $launchArgs = if ($Pair) { @('--pair') } else { @() }
    Write-Host "Opening MellowMesh$(if ($Pair) { ' on Connect a phone' })..." -ForegroundColor Green
    Start-Process -FilePath $exe -ArgumentList $launchArgs -WorkingDirectory $dist
    if ($Pair) {
        Write-Host "On your phone: MellowMesh > Connect to my network > scan the code. If Windows Firewall asks about mellowmeshd, click 'Allow access'." -ForegroundColor Green
    }
}
