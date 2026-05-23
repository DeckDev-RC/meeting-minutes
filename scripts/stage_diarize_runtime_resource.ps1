param(
  [string]$ProjectRoot = (Get-Location).Path,
  [string]$ResourceDir = "",
  [string]$Version = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Resolve-FullPath([string]$PathValue) {
  return [System.IO.Path]::GetFullPath($PathValue)
}

function Require-File([string]$PathValue, [string]$Label) {
  if (-not (Test-Path -LiteralPath $PathValue -PathType Leaf)) {
    throw "$Label nao encontrado: $PathValue"
  }
}

function Remove-TransientPythonFiles([string]$Root) {
  Get-ChildItem -LiteralPath $Root -Recurse -Directory -Force -Filter "__pycache__" -ErrorAction SilentlyContinue |
    Remove-Item -Recurse -Force
  Get-ChildItem -LiteralPath $Root -Recurse -File -Force -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -in @(".pyc", ".pyo") } |
    Remove-Item -Force
}

function Copy-DirectoryTree([string]$Source, [string]$Destination) {
  $sourceFull = Resolve-FullPath $Source
  $destinationFull = Resolve-FullPath $Destination
  New-Item -ItemType Directory -Force -Path $destinationFull | Out-Null
  $prefix = $sourceFull.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar

  Get-ChildItem -LiteralPath $sourceFull -Recurse -Force | ForEach-Object {
    $relative = $_.FullName.Substring($prefix.Length)
    $target = Join-Path $destinationFull $relative
    if ($_.PSIsContainer) {
      New-Item -ItemType Directory -Force -Path $target | Out-Null
    } else {
      New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
      [System.IO.File]::Copy($_.FullName, $target, $true)
    }
  }
}

function Copy-FileTo([string]$Source, [string]$Destination) {
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Destination) | Out-Null
  [System.IO.File]::Copy((Resolve-FullPath $Source), (Resolve-FullPath $Destination), $true)
}

function Write-Utf8NoBom([string]$PathValue, [string]$Content) {
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText((Resolve-FullPath $PathValue), $Content, $encoding)
}

$projectRootPath = Resolve-FullPath $ProjectRoot
if (-not (Test-Path -LiteralPath $projectRootPath -PathType Container)) {
  throw "ProjectRoot nao encontrado: $projectRootPath"
}

if ([string]::IsNullOrWhiteSpace($Version)) {
  $packagePath = Join-Path $projectRootPath "package.json"
  Require-File $packagePath "package.json"
  $Version = (Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json).version
}

if ([string]::IsNullOrWhiteSpace($ResourceDir)) {
  $ResourceDir = Join-Path $projectRootPath "src-tauri\resources\diarize"
}
$resourceDirPath = Resolve-FullPath $ResourceDir

$venvDir = Join-Path $projectRootPath ".venv-diarize"
$pythonExe = Join-Path $venvDir "Scripts\python.exe"
$backendScript = Join-Path $projectRootPath "scripts\diarize_cpu_backend.py"
$requirements = Join-Path $projectRootPath "scripts\requirements-diarize-cpu.txt"

if (-not (Test-Path -LiteralPath $venvDir -PathType Container)) {
  throw "Ambiente .venv-diarize nao encontrado: $venvDir. Rode npm run setup:diarize-cpu antes."
}
Require-File $pythonExe "Python do runtime de diarizacao"
Require-File $backendScript "Backend Python de diarizacao"
Require-File $requirements "requirements de diarizacao"

if (Test-Path -LiteralPath $resourceDirPath) {
  Remove-Item -LiteralPath $resourceDirPath -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $resourceDirPath | Out-Null

Copy-DirectoryTree $venvDir (Join-Path $resourceDirPath ".venv-diarize")
New-Item -ItemType Directory -Force -Path (Join-Path $resourceDirPath "scripts") | Out-Null
Copy-FileTo $backendScript (Join-Path $resourceDirPath "scripts\diarize_cpu_backend.py")
Copy-FileTo $requirements (Join-Path $resourceDirPath "scripts\requirements-diarize-cpu.txt")

Remove-TransientPythonFiles $resourceDirPath

$manifest = [ordered]@{
  name = "meeting-minutes-diarize-runtime"
  version = $Version
  platform = "windows-x64"
  createdAtUtc = (Get-Date).ToUniversalTime().ToString("o")
  layout = [ordered]@{
    python = ".venv-diarize/Scripts/python.exe"
    backendScript = "scripts/diarize_cpu_backend.py"
    requirements = "scripts/requirements-diarize-cpu.txt"
  }
}
Write-Utf8NoBom (Join-Path $resourceDirPath "runtime-manifest.json") ($manifest | ConvertTo-Json -Depth 6)
Write-Utf8NoBom (Join-Path $resourceDirPath ".gitkeep") ""

$size = Get-ChildItem -LiteralPath $resourceDirPath -Recurse -Force -File | Measure-Object -Property Length -Sum
Write-Output "ResourceDir=$resourceDirPath"
Write-Output "ResourceBytes=$($size.Sum)"
