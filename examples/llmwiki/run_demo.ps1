# Save original directory
$origDir = Get-Location

# Set location to this script's directory
Set-Location $PSScriptRoot

$daemonPath = "..\..\target\debug\mellowmeshd.exe"
$cliPath = "..\..\target\debug\mellowmesh.exe"

if (-not (Test-Path $daemonPath)) {
    Write-Host "Error: Daemon binary not found at $daemonPath. Please run 'cargo build' in the root first." -ForegroundColor Red
    Set-Location $origDir
    Exit 1
}

Write-Host "Starting MellowMesh Daemon in the background..." -ForegroundColor Cyan
$env:MELLOWMESH_WIKIS="quantum:.\quantum,agents:.\agents,onepiece:.\onepiece"

# Start the daemon process
$daemonProc = Start-Process -FilePath $daemonPath -NoNewWindow -PassThru

# Give it 2 seconds to start up
Start-Sleep -Seconds 2

if ($daemonProc.HasExited) {
    Write-Host "Error: Daemon failed to start." -ForegroundColor Red
    Set-Location $origDir
    Exit 1
}

Write-Host "Daemon running (PID: $($daemonProc.Id))" -ForegroundColor Green
Write-Host ""

try {
    # Run Sync operations
    Write-Host "--- 1. Synchronizing Wiki Namespaces ---" -ForegroundColor Yellow
    & $cliPath wiki sync --wiki quantum
    & $cliPath wiki sync --wiki agents
    & $cliPath wiki sync --wiki onepiece
    Write-Host ""

    # Run List operations
    Write-Host "--- 2. Listing Pages in 'quantum' Wiki ---" -ForegroundColor Yellow
    & $cliPath wiki list --wiki quantum
    Write-Host ""

    # Run Search operations
    Write-Host "--- 3. Searching for 'Nika' in 'onepiece' Wiki ---" -ForegroundColor Yellow
    & $cliPath wiki search "Nika" --wiki onepiece
    Write-Host ""

    # Run View operations
    Write-Host "--- 4. Viewing 'planning.md' in 'agents' Wiki ---" -ForegroundColor Yellow
    & $cliPath wiki view planning.md --wiki agents
    Write-Host ""
}
finally {
    Write-Host "Stopping daemon..." -ForegroundColor Cyan
    Stop-Process -Id $daemonProc.Id -Force
    Set-Location $origDir
}

Write-Host "Demo finished!" -ForegroundColor Green
