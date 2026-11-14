#Requires -Version 5.1
<#
.SYNOPSIS
    Builds MellowMesh for Windows: restores, runs the Core tests, builds the
    WinUI 3 app and publishes a self-contained folder to apps\windows\dist\.

.EXAMPLE
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
    [switch]$Zip
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
