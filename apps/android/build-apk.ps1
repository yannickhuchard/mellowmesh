# Builds the MellowMesh debug APK on Windows.
#   powershell -ExecutionPolicy Bypass -File .\build-apk.ps1
# Checks for a JDK (17+) and the Android SDK, runs gradlew assembleDebug and
# prints where the APK is.
$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

function Fail($msg) {
    Write-Host ""
    Write-Host "ERROR: $msg" -ForegroundColor Red
    exit 1
}

# --- JDK -------------------------------------------------------------------
$java = $null
if ($env:JAVA_HOME -and (Test-Path (Join-Path $env:JAVA_HOME "bin\java.exe"))) {
    $java = Join-Path $env:JAVA_HOME "bin\java.exe"
} else {
    $cmd = Get-Command java -ErrorAction SilentlyContinue
    if ($cmd) { $java = $cmd.Source }
}
if (-not $java) {
    # Android Studio ships a JDK ("jbr"); use it when nothing else is set up.
    $studioJbr = Join-Path ${env:ProgramFiles} "Android\Android Studio\jbr"
    if (Test-Path (Join-Path $studioJbr "bin\java.exe")) {
        $env:JAVA_HOME = $studioJbr
        $java = Join-Path $studioJbr "bin\java.exe"
    }
}
if (-not $java) {
    Fail "No Java found. Install JDK 17 or newer (or Android Studio) and set JAVA_HOME."
}
# "java -version" prints to stderr; do not let that stop the script.
$previousPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$versionLine = (& $java -version 2>&1 | Select-Object -First 1 | Out-String).Trim()
$ErrorActionPreference = $previousPreference
Write-Host "Java: $versionLine"
if ($versionLine -match '"(\d+)') {
    if ([int]$Matches[1] -lt 17) { Fail "Java 17 or newer is needed (found $($Matches[1]))." }
}

# --- Android SDK ------------------------------------------------------------
$sdk = $null
if (Test-Path "local.properties") {
    $line = Get-Content "local.properties" | Where-Object { $_ -match '^sdk\.dir=' } | Select-Object -First 1
    if ($line) { $sdk = ($line -replace '^sdk\.dir=', '') -replace '\\:', ':' -replace '\\\\', '\' }
}
if (-not $sdk -and $env:ANDROID_HOME) { $sdk = $env:ANDROID_HOME }
if (-not $sdk -and $env:ANDROID_SDK_ROOT) { $sdk = $env:ANDROID_SDK_ROOT }
if (-not $sdk) {
    $default = Join-Path $env:LOCALAPPDATA "Android\Sdk"
    if (Test-Path $default) { $sdk = $default }
}
if (-not $sdk -or -not (Test-Path $sdk)) {
    Fail "Android SDK not found. Install it with Android Studio, then set ANDROID_HOME or create local.properties with sdk.dir=..."
}
Write-Host "Android SDK: $sdk"
if (-not (Test-Path (Join-Path $sdk "platforms\android-35"))) {
    Write-Host "Note: platform android-35 is not installed; Gradle will try to download it (accept the licenses with sdkmanager if asked)." -ForegroundColor Yellow
}
if (-not (Test-Path "local.properties")) {
    $escaped = $sdk -replace '\\', '\\' -replace ':', '\:'
    Set-Content -Path "local.properties" -Value "sdk.dir=$escaped" -Encoding ASCII
    Write-Host "Wrote local.properties"
}

# --- Build ------------------------------------------------------------------
Write-Host ""
Write-Host "Building the debug APK (the first build downloads Gradle and libraries; it can take a few minutes)..."
& .\gradlew.bat assembleDebug
if ($LASTEXITCODE -ne 0) { Fail "The build failed (exit code $LASTEXITCODE). See the messages above." }

$apk = Join-Path $PSScriptRoot "app\build\outputs\apk\debug\app-debug.apk"
if (-not (Test-Path $apk)) { Fail "Build finished but the APK was not found at $apk" }
Write-Host ""
Write-Host "APK ready:" -ForegroundColor Green
Write-Host "  $apk"
Write-Host ""
Write-Host "Install with:  adb install -r `"$apk`""
Write-Host "or copy it to your phone and open it."
