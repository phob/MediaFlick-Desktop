param(
    [string]$StagingDir = "dist/JellyfinMPV",
    [string]$InnoCompiler = $env:ISCC,
    [string]$Version
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    throw "The Windows installer script must be run on Windows."
}

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$StagingPath = Join-Path $RepoRoot $StagingDir
$InnoScript = Join-Path $RepoRoot "packaging\windows\jellyfin-mpv.iss"

if (-not (Test-Path (Join-Path $StagingPath "jellyfin-mpv.exe"))) {
    throw "Missing staged payload. Run 'just windows-dist' before building the installer."
}

if (-not $Version) {
    $CargoToml = Get-Content (Join-Path $RepoRoot "Cargo.toml") -Raw
    if ($CargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
        throw "Could not read package version from Cargo.toml."
    }
    $Version = $Matches[1]
}

function Resolve-InnoCompiler {
    param([string]$ExplicitCompiler)

    $Candidates = @()
    # Environment/explicit inputs win over local developer defaults.
    if ($ExplicitCompiler) { $Candidates += $ExplicitCompiler }
    $Candidates += "C:\Users\pho\AppData\Local\Programs\Inno Setup 6\ISCC.exe"

    $Command = Get-Command "ISCC.exe" -ErrorAction SilentlyContinue
    if ($Command) { $Candidates += $Command.Source }

    $Candidates += @(
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )

    foreach ($Candidate in $Candidates) {
        if ($Candidate -and (Test-Path $Candidate)) {
            return (Resolve-Path $Candidate).Path
        }
    }

    return $null
}

$Compiler = Resolve-InnoCompiler $InnoCompiler
if (-not $Compiler) {
    throw "Inno Setup compiler not found. Install Inno Setup 6 or set ISCC to the full path of ISCC.exe."
}

$OutputDir = Join-Path $RepoRoot "dist\installer"
New-Item -ItemType Directory -Force $OutputDir | Out-Null

& $Compiler "/DMyAppVersion=$Version" "/DSourceDir=$StagingPath" $InnoScript
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup failed with exit code $LASTEXITCODE."
}

$Installer = Join-Path $OutputDir "JellyfinMPV-Setup-$Version.exe"
Write-Host "Created installer: $Installer"
