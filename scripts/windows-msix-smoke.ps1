<#
.SYNOPSIS
  Installs the Store package (MSIX) and checks the installed app works.

.DESCRIPTION
  Signs a copy of the MSIX with a throwaway certificate, installs it, starts
  it, and checks:
    - LinkAll.exe keeps running after start-up, and the tray helper is up;
    - the engine answers on its IPC pipe (linkall-cli status);
    - a linkall:// link reaches the app;
    - notifications register, so pairing toasts' buttons work;
    - Windows added the package's inbound firewall rules;
    - pairing, a clip each way and a file each way work with a second Link
      All device (linkall-core's smoke_peer example, on this machine),
      which dials the app over IPv4;
    - nothing in the app's trace mentions the Windows AI runtime, which the
      MSIX leaves out.

  Needs administrator rights (to trust the certificate machine-wide), so it is
  meant for CI runners.

.EXAMPLE
  pwsh scripts\windows-msix-smoke.ps1 -Msix dist\msix\LinkAll-windows-x64.msix `
    -Cli target\release\linkall-cli.exe -Peer target\release\examples\smoke_peer.exe
#>
param(
  [Parameter(Mandatory)] [string]$Msix,
  [string]$Cli = "target\release\linkall-cli.exe",
  [string]$Peer = "target\release\examples\smoke_peer.exe"
)
$ErrorActionPreference = 'Stop'
$failures = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
  if ($ok) { Write-Host "PASS  $what" } else { Write-Host "FAIL  $what"; $failures.Add($what) }
}
function WaitFor([scriptblock]$cond, [int]$seconds) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 500 }
  return [bool](& $cond)
}
$Cli = (Resolve-Path $Cli).Path
$Peer = (Resolve-Path $Peer).Path

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $Msix))
try {
  $reader = New-Object System.IO.StreamReader(($zip.Entries | Where-Object FullName -eq 'AppxManifest.xml').Open())
  try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
} finally { $zip.Dispose() }

# Sign a copy with a throwaway certificate and trust it for this machine.
$cert = New-SelfSignedCertificate -Type Custom -Subject $manifest.Package.Identity.Publisher `
  -FriendlyName "Link All smoke test" -KeyUsage DigitalSignature -CertStoreLocation Cert:\CurrentUser\My `
  -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
$cer = Join-Path $env:TEMP "linkall-smoke.cer"
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
$signed = Join-Path $env:TEMP "LinkAll-smoke.msix"
Copy-Item $Msix $signed -Force
& $signtool.FullName sign /fd SHA256 /sha1 $cert.Thumbprint $signed | Out-Null
if ($LASTEXITCODE) { throw "signtool failed" }

Add-AppxPackage -Path $signed -ForceApplicationShutdown
$pkg = Get-AppxPackage -Name $manifest.Package.Identity.Name
Check ($null -ne $pkg) "the package installs"
if (-not $pkg) { throw "install failed" }

# A fresh install writes %LOCALAPPDATA% to the package's own copy.
$traceFiles = @(
  (Join-Path $env:LOCALAPPDATA "Packages\$($pkg.PackageFamilyName)\LocalCache\Local\LinkAll\winui_trace.txt"),
  (Join-Path $env:LOCALAPPDATA "LinkAll\winui_trace.txt")
)
function Trace() {
  foreach ($f in $traceFiles) { if (Test-Path $f) { return Get-Content $f -Raw } }
  return ""
}

Start-Process "shell:AppsFolder\$($pkg.PackageFamilyName)!App"
Check (WaitFor { Get-Process LinkAll -ErrorAction SilentlyContinue } 30) "LinkAll.exe starts"
Check (WaitFor { (Trace) -match 'Native engine started' } 30) "the engine starts"
Start-Sleep -Seconds 10
Check ($null -ne (Get-Process LinkAll -ErrorAction SilentlyContinue)) "LinkAll.exe is still running 10 s later"
Check (WaitFor { Get-Process LinkAll.Tray -ErrorAction SilentlyContinue } 20) "the tray helper is running"
Check (WaitFor { & $Cli status *> $null; $LASTEXITCODE -eq 0 } 20) "the engine answers on its IPC pipe"

Start-Process "linkall://smoke-test"
Check (WaitFor { (Trace) -match 'protocol activation' } 20) "a linkall:// link reaches the app"

Check ((Trace) -match 'AppNotificationManager registered successfully') "notifications register (pairing toasts' buttons work)"
Check ((Trace) -notmatch 'Register FAILED') "no notification registration failure"

$exe = (Join-Path $pkg.InstallLocation 'LinkAll.exe').ToLowerInvariant()
$ports = Get-NetFirewallApplicationFilter -ErrorAction SilentlyContinue |
  Where-Object { $_.Program -and $_.Program.ToLowerInvariant() -eq $exe } |
  Get-NetFirewallRule | Where-Object { $_.Direction -eq 'Inbound' -and $_.Action -eq 'Allow' -and $_.Enabled -eq 'True' } |
  Get-NetFirewallPortFilter | ForEach-Object { "$($_.Protocol)/$($_.LocalPort)" }
foreach ($need in 'TCP/47823', 'UDP/47824', 'UDP/5353') {
  Check ($ports -contains $need) "inbound firewall rule $need for LinkAll.exe"
}

# A second device: pair, then a clip and a file each way.
$peerOut = Join-Path $env:TEMP "linkall-smoke-peer.txt"
$peerErr = Join-Path $env:TEMP "linkall-smoke-peer.err"
$env:RUST_BACKTRACE = '0'
$peerProc = Start-Process $Peer -ArgumentList '127.0.0.1', '47823', '150' -PassThru -NoNewWindow `
  -RedirectStandardOutput $peerOut -RedirectStandardError $peerErr
function PeerSaid([string]$pattern) { (Test-Path $peerOut) -and ((Get-Content $peerOut -Raw) -match $pattern) }

$peerId = $null
if (WaitFor { PeerSaid '(?m)^APP ' } 40) {
  $peerId = ([regex]'(?m)^PEER (\S+)').Match((Get-Content $peerOut -Raw)).Groups[1].Value
}
Check ([bool]$peerId) "the second device connects"
if ($peerId) {
  # Accept until the request has arrived and the device reports it paired.
  Check (WaitFor { & $Cli pair accept $peerId *> $null; PeerSaid '(?m)^PAIRED' } 40) "pairing is accepted"
  Check (WaitFor { (& $Cli history --search "smoke clip from peer" 2>&1 | Out-String) -match 'smoke clip from peer' } 30) "a clip from the device reaches the app"
  Check (WaitFor { Get-ChildItem "$env:USERPROFILE\Downloads\smoke-from-peer-*.bin" -ErrorAction SilentlyContinue | Where-Object Length -eq (3MB + 17) } 60) "a file from the device reaches the app"

  & $Cli send $peerId "smoke clip from app" | Out-Null
  $file = Join-Path $env:TEMP "smoke-from-app.bin"
  [IO.File]::WriteAllBytes($file, [byte[]]::new(2MB + 5))
  & $Cli send-file $peerId $file | Out-Null
  Check (WaitFor { PeerSaid '(?m)^GOT_CLIP smoke clip from app' } 30) "a clip from the app reaches the device"
  Check (WaitFor { PeerSaid "(?m)^GOT_FILE smoke-from-app\.bin $(2MB + 5)" } 60) "a file from the app reaches the device"
}
if (-not $peerProc.HasExited) { $peerProc.Kill() }
Write-Host "--- second device"; Get-Content $peerOut -ErrorAction SilentlyContinue; Get-Content $peerErr -ErrorAction SilentlyContinue | Select-Object -First 40

Check ($null -ne (Get-Process LinkAll -ErrorAction SilentlyContinue)) "LinkAll.exe is still running at the end"
Check ((Trace) -notmatch '(?i)onnxruntime|DirectML|Microsoft\.Windows\.AI') "no trace line mentions the Windows AI runtime"

Get-Process LinkAll, LinkAll.Tray -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-AppxPackage -Package $pkg.PackageFullName

if ($failures.Count) {
  Write-Host "`n--- trace ---"; Write-Host (Trace)
  throw "$($failures.Count) check(s) failed: $($failures -join '; ')"
}
Write-Host "`nAll smoke checks passed."
