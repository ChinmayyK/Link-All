<#
  Size report for the Windows app: total publish folder, tray helper, the
  largest folders and files, and optionally the MSI. Run it after each size
  change so every step has a measured number.

    pwsh scripts\windows-size.ps1 publish\windows
    pwsh scripts\windows-size.ps1 publish\windows Link All-windows-x64.msi
#>
param(
    [Parameter(Mandatory)] [string] $PublishDir,
    [string] $Msi
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path $PublishDir).Path

function MB([long] $bytes) { '{0,8:N1} MB' -f ($bytes / 1MB) }
function Size([string] $path) {
    (Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
}

$total = Size $root
Write-Output "Publish folder: $root"
Write-Output "$(MB $total)  total ($((Get-ChildItem $root -Recurse -File).Count) files)"
Write-Output "$(MB (Size (Join-Path $root 'tray')))  tray\"
Write-Output "$(MB (Get-ChildItem $root -File | Measure-Object Length -Sum).Sum)  top-level files"

Write-Output ''
Write-Output 'Largest folders:'
Get-ChildItem $root -Directory |
    ForEach-Object { [pscustomobject]@{ Name = $_.Name; Bytes = (Size $_.FullName) } } |
    Sort-Object Bytes -Descending | Select-Object -First 8 |
    ForEach-Object { Write-Output "$(MB $_.Bytes)  $($_.Name)\" }

Write-Output ''
Write-Output 'Largest files:'
Get-ChildItem $root -Recurse -File | Sort-Object Length -Descending | Select-Object -First 12 |
    ForEach-Object { Write-Output "$(MB $_.Length)  $($_.FullName.Substring($root.Length + 1))" }

if ($Msi) {
    Write-Output ''
    Write-Output "$(MB (Get-Item $Msi).Length)  MSI ($Msi)"
}
