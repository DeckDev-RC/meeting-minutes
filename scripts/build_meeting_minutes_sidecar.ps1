param(
  [ValidateSet("onefile", "onedir")]
  [string]$Mode = "onefile",
  [string]$Python = "python",
  [string]$OutDir = "dist\sidecar"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$sidecar = Join-Path $repoRoot "scripts\meeting_minutes_sidecar.py"
$distPath = Join-Path $repoRoot $OutDir
$buildPath = Join-Path $repoRoot "build\meeting-minutes-sidecar"

if (!(Test-Path $sidecar)) {
  throw "Sidecar entrypoint not found: $sidecar"
}

& $Python -m PyInstaller --version | Out-Null

$args = @(
  "--name", "meeting-minutes-sidecar",
  "--distpath", $distPath,
  "--workpath", $buildPath,
  "--paths", (Join-Path $repoRoot "scripts"),
  "--clean",
  "--noconfirm"
)

if ($Mode -eq "onefile") {
  $args += "--onefile"
} else {
  $args += "--onedir"
}

$args += $sidecar

& $Python -m PyInstaller @args

Write-Host "Sidecar build completed in $distPath"
