#Requires -Version 5.1
<#
.SYNOPSIS
    Packs WinDisplayManager into an MSIX package from an already-built release EXE.

.DESCRIPTION
    Stages the release executable, the visual assets and a version-stamped copy of
    AppxManifest.xml, then runs makeappx.exe to produce a .msix.

    For a Microsoft Store submission, upload the UNSIGNED .msix to Partner Center
    (the Store re-signs it). Use -Sign only to test-install the package locally.

.PARAMETER Version
    4-part package version (x.y.z.0). Defaults to the Cargo.toml version + ".0".

.PARAMETER Sign
    Also sign the package with a local self-signed certificate for sideload testing.

.EXAMPLE
    # 1. Build the app
    cargo build --release
    # 2. Produce the Store package (unsigned)
    pwsh .\packaging\msix\build-msix.ps1

.EXAMPLE
    # Produce a signed package for LOCAL testing, then install it.
    #   a) Create a self-signed cert whose subject EXACTLY matches Identity/@Publisher:
    #        $c = New-SelfSignedCertificate -Type Custom -CertStoreLocation Cert:\CurrentUser\My `
    #               -Subject "CN=412A6E72-F28E-4C36-AE10-EDF861A3A7FB" `
    #               -KeyUsage DigitalSignature -FriendlyName "WinDisplayManager Test" `
    #               -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3","2.5.29.19={text}")
    #   b) Trust it (Admin, one-time): export $c to a .cer and import into
    #        Cert:\LocalMachine\Root  (or  Cert:\LocalMachine\TrustedPeople).
    #   c) pwsh .\packaging\msix\build-msix.ps1 -Sign -CertThumbprint $c.Thumbprint
    #   d) Add-AppxPackage .\packaging\msix\obj\windisplaymanager_rs-<ver>.msix
    #   e) Remove-AppxPackage (Get-AppxPackage *WinDisplayManager*).PackageFullName
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Configuration = 'release',
    [string]$ExePath,
    [string]$OutputPath,
    [switch]$Sign,
    [string]$CertThumbprint
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
$repoRoot  = (Resolve-Path (Join-Path $scriptDir '..\..')).Path
$manifest  = Join-Path $scriptDir 'AppxManifest.xml'
$assets    = Join-Path $scriptDir 'Assets'
$stageDir  = Join-Path $scriptDir 'obj\pkg'
$objDir    = Join-Path $scriptDir 'obj'

function Find-SdkTool {
    param([string]$Name)
    $found = Get-Command $Name -ErrorAction SilentlyContinue
    if ($found) { return $found.Source }
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $hit = Get-ChildItem -Path $root -Recurse -Filter $Name -ErrorAction SilentlyContinue |
               Where-Object { $_.FullName -match '\\x64\\' } |
               Sort-Object FullName -Descending | Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }
    throw "Could not find $Name. Install the Windows 10/11 SDK."
}

# ---- Resolve version (default: Cargo.toml version + .0) -----------------------
if (-not $Version) {
    $cargo = Get-Content (Join-Path $repoRoot 'Cargo.toml') -Raw
    if ($cargo -match '(?m)^\s*version\s*=\s*"(\d+)\.(\d+)\.(\d+)"') {
        $Version = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
    } else {
        throw "Could not parse version from Cargo.toml; pass -Version x.y.z.0"
    }
}
if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') {
    throw "Version must be 4-part (x.y.z.0); got '$Version'."
}
Write-Host "Package version: $Version"

# ---- Resolve the built EXE ----------------------------------------------------
if (-not $ExePath) {
    $ExePath = Join-Path $repoRoot "target\$Configuration\windisplaymanager_rs.exe"
}
if (-not (Test-Path $ExePath)) {
    throw "Executable not found at '$ExePath'. Run: cargo build --$Configuration"
}

# ---- Stage layout -------------------------------------------------------------
if (Test-Path $stageDir) { Remove-Item $stageDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stageDir | Out-Null

# Version-stamped manifest copy
$manifestXml = Get-Content $manifest -Raw
$manifestXml = $manifestXml.Replace('Version="0.0.0.0"', "Version=""$Version""")

# Warn only if the actual identity VALUES are still placeholders (ignore comments).
$doc = [xml]$manifestXml
$idValues = "$($doc.Package.Identity.Name)|$($doc.Package.Identity.Publisher)|$($doc.Package.Properties.PublisherDisplayName)"
if ($idValues -match 'Placeholder') {
    Write-Warning "AppxManifest.xml still contains placeholder identity values. Fill in the Partner Center Name/Publisher/PublisherDisplayName before submitting to the Store."
}
Set-Content -Path (Join-Path $stageDir 'AppxManifest.xml') -Value $manifestXml -Encoding UTF8

Copy-Item $assets (Join-Path $stageDir 'Assets') -Recurse
Copy-Item $ExePath (Join-Path $stageDir 'windisplaymanager_rs.exe')

# ---- Pack ---------------------------------------------------------------------
if (-not $OutputPath) {
    $OutputPath = Join-Path $objDir "windisplaymanager_rs-$Version.msix"
}
$makeappx = Find-SdkTool 'makeappx.exe'
Write-Host "makeappx: $makeappx"
& $makeappx pack /o /d $stageDir /p $OutputPath
if ($LASTEXITCODE -ne 0) { throw "makeappx failed with exit code $LASTEXITCODE." }
Write-Host "Package created: $OutputPath"

# ---- Optional local test signing ---------------------------------------------
if ($Sign) {
    if (-not $CertThumbprint) {
        throw "Pass -CertThumbprint <thumb> of a self-signed cert whose subject matches Identity/@Publisher (see examples in the script header)."
    }
    $signtool = Find-SdkTool 'signtool.exe'
    & $signtool sign /fd SHA256 /sha1 $CertThumbprint $OutputPath
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE." }
    Write-Host "Signed: $OutputPath"
}
