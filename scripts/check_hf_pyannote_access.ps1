$ErrorActionPreference = "Stop"

$processToken = [Environment]::GetEnvironmentVariable("HF_TOKEN", "Process")
$userToken = [Environment]::GetEnvironmentVariable("HF_TOKEN", "User")
$token = if ($processToken) { $processToken } else { $userToken }

Write-Output ("ProcessHFToken=" + [bool]$processToken)
Write-Output ("UserHFToken=" + [bool]$userToken)

if (-not $token) {
  Write-Output "Result=missing-token"
  Write-Output "Action=Set HF_TOKEN with setx HF_TOKEN <token> and open a new terminal."
  exit 2
}

try {
  $who = Invoke-WebRequest `
    -UseBasicParsing `
    -Uri "https://huggingface.co/api/whoami-v2" `
    -Headers @{ Authorization = "Bearer $token" } `
    -TimeoutSec 30
  Write-Output ("WhoamiStatus=" + [int]$who.StatusCode)
} catch {
  $status = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { "no-response" }
  Write-Output ("WhoamiStatus=error-" + $status)
  Write-Output "Result=invalid-token"
  exit 2
}

try {
  $model = Invoke-WebRequest `
    -UseBasicParsing `
    -Uri "https://huggingface.co/api/models/pyannote/speaker-diarization-community-1" `
    -Headers @{ Authorization = "Bearer $token" } `
    -TimeoutSec 30
  Write-Output ("ModelApiStatus=" + [int]$model.StatusCode)
} catch {
  $status = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { "no-response" }
  Write-Output ("ModelApiStatus=error-" + $status)
}

try {
  $config = Invoke-WebRequest `
    -UseBasicParsing `
    -Uri "https://huggingface.co/pyannote/speaker-diarization-community-1/resolve/main/config.yaml" `
    -Headers @{ Authorization = "Bearer $token" } `
    -TimeoutSec 30
  Write-Output ("PyannoteConfigStatus=" + [int]$config.StatusCode)
  Write-Output ("PyannoteConfigBytes=" + $config.RawContentLength)
  Write-Output "Result=ok"
  exit 0
} catch {
  $status = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { "no-response" }
  Write-Output ("PyannoteConfigStatus=error-" + $status)
  if ($status -eq 403) {
    Write-Output "Result=forbidden"
    Write-Output "Action=In the Hugging Face token settings, enable read access to public gated repositories or explicitly allow pyannote/speaker-diarization-community-1. If you created a new token, run setx HF_TOKEN <new_token> and restart the terminal/app."
  } else {
    Write-Output "Result=download-check-failed"
  }
  exit 2
}
