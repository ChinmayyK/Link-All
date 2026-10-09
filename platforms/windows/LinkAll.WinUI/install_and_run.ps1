# Link All Windows Setup Script
# This script ensures the .NET SDK is available, copies the native backend DLL,
# publishes the WPF app as a standalone release, and places a shortcut on your Desktop.

Write-Host "========================================"
Write-Host " Link All Windows & Android Setup "
Write-Host "========================================"

# 0. Clean up previous installations
Write-Host "[*] Cleaning up previous Windows installation..." -ForegroundColor Cyan
Stop-Process -Name "LinkAll" -Force -ErrorAction SilentlyContinue
$installPath = "$env:LOCALAPPDATA\LinkAllApp"
if (Test-Path $installPath) {
    Remove-Item -Path $installPath -Recurse -Force -ErrorAction SilentlyContinue
}
$legacyPath = "$env:LOCALAPPDATA\Programs\LinkAll"
if (Test-Path $legacyPath) {
    Remove-Item -Path $legacyPath -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "[*] Cleaning up previous Android installation..." -ForegroundColor Cyan
if (Get-Command "adb" -ErrorAction SilentlyContinue) {
    adb uninstall app.linkall | Out-Null
} else {
    Write-Host "[-] adb not found in PATH. Skipping Android uninstall." -ForegroundColor Yellow
}

# 1. Build the native core fresh from source. Never use the checked-in
#    release/windows/linkall_core.dll snapshot here - it is a historical
#    release artifact, not a build input, and using it silently links the
#    WinUI app against a stale/mismatched core (wrong IPC pipe protocol,
#    missing IPC commands, etc).
Write-Host "[*] Building linkall-core (release) from current source..." -ForegroundColor Cyan
Push-Location "..\..\.."
cargo build --release -p linkall-core
$cargoExitCode = $LASTEXITCODE
Pop-Location

if ($cargoExitCode -ne 0) {
    Write-Host "[-] cargo build failed. Please check the errors above." -ForegroundColor Red
    Exit
}

if (Test-Path "..\..\..\target\release\linkall_core.dll") {
    Write-Host "[+] Native backend built successfully."
} else {
    Write-Host "[-] linkall_core.dll not found in target\release after build." -ForegroundColor Red
    Exit
}

# 2. Check for .NET SDK
$dotnetCmd = "$env:LOCALAPPDATA\Microsoft\dotnet\dotnet.exe"
if (-not (Test-Path $dotnetCmd)) {
    $dotnetCmd = "dotnet"
}
Write-Host "[+] Using .NET SDK at $dotnetCmd"

# 3. Publish the Application
$installPath = "$env:LOCALAPPDATA\LinkAllApp"
Write-Host "[*] Building and publishing Link All to $installPath..." -ForegroundColor Cyan

& $dotnetCmd publish LinkAll.WinUI.csproj -c Release -r win-x64 --self-contained true -o $installPath

if ($LASTEXITCODE -ne 0) {
    Write-Host "[-] Build failed. Please check the errors above." -ForegroundColor Red
    Exit
}

Write-Host "[+] Build published successfully!" -ForegroundColor Green

# 4. Create Desktop Shortcut
$WshShell = New-Object -comObject WScript.Shell
$ShortcutPath = "$env:USERPROFILE\Desktop\LinkAll.lnk"
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = "$installPath\LinkAll.exe"
$Shortcut.WorkingDirectory = $installPath
$Shortcut.IconLocation = "$installPath\LinkAll.exe,0"
$Shortcut.Save()

Write-Host "[+] Desktop shortcut created at $ShortcutPath."

# 5. Launch Windows Application
Write-Host "[*] Launching Link All for Windows..." -ForegroundColor Cyan
Start-Process "$installPath\LinkAll.exe"

# 6. Install Android Application
Write-Host "[*] Building and installing Android application..." -ForegroundColor Cyan
$androidDir = "..\..\android"
if (Test-Path "$androidDir\gradlew.bat") {
    Push-Location $androidDir
    Write-Host "[*] Running Gradle installDebug..."
    .\gradlew.bat installDebug
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[+] Android app installed successfully!" -ForegroundColor Green
    } else {
        Write-Host "[-] Android app installation failed." -ForegroundColor Red
    }
    Pop-Location
} else {
    Write-Host "[-] Android project not found at $androidDir." -ForegroundColor Yellow
}

Write-Host "========================================"
Write-Host " Setup Complete! Link All is installed on both platforms." -ForegroundColor Green
Write-Host "========================================"
