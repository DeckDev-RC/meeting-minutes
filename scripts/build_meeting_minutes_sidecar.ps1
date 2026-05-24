param(
  [ValidateSet("onefile", "onedir")]
  [string]$Mode = "onefile",
  [ValidateSet("minimal", "diarize", "transcribe", "full")]
  [string]$Profile = "full",
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

$hiddenImports = @("diarize_cpu_backend", "transcribe_faster_whisper_backend")

if ($Profile -eq "diarize" -or $Profile -eq "full") {
  $hiddenImports += @(
    "diarize",
    "numpy._core._exceptions",
    "scipy._cyutility",
    "silero_vad",
    "soundfile",
    "torch",
    "torchaudio",
    "wespeakerruntime"
  )
}

if ($Profile -eq "transcribe" -or $Profile -eq "full") {
  $hiddenImports += @(
    "ctranslate2",
    "faster_whisper",
    "numpy._core._exceptions"
  )
}

foreach ($hiddenImport in $hiddenImports) {
  $args += @("--hidden-import", $hiddenImport)
}

$metadataPackages = @()

if ($Profile -eq "diarize" -or $Profile -eq "full") {
  $metadataPackages += @(
    "diarize",
    "numpy",
    "silero-vad",
    "soundfile",
    "torch",
    "torchaudio",
    "wespeakerruntime"
  )
}

if ($Profile -eq "transcribe" -or $Profile -eq "full") {
  $metadataPackages += @(
    "ctranslate2",
    "faster-whisper",
    "numpy"
  )
}

foreach ($metadataPackage in $metadataPackages) {
  $args += @("--copy-metadata", $metadataPackage)
}

$submodulePackages = @()

if ($Profile -eq "diarize" -or $Profile -eq "full") {
  $submodulePackages += @(
    "diarize",
    "numpy",
    "scipy",
    "sklearn",
    "silero_vad",
    "wespeakerruntime"
  )
}

if ($Profile -eq "transcribe" -or $Profile -eq "full") {
  $submodulePackages += @(
    "ctranslate2",
    "faster_whisper",
    "numpy"
  )
}

foreach ($submodulePackage in $submodulePackages) {
  $args += @("--collect-submodules", $submodulePackage)
}

$dataPackages = @()

if ($Profile -eq "diarize" -or $Profile -eq "full") {
  $dataPackages += @(
    "silero_vad",
    "wespeakerruntime"
  )
}

if ($Profile -eq "transcribe" -or $Profile -eq "full") {
  $dataPackages += @(
    "faster_whisper"
  )
}

foreach ($dataPackage in $dataPackages) {
  $args += @("--collect-data", $dataPackage)
}

if ($Mode -eq "onefile") {
  $args += "--onefile"
} else {
  $args += "--onedir"
}

$args += $sidecar

& $Python -m PyInstaller @args

Write-Host "Sidecar build completed in $distPath"
