#Requires -Version 5.1
<#
.SYNOPSIS
    Generates the MSIX / Microsoft Store PNG visual assets from icon.ico.

.DESCRIPTION
    Extracts the highest-resolution frame (256x256) from the repo's icon.ico and
    renders the logo/tile PNGs referenced by AppxManifest.xml into .\Assets.

    Run this once, and again whenever icon.ico changes. The generated PNGs under
    Assets\ are committed to the repository so CI does not need to regenerate them.

.EXAMPLE
    pwsh .\generate-assets.ps1
#>
[CmdletBinding()]
param(
    [string]$IconPath = (Join-Path $PSScriptRoot '..\..\icon.ico'),
    [string]$OutDir   = (Join-Path $PSScriptRoot 'Assets')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$IconPath = (Resolve-Path $IconPath).Path
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# Load the 256x256 frame from the .ico as a 32bpp ARGB bitmap.
$icon   = New-Object System.Drawing.Icon($IconPath, 256, 256)
$source = $icon.ToBitmap()

function New-Canvas {
    param([int]$Width, [int]$Height)
    $bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g   = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode  = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode      = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode    = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)
    return [pscustomobject]@{ Bitmap = $bmp; Graphics = $g }
}

function Save-Png {
    param([System.Drawing.Bitmap]$Bitmap, [System.Drawing.Graphics]$Graphics, [string]$Name)
    $Graphics.Dispose()
    $path = Join-Path $OutDir $Name
    $Bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host ("  {0,-24} {1}x{2}" -f $Name, $Bitmap.Width, $Bitmap.Height)
    $Bitmap.Dispose()
}

# Square logos: the icon fills the whole square; the manifest BackgroundColor
# shows through the icon's transparent margins on Start tiles.
$squares = @{
    'StoreLogo.png'          = 50
    'Square44x44Logo.png'    = 44
    'Square71x71Logo.png'    = 71
    'Square150x150Logo.png'  = 150
    'Square310x310Logo.png'  = 310
}

Write-Host "Generating square assets:"
foreach ($name in $squares.Keys) {
    $size   = $squares[$name]
    $canvas = New-Canvas -Width $size -Height $size
    $rect   = New-Object System.Drawing.Rectangle 0, 0, $size, $size
    $canvas.Graphics.DrawImage($source, $rect)
    Save-Png -Bitmap $canvas.Bitmap -Graphics $canvas.Graphics -Name $name
}

# Wide tile: center the square icon (66% of tile height) on a transparent canvas.
Write-Host "Generating wide tile:"
$wideW = 310; $wideH = 150
$canvas = New-Canvas -Width $wideW -Height $wideH
$logo   = [int][Math]::Round($wideH * 0.66)
$x      = [int](($wideW - $logo) / 2)
$y      = [int](($wideH - $logo) / 2)
$rect   = New-Object System.Drawing.Rectangle $x, $y, $logo, $logo
$canvas.Graphics.DrawImage($source, $rect)
Save-Png -Bitmap $canvas.Bitmap -Graphics $canvas.Graphics -Name 'Wide310x150Logo.png'

$source.Dispose()
$icon.Dispose()
Write-Host "Done. Assets written to $OutDir"
