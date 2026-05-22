param(
  [string]$RunStamp = (Get-Date -Format 'yyyyMMdd-HHmmss'),
  [switch]$SkipBuild,
  [switch]$ReuseBaselineTranscription,
  [string]$OnlyMeeting = ""
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$CargoManifest = Join-Path $ProjectRoot "src-tauri\Cargo.toml"
$RunsRoot = Join-Path $ProjectRoot "benchmarks\runs\matrix-$RunStamp"
$SummaryCsv = Join-Path $RunsRoot "summary.csv"
$SummaryMd = Join-Path $RunsRoot "summary.md"

$Meetings = @(
  @{
    Key = "slow-2026-05-08"
    Source = "C local 2026-05-08 15-48-29"
    Input = "C:\2026-05-08 15-48-29.mp4"
  },
  @{
    Key = "fast-2026-05-15"
    Source = "Videos 2026-05-15 10-05-59"
    Input = Join-Path $env:USERPROFILE "Videos\2026-05-15 10-05-59.mp4"
  }
)

if (-not [string]::IsNullOrWhiteSpace($OnlyMeeting)) {
  $Meetings = @($Meetings | Where-Object { $_.Key -eq $OnlyMeeting })
  if ($Meetings.Count -eq 0) {
    throw "No meeting matched OnlyMeeting=$OnlyMeeting"
  }
}

$VariantPlan = @(
  @{
    Name = "thinking0"
    Args = @()
    ThinkingBudget = "0"
  },
  @{
    Name = "local-ata"
    Args = @("--minutes-mode", "local")
    ThinkingBudget = $null
  },
  @{
    Name = "chunks-240-thinking0-local"
    Args = @("--target-sec", "240", "--min-sec", "120", "--max-sec", "360", "--minutes-mode", "local")
    ThinkingBudget = "0"
  },
  @{
    Name = "chunks-180-thinking0-local"
    Args = @("--target-sec", "180", "--min-sec", "90", "--max-sec", "270", "--minutes-mode", "local")
    ThinkingBudget = "0"
  },
  @{
    Name = "modern-cpu-expected"
    Args = @("--diarization-mode", "modern-cpu")
    ThinkingBudget = $null
    NeedsExpectedSpeakers = $true
  },
  @{
    Name = "modern-cpu-chunked-expected"
    Args = @("--diarization-mode", "modern-cpu-chunked")
    ThinkingBudget = $null
    NeedsExpectedSpeakers = $true
  }
)

function Invoke-BenchmarkCase {
  param(
    [hashtable]$Meeting,
    [string]$VariantName,
    [string[]]$ExtraArgs = @(),
    [string]$ThinkingBudget = $null,
    [int]$ExpectedSpeakers = 0,
    [string]$ReuseTranscriptionFrom = $null
  )

  if (-not (Test-Path $Meeting.Input)) {
    throw "Input file not found: $($Meeting.Input)"
  }

  $caseId = "$($Meeting.Key)-$VariantName"
  $outDir = Join-Path $RunsRoot $caseId
  $logPath = Join-Path $outDir "benchmark.log"
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null

  if ([string]::IsNullOrWhiteSpace($ThinkingBudget)) {
    Remove-Item Env:MEETING_MINUTES_GEMINI_THINKING_BUDGET -ErrorAction SilentlyContinue
  } else {
    $env:MEETING_MINUTES_GEMINI_THINKING_BUDGET = $ThinkingBudget
  }

  $args = @(
    "run", "--release",
    "--manifest-path", $CargoManifest,
    "--bin", "e2e_benchmark",
    "--",
    "--input", $Meeting.Input,
    "--id", $caseId,
    "--out-dir", $outDir,
    "--source", "$($Meeting.Source) $VariantName",
    "--transcribe-concurrency", "4",
    "--facts-concurrency", "3"
  )
  if ($ExpectedSpeakers -gt 0) {
    $args += @("--expected-speakers", "$ExpectedSpeakers")
  }
  if (-not [string]::IsNullOrWhiteSpace($ReuseTranscriptionFrom)) {
    $args += @("--reuse-transcription-from", $ReuseTranscriptionFrom)
  }
  $args += $ExtraArgs

  Write-Host "[$caseId] start"
  $started = Get-Date
  $previousErrorActionPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = "Continue"
    & cargo @args *> $logPath
    $exitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previousErrorActionPreference
    Remove-Item Env:MEETING_MINUTES_GEMINI_THINKING_BUDGET -ErrorAction SilentlyContinue
  }
  $elapsed = ((Get-Date) - $started).TotalSeconds

  if ($exitCode -ne 0) {
    Write-Warning "[$caseId] failed after $([math]::Round($elapsed, 2))s"
    return [pscustomobject]@{
      Id = $caseId
      Meeting = $Meeting.Key
      Variant = $VariantName
      Status = "failed"
      WallClockSec = $null
      SpeedX = $null
      Chunks = $null
      Speakers = $null
      Decisions = $null
      Actions = $null
      DiarizationSec = $null
      FactsSec = $null
      MinutesSec = $null
      Report = ""
      Log = $logPath
    }
  }

  $reportPath = Join-Path $outDir "benchmark-report.json"
  $report = Get-Content $reportPath -Raw | ConvertFrom-Json
  Write-Host "[$caseId] done wall=$([math]::Round($report.wallClockSec, 2))s speed=$([math]::Round($report.speedX, 2))x"

  $stageByName = @{}
  foreach ($stage in $report.stages) {
    $stageByName[$stage.name] = [double]$stage.durationSec
  }

  return [pscustomobject]@{
    Id = $caseId
    Meeting = $Meeting.Key
    Variant = $VariantName
    Status = "ok"
    WallClockSec = [double]$report.wallClockSec
    SpeedX = [double]$report.speedX
    Chunks = [int]$report.chunkCount
    Speakers = [int]$report.speakerCount
    Decisions = [int]$report.decisionCount
    Actions = [int]$report.actionCount
    DiarizationSec = $stageByName["diarize_speculative"]
    FactsSec = $stageByName["extract_facts_parallel"]
    MinutesSec = $stageByName["generate_minutes"]
    Report = $reportPath
    Log = $logPath
  }
}

