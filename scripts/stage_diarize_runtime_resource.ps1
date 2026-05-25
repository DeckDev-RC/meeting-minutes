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

function Get-VenvPythonHome([string]$VenvPath) {
  $cfgPath = Join-Path $VenvPath "pyvenv.cfg"
  Require-File $cfgPath "pyvenv.cfg do runtime de diarizacao"

  $pythonHomeValue = $null
  foreach ($line in Get-Content -LiteralPath $cfgPath) {
    if ($line -match "^\s*home\s*=\s*(.+?)\s*$") {
      $pythonHomeValue = $Matches[1].Trim()
      break
    }
  }

  if ([string]::IsNullOrWhiteSpace($pythonHomeValue)) {
    throw "pyvenv.cfg nao contem o campo home: $cfgPath"
  }

  $homePath = Resolve-FullPath $pythonHomeValue
  Require-File (Join-Path $homePath "python.exe") "Python base do runtime de diarizacao"
  return $homePath
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

function Copy-PythonRuntime([string]$Source, [string]$Destination) {
  $sourceFull = Resolve-FullPath $Source
  $destinationFull = Resolve-FullPath $Destination
  New-Item -ItemType Directory -Force -Path $destinationFull | Out-Null
  $prefix = $sourceFull.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar

  Get-ChildItem -LiteralPath $sourceFull -Recurse -Force | ForEach-Object {
    $relative = $_.FullName.Substring($prefix.Length)
    $normalized = $relative.Replace("\", "/")
    if (
      $normalized -eq "Lib/site-packages" -or
      $normalized.StartsWith("Lib/site-packages/") -or
      $normalized -eq "Lib/test" -or
      $normalized.StartsWith("Lib/test/") -or
      $normalized -eq "Lib/idlelib" -or
      $normalized.StartsWith("Lib/idlelib/") -or
      $normalized -eq "Lib/tkinter" -or
      $normalized.StartsWith("Lib/tkinter/") -or
      $normalized -eq "Scripts" -or
      $normalized.StartsWith("Scripts/") -or
      $normalized -eq "Doc" -or
      $normalized.StartsWith("Doc/") -or
      $normalized -eq "include" -or
      $normalized.StartsWith("include/") -or
      $normalized -eq "libs" -or
      $normalized.StartsWith("libs/") -or
      $normalized -eq "Tools" -or
      $normalized.StartsWith("Tools/") -or
      $normalized -eq "tcl" -or
      $normalized.StartsWith("tcl/")
    ) {
      return
    }
    if (-not $_.PSIsContainer) {
      if (
        $_.Extension -eq ".pdb" -or
        $_.Name -match "_d\.(dll|exe|pyd)$" -or
        $_.Name -in @("python_d.exe", "pythonw_d.exe", "python311_d.dll", "python312_d.dll", "tcl86t.dll", "tk86t.dll")
      ) {
        return
      }
    }

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
$pythonHome = Get-VenvPythonHome $venvDir

if (Test-Path -LiteralPath $resourceDirPath) {
  Remove-Item -LiteralPath $resourceDirPath -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $resourceDirPath | Out-Null

Copy-PythonRuntime $pythonHome (Join-Path $resourceDirPath ".python")
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
    python = ".python/python.exe"
    sitePackages = ".venv-diarize/Lib/site-packages"
    backendScript = "scripts/diarize_cpu_backend.py"
    requirements = "scripts/requirements-diarize-cpu.txt"
  }
}
Write-Utf8NoBom (Join-Path $resourceDirPath "runtime-manifest.json") ($manifest | ConvertTo-Json -Depth 6)
Write-Utf8NoBom (Join-Path $resourceDirPath ".gitkeep") ""

$size = Get-ChildItem -LiteralPath $resourceDirPath -Recurse -Force -File | Measure-Object -Property Length -Sum
Write-Output "ResourceDir=$resourceDirPath"
Write-Output "ResourceBytes=$($size.Sum)"
