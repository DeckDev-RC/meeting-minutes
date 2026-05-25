param(
  [string]$ProjectRoot = (Get-Location).Path,
  [string]$ResourceDir = "",
  [string]$Version = "",
  [string]$Model = "turbo",
  [string]$IncludeModel = "true"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Resolve-FullPath([string]$PathValue) {
  return [System.IO.Path]::GetFullPath($PathValue)
}

$projectRootPath = Resolve-FullPath $ProjectRoot
if ([string]::IsNullOrWhiteSpace($ResourceDir)) {
  $ResourceDir = Join-Path $projectRootPath "src-tauri\resources\transcribe"
}
$resourceDirPath = Resolve-FullPath $ResourceDir
$tempOutDir = Join-Path ([System.IO.Path]::GetTempPath()) "meeting-minutes-transcribe-stage-$([guid]::NewGuid())"

try {
  New-Item -ItemType Directory -Force -Path $tempOutDir | Out-Null
  $packArgs = @(
    "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $projectRootPath "scripts\build_transcribe_runtime_pack.ps1"),
    "-ProjectRoot", $projectRootPath,
    "-OutDir", $tempOutDir,
    "-Model", $Model,
    "-IncludeModel", $IncludeModel
  )
  if (-not [string]::IsNullOrWhiteSpace($Version)) {
    $packArgs += @("-Version", $Version)
  }
  powershell @packArgs
  if ($LASTEXITCODE -ne 0) {
    throw "Falha ao criar pacote de runtime de transcricao"
  }

  $packPath = Join-Path $tempOutDir "meeting-minutes-transcribe-runtime-windows-x64.zip"
  if (-not (Test-Path -LiteralPath $packPath -PathType Leaf)) {
    throw "Pacote de runtime nao encontrado: $packPath"
  }

  if (Test-Path -LiteralPath $resourceDirPath) {
    Remove-Item -LiteralPath $resourceDirPath -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $resourceDirPath | Out-Null
  Expand-Archive -LiteralPath $packPath -DestinationPath $resourceDirPath -Force
  New-Item -ItemType File -Force -Path (Join-Path $resourceDirPath ".gitkeep") | Out-Null

  $size = Get-ChildItem -LiteralPath $resourceDirPath -Recurse -Force -File | Measure-Object -Property Length -Sum
  Write-Output "ResourceDir=$resourceDirPath"
  Write-Output "ResourceBytes=$($size.Sum)"
} finally {
  if (Test-Path -LiteralPath $tempOutDir) {
    Remove-Item -LiteralPath $tempOutDir -Recurse -Force
  }
}
