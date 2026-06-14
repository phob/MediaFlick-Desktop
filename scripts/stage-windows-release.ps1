param(
    [string]$StagingDir = "dist/MediaFlickDesktop",
    [string]$MpvSource = $env:MEDIAFLICK_DESKTOP_PACKAGE_MPV,
    [switch]$AllowMissingMpv
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    throw "The Windows release staging script must be run on Windows."
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BuildDir = Join-Path $RepoRoot "build"
$StagingPath = Join-Path $RepoRoot $StagingDir

if (-not (Test-Path (Join-Path $BuildDir "mediaflick-desktop.exe"))) {
    throw "Missing build/mediaflick-desktop.exe. Run 'just release' before staging the installer payload."
}

if (Test-Path $StagingPath) {
    Remove-Item -Recurse -Force $StagingPath
}
New-Item -ItemType Directory -Force $StagingPath | Out-Null

$RequiredFiles = @(
    "mediaflick-desktop.exe",
    "libcef.dll",
    "chrome_elf.dll",
    "icudtl.dat",
    "resources.pak",
    "chrome_100_percent.pak",
    "chrome_200_percent.pak",
    "v8_context_snapshot.bin"
)

$OptionalFiles = @(
    "d3dcompiler_47.dll",
    "dxcompiler.dll",
    "dxil.dll",
    "libEGL.dll",
    "libGLESv2.dll",
    "vk_swiftshader.dll",
    "vk_swiftshader_icd.json",
    "vulkan-1.dll",
    "CREDITS.html"
)

foreach ($File in $RequiredFiles) {
    $Source = Join-Path $BuildDir $File
    if (-not (Test-Path $Source)) {
        throw "Required runtime file is missing from build/: $File"
    }
    Copy-Item $Source -Destination $StagingPath -Force
}

foreach ($File in $OptionalFiles) {
    $Source = Join-Path $BuildDir $File
    if (Test-Path $Source) {
        Copy-Item $Source -Destination $StagingPath -Force
    }
}

$LocalesSource = Join-Path $BuildDir "locales"
if (-not (Test-Path $LocalesSource)) {
    throw "Required CEF locales directory is missing from build/."
}
Copy-Item $LocalesSource -Destination (Join-Path $StagingPath "locales") -Recurse -Force

function Resolve-MpvSource {
    param([string]$ExplicitSource)

    $Candidates = @()
    # Environment/explicit inputs win over local developer defaults.
    if ($ExplicitSource) { $Candidates += $ExplicitSource }
    if ($env:MEDIAFLICK_DESKTOP_MPV_PATH) { $Candidates += $env:MEDIAFLICK_DESKTOP_MPV_PATH }
    $Candidates += "C:\mpv"

    $Command = Get-Command "mpv.exe" -ErrorAction SilentlyContinue
    if ($Command) { $Candidates += $Command.Source }

    $Candidates += @(
        (Join-Path $env:ProgramFiles "mpv\mpv.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "mpv\mpv.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\mpv\mpv.exe")
    )

    foreach ($Candidate in $Candidates) {
        if ($Candidate -and (Test-Path $Candidate)) {
            return (Resolve-Path $Candidate).Path
        }
    }

    return $null
}

$ResolvedMpvSource = Resolve-MpvSource $MpvSource
if (-not $ResolvedMpvSource) {
    if ($AllowMissingMpv) {
        Write-Warning "No mpv source found; staging app without bundled mpv. Set MEDIAFLICK_DESKTOP_PACKAGE_MPV or MEDIAFLICK_DESKTOP_MPV_PATH to bundle mpv."
    } else {
        throw "No mpv source found. Set MEDIAFLICK_DESKTOP_PACKAGE_MPV to an mpv directory, or MEDIAFLICK_DESKTOP_MPV_PATH to mpv.exe."
    }
} else {
    $MpvSourceItem = Get-Item $ResolvedMpvSource
    $MpvRoot = if ($MpvSourceItem.PSIsContainer) { $MpvSourceItem.FullName } else { $MpvSourceItem.DirectoryName }
    $MpvExe = if ($MpvSourceItem.PSIsContainer) {
        Get-ChildItem $MpvRoot -Filter "mpv.exe" -Recurse -File | Select-Object -First 1
    } elseif ($MpvSourceItem.Name -ieq "mpv.exe") {
        $MpvSourceItem
    } else {
        Get-ChildItem $MpvRoot -Filter "mpv.exe" -File | Select-Object -First 1
    }

    if (-not $MpvExe) {
        throw "The mpv source '$ResolvedMpvSource' does not contain mpv.exe."
    }

    # Copy the directory that directly contains mpv.exe so the installed player is
    # always available as {app}\mpv\mpv.exe, which the application auto-detects.
    $MpvRoot = $MpvExe.DirectoryName
    $MpvDest = Join-Path $StagingPath "mpv"
    New-Item -ItemType Directory -Force $MpvDest | Out-Null
    Get-ChildItem -LiteralPath $MpvRoot -Force | Copy-Item -Destination $MpvDest -Recurse -Force
    Write-Host "Bundled mpv from $MpvRoot"
}

$Bytes = (Get-ChildItem $StagingPath -Recurse -File | Measure-Object -Property Length -Sum).Sum
$MiB = [math]::Round($Bytes / 1MB, 1)
Write-Host "Staged Windows release payload at $StagingPath ($MiB MiB)."