New-Item -ItemType Directory -Force -Path $RunsRoot | Out-Null
Set-Location $ProjectRoot

if (-not $SkipBuild) {
  Write-Host "[build] cargo build --release --bin e2e_benchmark"
  & cargo build --release --manifest-path $CargoManifest --bin e2e_benchmark
  if ($LASTEXITCODE -ne 0) {
    throw "Release build failed."
  }
}

$results = New-Object System.Collections.Generic.List[object]

foreach ($meeting in $Meetings) {
  $baseline = Invoke-BenchmarkCase -Meeting $meeting -VariantName "baseline"
  $results.Add($baseline)

  $expectedSpeakers = $null
  if ($baseline.Status -eq "ok" -and $baseline.Speakers -gt 0) {
    $expectedSpeakers = [int]$baseline.Speakers
  }
  $reuseTranscriptionFrom = $null
  if ($ReuseBaselineTranscription -and $baseline.Status -eq "ok") {
    $reuseTranscriptionFrom = Split-Path -Parent $baseline.Report
  }

  foreach ($variant in $VariantPlan) {
    $caseExpected = $null
    if ($variant.NeedsExpectedSpeakers -and $null -ne $expectedSpeakers) {
      $caseExpected = $expectedSpeakers
    }
    $result = Invoke-BenchmarkCase `
      -Meeting $meeting `
      -VariantName $variant.Name `
      -ExtraArgs $variant.Args `
      -ThinkingBudget $variant.ThinkingBudget `
      -ExpectedSpeakers $caseExpected `
      -ReuseTranscriptionFrom $reuseTranscriptionFrom
    $results.Add($result)
  }
}

$results | Export-Csv -NoTypeInformation -Encoding UTF8 -Path $SummaryCsv

$md = New-Object System.Collections.Generic.List[string]
$md.Add("# E2E Benchmark Matrix $RunStamp")
$md.Add("")
$md.Add("| Meeting | Variant | Status | Wall | Speed | Chunks | Speakers | Diarization | Facts | Minutes | Decisions | Actions |")
$md.Add("| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
foreach ($item in $results) {
  $wall = if ($null -eq $item.WallClockSec) { "" } else { "{0:N2}s" -f $item.WallClockSec }
  $speed = if ($null -eq $item.SpeedX) { "" } else { "{0:N2}x" -f $item.SpeedX }
  $diar = if ($null -eq $item.DiarizationSec) { "" } else { "{0:N2}s" -f $item.DiarizationSec }
  $facts = if ($null -eq $item.FactsSec) { "" } else { "{0:N2}s" -f $item.FactsSec }
  $minutes = if ($null -eq $item.MinutesSec) { "" } else { "{0:N2}s" -f $item.MinutesSec }
  $md.Add("| $($item.Meeting) | $($item.Variant) | $($item.Status) | $wall | $speed | $($item.Chunks) | $($item.Speakers) | $diar | $facts | $minutes | $($item.Decisions) | $($item.Actions) |")
}
$md.Add("")
$md.Add("CSV: $SummaryCsv")
$md.Add("Reports: $RunsRoot")
$md | Set-Content -Encoding UTF8 -Path $SummaryMd

Write-Host "[summary] $SummaryMd"
