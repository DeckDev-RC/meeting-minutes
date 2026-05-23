param(
  [Parameter(Mandatory = $true)]
  [string]$PackPath,
  [string]$InstallRoot = "",
  [switch]$NoEnv
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

function Assert-SafeInstallRoot([string]$PathValue) {
  $full = Resolve-FullPath $PathValue
  $root = [System.IO.Path]::GetPathRoot($full)
  if ([string]::IsNullOrWhiteSpace($full) -or $full -eq $root) {
    throw "InstallRoot inseguro: $full"
  }
  $parent = Split-Path -Parent $full
  if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $full) {
    throw "InstallRoot sem diretorio pai valido: $full"
  }
  return $full
}

function Write-Utf8NoBom([string]$PathValue, [string]$Content) {
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText((Resolve-FullPath $PathValue), $Content, $encoding)
}

function Remove-InstallRootSafely([string]$PathValue) {
  if (-not (Test-Path -LiteralPath $PathValue)) {
    return
  }

  Get-ChildItem -LiteralPath $PathValue -Force | ForEach-Object {
    if (($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
      if ($_.PSIsContainer) {
        [System.IO.Directory]::Delete($_.FullName, $false)
      } else {
        [System.IO.File]::Delete($_.FullName)
      }
    } else {
      Remove-Item -LiteralPath $_.FullName -Recurse -Force
    }
  }
  Remove-Item -LiteralPath $PathValue -Force
}

$packFullPath = Resolve-FullPath $PackPath
Require-File $packFullPath "Runtime pack"

if ([string]::IsNullOrWhiteSpace($InstallRoot)) {
  if ([string]::IsNullOrWhiteSpace($env:APPDATA)) {
    throw "APPDATA nao esta definido; informe -InstallRoot."
  }
  $InstallRoot = Join-Path $env:APPDATA "com.agregar.meeting-minutes\runtime\diarize"
}
$installRootPath = Assert-SafeInstallRoot $InstallRoot
$installParent = Split-Path -Parent $installRootPath
$extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) "meeting-minutes-diarize-runtime-install-$([guid]::NewGuid())"

try {
  New-Item -ItemType Directory -Force -Path $extractRoot | Out-Null
  Expand-Archive -LiteralPath $packFullPath -DestinationPath $extractRoot -Force

  $manifestPath = Join-Path $extractRoot "runtime-manifest.json"
  $pythonPath = Join-Path $extractRoot ".venv-diarize\Scripts\python.exe"
  $scriptPath = Join-Path $extractRoot "scripts\diarize_cpu_backend.py"
  Require-File $manifestPath "Manifest do runtime"
  Require-File $pythonPath "Python do runtime"
  Require-File $scriptPath "Backend Python de diarizacao"

  $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
  if ($manifest.name -ne "meeting-minutes-diarize-runtime") {
    throw "Runtime pack invalido: nome inesperado '$($manifest.name)'."
  }

  New-Item -ItemType Directory -Force -Path $installParent | Out-Null
  Remove-InstallRootSafely $installRootPath
  New-Item -ItemType Directory -Force -Path $installRootPath | Out-Null

  Get-ChildItem -LiteralPath $extractRoot -Force | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $installRootPath -Recurse -Force
  }

  $envConfigured = -not $NoEnv
  if ($envConfigured) {
    [Environment]::SetEnvironmentVariable("MEETING_MINUTES_DIARIZE_ROOT", $installRootPath, "User")
  }

  $installedManifest = [ordered]@{
    name = "meeting-minutes-diarize-runtime-install"
    sourcePack = $packFullPath
    installRoot = $installRootPath
    installedAtUtc = (Get-Date).ToUniversalTime().ToString("o")
    envConfigured = $envConfigured
    runtime = $manifest
  }
  Write-Utf8NoBom (Join-Path $installRootPath "installed-manifest.json") ($installedManifest | ConvertTo-Json -Depth 8)

  Write-Output "InstallRoot=$installRootPath"
  Write-Output "EnvConfigured=$envConfigured"
} finally {
  if (Test-Path -LiteralPath $extractRoot) {
    Remove-Item -LiteralPath $extractRoot -Recurse -Force
  }
}
