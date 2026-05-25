param(
  [string]$ProjectRoot = (Get-Location).Path,
  [string]$OutDir = "",
  [string]$Version = "",
  [string]$Model = "turbo",
  [string]$IncludeModel = "true"
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
  Require-File $cfgPath "pyvenv.cfg do runtime de transcricao"

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
  Require-File (Join-Path $homePath "python.exe") "Python base do runtime de transcricao"
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

function Get-Sha256Hex([string]$PathValue) {
  $stream = [System.IO.File]::OpenRead((Resolve-FullPath $PathValue))
  try {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
      $hash = $sha.ComputeHash($stream)
      return ([System.BitConverter]::ToString($hash)).Replace("-", "").ToLowerInvariant()
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

$projectRootPath = Resolve-FullPath $ProjectRoot
$includeModelFlag = $IncludeModel -notmatch "^(false|0|no)$"
if (-not (Test-Path -LiteralPath $projectRootPath -PathType Container)) {
  throw "ProjectRoot nao encontrado: $projectRootPath"
}

if ([string]::IsNullOrWhiteSpace($Version)) {
  $packagePath = Join-Path $projectRootPath "package.json"
  Require-File $packagePath "package.json"
  $Version = (Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json).version
}

if ([string]::IsNullOrWhiteSpace($OutDir)) {
  $OutDir = Join-Path $projectRootPath "dist\runtime"
}
$outDirPath = Resolve-FullPath $OutDir
New-Item -ItemType Directory -Force -Path $outDirPath | Out-Null

$venvDir = Join-Path $projectRootPath ".venv-transcribe"
$pythonExe = Join-Path $venvDir "Scripts\python.exe"
$backendScript = Join-Path $projectRootPath "scripts\transcribe_faster_whisper_backend.py"
$requirements = Join-Path $projectRootPath "scripts\requirements-transcribe-local.txt"

if (-not (Test-Path -LiteralPath $venvDir -PathType Container)) {
  throw "Ambiente .venv-transcribe nao encontrado: $venvDir. Rode npm run setup:transcribe-local antes."
}
Require-File $pythonExe "Python do runtime de transcricao"
Require-File $backendScript "Backend Python de transcricao"
Require-File $requirements "requirements de transcricao"
$pythonHome = Get-VenvPythonHome $venvDir

$safeVersion = ($Version -replace "[^0-9A-Za-z._-]", "-")
$versionedPackPath = Join-Path $outDirPath "meeting-minutes-transcribe-runtime-$safeVersion-windows-x64.zip"
$stablePackPath = Join-Path $outDirPath "meeting-minutes-transcribe-runtime-windows-x64.zip"
$stagingRoot = Join-Path ([System.IO.Path]::GetTempPath()) "meeting-minutes-transcribe-runtime-pack-$([guid]::NewGuid())"

try {
  New-Item -ItemType Directory -Force -Path $stagingRoot | Out-Null

  Copy-PythonRuntime $pythonHome (Join-Path $stagingRoot ".python")
  Copy-DirectoryTree $venvDir (Join-Path $stagingRoot ".venv-transcribe")
  New-Item -ItemType Directory -Force -Path (Join-Path $stagingRoot "scripts") | Out-Null
  Copy-FileTo $backendScript (Join-Path $stagingRoot "scripts\transcribe_faster_whisper_backend.py")
  Copy-FileTo $requirements (Join-Path $stagingRoot "scripts\requirements-transcribe-local.txt")

  $modelLayout = $null
  if ($includeModelFlag) {
    $modelDir = Join-Path $stagingRoot "models\faster-whisper-turbo"
    New-Item -ItemType Directory -Force -Path $modelDir | Out-Null
    $downloadScript = Join-Path $stagingRoot "download_faster_whisper_model.py"
    Write-Utf8NoBom $downloadScript @"
from faster_whisper.utils import download_model
download_model("$Model", output_dir=r"$modelDir")
"@
    & $pythonExe $downloadScript
    if ($LASTEXITCODE -ne 0) {
      throw "Falha ao baixar o modelo faster-whisper $Model"
    }
    Remove-Item -LiteralPath $downloadScript -Force
    $modelLayout = "models/faster-whisper-turbo"
  }

  Remove-TransientPythonFiles $stagingRoot

  $manifest = [ordered]@{
    name = "meeting-minutes-transcribe-runtime"
    version = $Version
    platform = "windows-x64"
    createdAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    layout = [ordered]@{
      python = ".python/python.exe"
      sitePackages = ".venv-transcribe/Lib/site-packages"
      backendScript = "scripts/transcribe_faster_whisper_backend.py"
      requirements = "scripts/requirements-transcribe-local.txt"
      fasterWhisperModel = $modelLayout
    }
  }
  Write-Utf8NoBom (Join-Path $stagingRoot "runtime-manifest.json") ($manifest | ConvertTo-Json -Depth 6)

  Add-Type -AssemblyName System.IO.Compression.FileSystem
  foreach ($packPath in @($versionedPackPath, $stablePackPath)) {
    if (Test-Path -LiteralPath $packPath) {
      Remove-Item -LiteralPath $packPath -Force
    }
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
      $stagingRoot,
      $packPath,
      [System.IO.Compression.CompressionLevel]::Optimal,
      $false
    )
    $pack = Get-Item -LiteralPath $packPath
    Write-Output "PackPath=$($pack.FullName)"
    Write-Output "PackBytes=$($pack.Length)"
    Write-Output "PackSha256=$(Get-Sha256Hex $pack.FullName)"
  }
} finally {
  if (Test-Path -LiteralPath $stagingRoot) {
    Remove-Item -LiteralPath $stagingRoot -Recurse -Force
  }
}
