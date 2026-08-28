param(
    [string]$StagingDir = "dist/MediaFlickDesktop"
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    throw "The Windows release staging script must be run on Windows."
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BuildDir = Join-Path $RepoRoot "build"
$StagingPath = Join-Path $RepoRoot $StagingDir
$LibmpvDir = Join-Path $BuildDir "libmpv-windows-x64"

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

$LibmpvSource = Join-Path $LibmpvDir "libmpv-2.dll"
if (-not (Test-Path $LibmpvSource)) {
    throw "Missing bundled libmpv runtime: build/libmpv-windows-x64/libmpv-2.dll. Run the libmpv build first."
}
Copy-Item $LibmpvSource -Destination (Join-Path $StagingPath "libmpv-2.dll") -Force

$LibmpvNotices = Join-Path $StagingPath "licenses/libmpv"
New-Item -ItemType Directory -Force $LibmpvNotices | Out-Null
foreach ($File in @("LICENSE.mpv-GPL", "LICENSE.mpv-LGPL", "Copyright.mpv", "THIRD-PARTY-NOTICES.md", "SOURCE-REVISIONS.txt", "libmpv-2.dll.sha256")) {
    $Source = Join-Path $LibmpvDir $File
    if (-not (Test-Path $Source)) {
        throw "Missing bundled libmpv notice: build/libmpv-windows-x64/$File"
    }
    Copy-Item $Source -Destination $LibmpvNotices -Force
}

$LocalesSource = Join-Path $BuildDir "locales"
if (-not (Test-Path $LocalesSource)) {
    throw "Required CEF locales directory is missing from build/."
}
Copy-Item $LocalesSource -Destination (Join-Path $StagingPath "locales") -Recurse -Force

$Bytes = (Get-ChildItem $StagingPath -Recurse -File | Measure-Object -Property Length -Sum).Sum
$MiB = [math]::Round($Bytes / 1MB, 1)
Write-Host "Staged Windows release payload at $StagingPath ($MiB MiB)."
