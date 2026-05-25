param(
  [string]$ProjectRoot = (Get-Location).Path,
  [string]$OutDir = "",
  [string]$Model = "turbo",
  [string]$IncludeModel = "true",
  [switch]$SkipTranscribeSetup
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Resolve-FullPath([string]$PathValue) {
  return [System.IO.Path]::GetFullPath($PathValue)
}

function Clear-TranscribeResource([string]$ProjectRootPath) {
  $resourceDir = Resolve-FullPath (Join-Path $ProjectRootPath "src-tauri\resources\transcribe")
  $expectedParent = Resolve-FullPath (Join-Path $ProjectRootPath "src-tauri\resources")
  if (-not $resourceDir.StartsWith($expectedParent, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Diretorio de recurso inseguro: $resourceDir"
  }
  if (Test-Path -LiteralPath $resourceDir) {
    Remove-Item -LiteralPath $resourceDir -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $resourceDir | Out-Null
  New-Item -ItemType File -Force -Path (Join-Path $resourceDir ".gitkeep") | Out-Null
}

$projectRootPath = Resolve-FullPath $ProjectRoot
if ([string]::IsNullOrWhiteSpace($OutDir)) {
  $OutDir = Join-Path $projectRootPath "dist\offline-bundle"
}
$outDirPath = Resolve-FullPath $OutDir
New-Item -ItemType Directory -Force -Path $outDirPath | Out-Null

try {
  if (-not $SkipTranscribeSetup -and -not (Test-Path -LiteralPath (Join-Path $projectRootPath ".venv-transcribe\Scripts\python.exe") -PathType Leaf)) {
    npm run setup:transcribe-local
    if ($LASTEXITCODE -ne 0) {
      throw "setup:transcribe-local falhou"
    }
  }

  powershell -ExecutionPolicy Bypass -File (Join-Path $projectRootPath "scripts\stage_transcribe_runtime_resource.ps1") `
    -ProjectRoot $projectRootPath `
    -Model $Model `
    -IncludeModel $IncludeModel
  if ($LASTEXITCODE -ne 0) {
    throw "stage_transcribe_runtime_resource falhou"
  }

  npm run tauri:build
  if ($LASTEXITCODE -ne 0) {
    throw "tauri:build falhou"
  }

  $bundleRoot = Join-Path $projectRootPath "src-tauri\target\release\bundle"
  $artifacts = Get-ChildItem -LiteralPath $bundleRoot -Recurse -File -Include *.exe,*.msi -ErrorAction SilentlyContinue
  foreach ($artifact in $artifacts) {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($artifact.Name)
    $extension = $artifact.Extension
    $target = Join-Path $outDirPath "$name-offline$extension"
    Copy-Item -LiteralPath $artifact.FullName -Destination $target -Force
    Write-Output "OfflineBundle=$target"
  }
} finally {
  Clear-TranscribeResource $projectRootPath
}
