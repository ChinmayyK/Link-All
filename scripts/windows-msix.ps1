<#
.SYNOPSIS
  Builds the Microsoft Store package (MSIX) of Link All for Windows.

.DESCRIPTION
  Default: an unsigned dist\msix\LinkAll-windows-x64.msix, the file to upload
  in Partner Center. The Store signs it, so no certificate is needed.

  -Sign signs that copy with a local test certificate whose subject matches
  the manifest's Publisher (made on first use, kept in CurrentUser\My) and
  trusts it for this machine, which asks for administrator rights once.
  -Install then installs it, so the Store build can be tried before
  submission. Uninstall the MSI build first: both would start at sign-in.

  Needs Rust, the .NET 10 SDK and the Windows SDK (signtool, for -Sign).

.EXAMPLE
  pwsh scripts\windows-msix.ps1                  # the file for Partner Center
  pwsh scripts\windows-msix.ps1 -Sign -Install   # try the Store build locally
#>
param(
  [string]$OutDir = "dist\msix",
  [switch]$Sign,
  [switch]$Install,
  [switch]$SkipRust
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..").Path
Set-Location $root

$appDir = "platforms\windows\LinkAll.WinUI"
$manifest = "$appDir\Package.appxmanifest"

if (-not $SkipRust) {
  cargo build --release -p linkall-core
  if ($LASTEXITCODE) { throw "cargo build failed" }
}
Copy-Item target\release\linkall_core.dll $appDir\ -Force

$pkgDir = Join-Path $root "$OutDir\pkg\"
Remove-Item $pkgDir -Recurse -Force -ErrorAction SilentlyContinue

# The csproj builds the unpackaged app by default (WindowsPackageType None);
# these properties make the same project produce the MSIX instead.
dotnet publish "$appDir\LinkAll.WinUI.csproj" -c Release -r win-x64 --self-contained true `
  -p:Platform=x64 `
  -p:WindowsPackageType=MSIX `
  -p:GenerateAppxPackageOnBuild=true `
  -p:AppxPackageSigningEnabled=false `
  -p:AppxBundle=Never `
  -p:UapAppxPackageBuildMode=SideloadOnly `
  -p:AppxPackageDir=$pkgDir
if ($LASTEXITCODE) { throw "dotnet publish failed" }

$msix = Get-ChildItem $pkgDir -Recurse -Filter *.msix |
  Where-Object { $_.Name -notlike 'Microsoft.*' } |
  Select-Object -First 1
if (-not $msix) { throw "no .msix was produced under $pkgDir" }

# The tray helper is a separate exe; the app has no tray icon without it.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($msix.FullName)
try {
  foreach ($need in 'LinkAll.exe', 'linkall_core.dll', 'tray/LinkAll.Tray.exe') {
    if (-not ($zip.Entries | Where-Object { $_.FullName -eq $need })) { throw "the MSIX is missing $need" }
  }
  # The unused Windows AI runtime is ~16 MB; the csproj keeps it out (TrimMsixPayload).
  foreach ($unused in 'onnxruntime.dll', 'DirectML.dll') {
    if ($zip.Entries | Where-Object { $_.Name -eq $unused }) { throw "the MSIX still contains $unused" }
  }
  $mb = [math]::Round((Get-Item $msix.FullName).Length / 1MB, 1)
  Write-Host "MSIX size: $mb MB"
} finally { $zip.Dispose() }

$out = Join-Path $root "$OutDir\LinkAll-windows-x64.msix"
Copy-Item $msix.FullName $out -Force
Write-Host "MSIX for Partner Center: $out"

if (-not $Sign) { return }

[xml]$xml = Get-Content $manifest -Encoding UTF8
$publisher = $xml.Package.Identity.Publisher

$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $publisher } | Select-Object -First 1
if (-not $cert) {
  $cert = New-SelfSignedCertificate -Type Custom -Subject $publisher `
    -FriendlyName "Link All local test signing" -KeyUsage DigitalSignature `
    -CertStoreLocation Cert:\CurrentUser\My `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
  Write-Host "Made a test certificate for $publisher"
}

# Windows installs a sideloaded package only if its signer is trusted machine-wide.
$trusted = Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
if (-not $trusted) {
  $cer = Join-Path $root "$OutDir\LinkAll-test.cer"
  Export-Certificate -Cert $cert -FilePath $cer | Out-Null
  Write-Host "Trusting the test certificate (administrator prompt)..."
  Start-Process powershell -Verb RunAs -Wait -ArgumentList `
    "-NoProfile -Command Import-Certificate -FilePath '$cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople"
}

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
  Sort-Object FullName | Select-Object -Last 1
if (-not $signtool) { throw "signtool.exe not found: install the Windows SDK" }

$signed = Join-Path $root "$OutDir\LinkAll-windows-x64-test.msix"
Copy-Item $out $signed -Force
& $signtool.FullName sign /fd SHA256 /sha1 $cert.Thumbprint $signed
if ($LASTEXITCODE) { throw "signtool failed" }
Write-Host "Test-signed MSIX: $signed"

if ($Install) {
  Add-AppxPackage -Path $signed -ForceApplicationShutdown
  Write-Host "Installed. Open Link All from the Start menu."
}
