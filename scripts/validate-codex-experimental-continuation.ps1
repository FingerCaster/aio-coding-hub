param(
  [int]$TargetTriggered = 5,
  [int]$BatchSize = 5,
  [int]$MaxAttempts = 60,
  [int]$PerAttemptTimeoutSec = 300,
  [switch]$UseDebugGateway,
  [switch]$KeepDebugGateway,
  [switch]$KeepWindowVisible,
  [string]$Model = "gpt-5.5",
  [string]$ReasoningEffort = "xhigh",
  [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
  [string]$SettingsPath = (Join-Path $env:USERPROFILE ".aio-coding-hub\settings.json"),
  [string]$DbPath = (Join-Path $env:USERPROFILE ".aio-coding-hub\aio-coding-hub.db"),
  [string]$Sqlite = $(if ($env:AIO_SQLITE_EXE) { $env:AIO_SQLITE_EXE } else { "D:\Android\SDK\platform-tools\sqlite3.exe" }),
  [string]$InstalledExe = "D:\Program Files\AIO Coding Hub\aio-coding-hub.exe",
  [string]$DebugExe = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")).Path "src-tauri\target\debug\aio-coding-hub.exe"),
  [string]$CodexExe = $(if ($env:CODEX_EXE) { $env:CODEX_EXE } else { "C:\Users\Administrator\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe" }),
  [string]$PromptPath = (Join-Path $PSScriptRoot "codex-candy-prompt.txt"),
  [string]$ArtifactDir = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..")).Path (".omx\artifacts\live-codex-experimental-continuation-" + (Get-Date -Format "yyyyMMddTHHmmss")))
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)

function Write-Utf8NoBomFile([string]$Path, [string]$Value) {
  $parent = Split-Path -Parent $Path
  if ($parent -and -not (Test-Path $parent)) {
    New-Item -ItemType Directory -Path $parent | Out-Null
  }
  $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
  [System.IO.File]::WriteAllText($Path, $Value, $utf8NoBom)
}

function Write-JsonFile([string]$Path, [object]$Value, [int]$Depth = 20) {
  Write-Utf8NoBomFile -Path $Path -Value ($Value | ConvertTo-Json -Depth $Depth)
}

function Write-ProgressLine([string]$Message) {
  $existing = ""
  if (Test-Path $script:ProgressLog) {
    $existing = Get-Content -Raw $script:ProgressLog
  }
  Write-Utf8NoBomFile -Path $script:ProgressLog -Value ($existing + "$(Get-Date -Format o) $Message`n")
}

function Write-SettingsJson([object]$Settings) {
  Write-JsonFile -Path $SettingsPath -Value $Settings -Depth 100
}

function Convert-ValidationResultSummaries($Results) {
  @($Results | ForEach-Object {
    $guard = $_.GuardRow
    [pscustomobject]@{
      Attempt = $_.Attempt
      SessionId = $_.SessionId
      ExitCode = $_.ExitCode
      TimedOut = $_.TimedOut
      Triggered = $_.Triggered
      Repaired = $_.Repaired
      Has21 = $_.Has21
      CountedSuccess = $_.CountedSuccess
      BadTriggered = $_.BadTriggered
      GuardId = if ($guard) { $guard.id } else { $null }
      GuardStatus = if ($guard) { $guard.status } else { $null }
      GuardOutcome = if ($guard) { $guard.guardOutcome } else { $null }
      ContinuationSentRounds = if ($guard) { $guard.continuationSentRounds } else { $null }
      ReasoningTokens = if ($guard) { $guard.reasoningTokens } else { $null }
      RequestReasoningEffort = if ($guard) { $guard.requestReasoningEffort } else { $null }
      EventsPath = $_.EventsPath
      LastMessagePath = $_.LastMessagePath
    }
  })
}

function Join-ProcessArguments([object[]]$Arguments) {
  ($Arguments | ForEach-Object {
    '"' + ([string]$_).Replace('"', '\"') + '"'
  }) -join " "
}

function Get-PortOwner() {
  $conn = Get-NetTCPConnection -LocalPort 37123 -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -eq $conn) { return $null }
  try { return Get-Process -Id $conn.OwningProcess -ErrorAction Stop } catch { return $null }
}

function Wait-Port([int]$TimeoutSec) {
  $deadline = (Get-Date).AddSeconds($TimeoutSec)
  while ((Get-Date) -lt $deadline) {
    $owner = Get-PortOwner
    if ($null -ne $owner) { return $owner }
    Start-Sleep -Milliseconds 500
  }
  return $null
}

function Stop-PortOwner([string]$Reason) {
  $owner = Get-PortOwner
  if ($null -eq $owner) { return }
  Write-ProgressLine "stopping port 37123 owner pid=$($owner.Id) path=$($owner.Path) reason=$Reason"
  try {
    Stop-Process -Id $owner.Id -ErrorAction Stop
    Start-Sleep -Seconds 2
  } catch {
    Write-ProgressLine "graceful stop failed pid=$($owner.Id): $($_.Exception.Message)"
  }
  $still = Get-Process -Id $owner.Id -ErrorAction SilentlyContinue
  if ($null -ne $still) {
    Write-ProgressLine "forcing stop pid=$($owner.Id)"
    Stop-Process -Id $owner.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 1
  }
}

function Get-MaxRequestId() {
  $value = & $Sqlite -batch -noheader $DbPath "select coalesce(max(id),0) from request_logs;"
  if ([string]::IsNullOrWhiteSpace($value)) { return 0 }
  return [int64]$value.Trim()
}

function Query-GuardRow([string]$SessionId, [int64]$StartId) {
  if ([string]::IsNullOrWhiteSpace($SessionId)) { return $null }
  $safeSession = $SessionId.Replace("'", "''")
  $sql = @"
select r.id, r.status, r.error_code, r.session_id,
json_extract(g.value,'$.guardStrategyOutcome') as guardOutcome,
json_extract(g.value,'$.continuationSentRounds') as continuationSentRounds,
json_extract(g.value,'$.continuationFailureKind') as continuationFailureKind,
json_extract(g.value,'$.strategyReason') as strategyReason,
json_extract(g.value,'$.guardPostMatchStrategy') as guardPostMatchStrategy,
json_extract(g.value,'$.reasoningTokens') as reasoningTokens,
json_extract(g.value,'$.actionTaken') as actionTaken,
json_extract(g.value,'$.guardRetryPhase') as guardRetryPhase,
json_extract(g.value,'$.visibleAssemblyKind') as visibleAssemblyKind,
(select json_extract(e.value,'$.effort')
 from json_each(r.special_settings_json) e
 where json_extract(e.value,'$.type')='codex_reasoning_effort'
 order by cast(e.key as integer) desc
 limit 1) as requestReasoningEffort,
(select json_extract(e.value,'$.rawEffort')
 from json_each(r.special_settings_json) e
 where json_extract(e.value,'$.type')='codex_reasoning_effort'
 order by cast(e.key as integer) desc
 limit 1) as rawRequestReasoningEffort
from request_logs r, json_each(r.special_settings_json) g
where r.id > $StartId
and r.cli_key='codex'
and r.session_id='$safeSession'
and json_extract(g.value,'$.type')='codex_reasoning_guard'
and json_extract(g.value,'$.guardPostMatchStrategy')='continuation_repair_experimental'
order by r.id desc limit 1;
"@
  $json = & $Sqlite -json $DbPath $sql
  if ([string]::IsNullOrWhiteSpace($json) -or $json.Trim() -eq "[]") { return $null }
  $rows = $json | ConvertFrom-Json
  if ($rows -is [array]) { return $rows[0] }
  return $rows
}

function Dump-RequestRows([int64]$StartId) {
  $sql = @"
select r.id, r.status, r.error_code, r.session_id, r.created_at_ms, r.requested_model, r.output_tokens,
json_extract(g.value,'$.type') as settingType,
json_extract(g.value,'$.guardPostMatchStrategy') as guardPostMatchStrategy,
json_extract(g.value,'$.guardStrategyOutcome') as guardOutcome,
json_extract(g.value,'$.continuationSentRounds') as continuationSentRounds,
json_extract(g.value,'$.continuationFailureKind') as continuationFailureKind,
json_extract(g.value,'$.strategyReason') as strategyReason,
json_extract(g.value,'$.reasoningTokens') as reasoningTokens,
json_extract(g.value,'$.visibleAssemblyKind') as visibleAssemblyKind,
json_extract(g.value,'$.effort') as requestReasoningEffort,
json_extract(g.value,'$.rawEffort') as rawRequestReasoningEffort
from request_logs r
left join json_each(r.special_settings_json) g
where r.id > $StartId
and r.cli_key='codex'
and (g.value is null or json_extract(g.value,'$.type') in ('codex_reasoning_guard','codex_reasoning_continuation','codex_reasoning_features','codex_reasoning_effort'))
order by r.id asc;
"@
  Write-Utf8NoBomFile -Path (Join-Path $ArtifactDir "request-rows.json") -Value (& $Sqlite -json $DbPath $sql)
}

function Start-CodexAttempt([int]$AttemptNo, [string]$Prompt) {
  $eventsPath = Join-Path $ArtifactDir "run-$AttemptNo-events.jsonl"
  $lastPath = Join-Path $ArtifactDir "run-$AttemptNo-last-message.txt"
  $stderrPath = Join-Path $ArtifactDir "run-$AttemptNo-stderr.txt"
  $metaPath = Join-Path $ArtifactDir "run-$AttemptNo-meta.json"
  Write-Utf8NoBomFile -Path (Join-Path $ArtifactDir "run-$AttemptNo-prompt.txt") -Value $Prompt

  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $CodexExe
  $psi.Arguments = Join-ProcessArguments @("exec", "--json", "-m", $Model, "-c", "model_reasoning_effort=`"$ReasoningEffort`"", "-o", $lastPath, "--sandbox", "read-only", "--cd", $Root, $Prompt)
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.StandardOutputEncoding = [System.Text.UTF8Encoding]::new($false)
  $psi.StandardErrorEncoding = [System.Text.UTF8Encoding]::new($false)
  $proc = [System.Diagnostics.Process]::new()
  $proc.StartInfo = $psi
  $startedAt = Get-Date
  [void]$proc.Start()
  [pscustomobject]@{
    Attempt = $AttemptNo
    Process = $proc
    StartedAt = $startedAt
    Deadline = $startedAt.AddSeconds($PerAttemptTimeoutSec)
    StdoutTask = $proc.StandardOutput.ReadToEndAsync()
    StderrTask = $proc.StandardError.ReadToEndAsync()
    EventsPath = $eventsPath
    LastMessagePath = $lastPath
    StderrPath = $stderrPath
    MetaPath = $metaPath
    Collected = $false
  }
}

function Collect-Attempt($Record, [bool]$TimedOut) {
  if ($Record.Collected) { return }
  if ($TimedOut) {
    try { $Record.Process.Kill($true) } catch { try { $Record.Process.Kill() } catch {} }
  }
  try { $Record.Process.WaitForExit() } catch {}
  $endedAt = Get-Date
  try { $stdout = $Record.StdoutTask.Result } catch { $stdout = "" }
  try { $stderr = $Record.StderrTask.Result } catch { $stderr = "" }
  if ($null -eq $stdout) { $stdout = "" }
  if ($null -eq $stderr) { $stderr = "" }
  Write-Utf8NoBomFile -Path $Record.EventsPath -Value $stdout
  Write-Utf8NoBomFile -Path $Record.StderrPath -Value $stderr
  Write-JsonFile -Path $Record.MetaPath -Depth 10 -Value ([pscustomobject]@{
    Attempt = $Record.Attempt
    TimedOut = $TimedOut
    ExitCode = if ($TimedOut) { $null } else { $Record.Process.ExitCode }
    Pid = $Record.Process.Id
    DurationSeconds = [math]::Round(($endedAt - $Record.StartedAt).TotalSeconds, 3)
    EventsPath = $Record.EventsPath
    LastMessagePath = $Record.LastMessagePath
    StderrPath = $Record.StderrPath
    Model = $Model
    ReasoningEffort = $ReasoningEffort
  })
  $Record.Collected = $true
}

function Start-DebugGateway() {
  if (-not (Test-Path $DebugExe)) { throw "debug exe not found: $DebugExe" }
  $script:OldWatchdogEnv = $env:AIO_CODING_HUB_DISABLE_HEARTBEAT_WATCHDOG
  $env:AIO_CODING_HUB_DISABLE_HEARTBEAT_WATCHDOG = "1"
  Write-Utf8NoBomFile -Path (Join-Path $ArtifactDir "debug-env.txt") -Value "AIO_CODING_HUB_DISABLE_HEARTBEAT_WATCHDOG=1`n"
  if ($KeepWindowVisible) {
    $process = Start-Process -FilePath $DebugExe -PassThru
  } else {
    $process = Start-Process -FilePath $DebugExe -WindowStyle Hidden -PassThru
  }
  Write-ProgressLine "started debug app pid=$($process.Id) env_disable_watchdog=1"
  $owner = Wait-Port 60
  if ($null -eq $owner) { throw "debug app did not listen on 37123 within 60s" }
  Write-ProgressLine "debug gateway ready pid=$($owner.Id) path=$($owner.Path)"
}

function Restore-WatchdogEnv() {
  if ($null -eq $script:OldWatchdogEnv) {
    Remove-Item Env:\AIO_CODING_HUB_DISABLE_HEARTBEAT_WATCHDOG -ErrorAction SilentlyContinue
  } else {
    $env:AIO_CODING_HUB_DISABLE_HEARTBEAT_WATCHDOG = $script:OldWatchdogEnv
  }
}

if (-not (Test-Path $PromptPath)) { throw "prompt file not found: $PromptPath" }
if (-not (Test-Path $SettingsPath)) { throw "settings file not found: $SettingsPath" }
if (-not (Test-Path $DbPath)) { throw "database not found: $DbPath" }
if (-not (Test-Path $Sqlite)) { throw "sqlite executable not found: $Sqlite" }
if (-not (Test-Path $CodexExe)) { throw "codex executable not found: $CodexExe" }

New-Item -ItemType Directory -Path $ArtifactDir -Force | Out-Null
$script:ProgressLog = Join-Path $ArtifactDir "progress.log"
$script:OldWatchdogEnv = $null
$prompt = Get-Content -Path $PromptPath -Raw -Encoding UTF8
$originalSettingsRaw = [System.IO.File]::ReadAllText($SettingsPath, [System.Text.Encoding]::UTF8)
Write-Utf8NoBomFile -Path (Join-Path $ArtifactDir "settings.original.json") -Value $originalSettingsRaw
$startId = 0
$allResults = @()
$success = $false
$failureReason = $null

try {
  Write-ProgressLine "artifact=$ArtifactDir"
  Write-ProgressLine "codex model=$Model reasoning_effort=$ReasoningEffort"
  $startId = Get-MaxRequestId
  Write-Utf8NoBomFile -Path (Join-Path $ArtifactDir "start-id.txt") -Value "$startId`n"
  Write-ProgressLine "start request id=$startId"

  if ($UseDebugGateway) {
    Stop-PortOwner "switch-to-debug-build"
  }

  $settings = $originalSettingsRaw | ConvertFrom-Json
  $settings.codex_reasoning_guard_enabled = $true
  $settings.codex_reasoning_guard_post_match_strategy = "continuation_repair_experimental"
  $settings.codex_reasoning_guard_continuation_repair_enabled = $true
  $settings.codex_reasoning_guard_concurrent_max = 5
  $settings.codex_reasoning_guard_concurrent_max_attempts = 10
  $settings.codex_reasoning_guard_rule_mode = "reasoning_tokens"
  $settings.codex_reasoning_guard_compare_mode = "equals"
  $settings.codex_reasoning_guard_reasoning_equals = @(516, 1034, 1552)
  if ($KeepWindowVisible) {
    $settings.start_minimized = $false
  } else {
    $settings.start_minimized = $true
  }
  $settings.tray_enabled = $true
  Write-SettingsJson -Settings $settings
  Write-ProgressLine "guard enabled with experimental continuation repair"

  if ($UseDebugGateway) {
    Start-DebugGateway
  } else {
    $owner = Wait-Port 30
    if ($null -eq $owner) { throw "gateway did not listen on 37123 within 30s" }
    Write-ProgressLine "using current gateway pid=$($owner.Id) path=$($owner.Path)"
  }

  $attempt = 1
  while ($attempt -le $MaxAttempts) {
    $remainingAttempts = $MaxAttempts - $attempt + 1
    $thisBatch = [math]::Min($BatchSize, $remainingAttempts)
    Write-ProgressLine "starting batch attempts=$attempt..$($attempt + $thisBatch - 1)"
    $records = @()
    for ($offset = 0; $offset -lt $thisBatch; $offset += 1) {
      $records += Start-CodexAttempt -AttemptNo ($attempt + $offset) -Prompt $prompt
      Write-ProgressLine "started attempt=$($attempt + $offset) pid=$($records[-1].Process.Id)"
    }

    while (@($records | Where-Object { -not $_.Collected }).Count -gt 0) {
      foreach ($record in @($records | Where-Object { -not $_.Collected })) {
        if ($record.Process.HasExited) {
          Collect-Attempt $record $false
        } elseif ((Get-Date) -ge $record.Deadline) {
          Collect-Attempt $record $true
        }
      }
      Start-Sleep -Milliseconds 500
    }
    Start-Sleep -Seconds 3

    foreach ($record in $records) {
      $events = if (Test-Path $record.EventsPath) { Get-Content -Path $record.EventsPath -Raw -Encoding UTF8 } else { "" }
      $answer = if (Test-Path $record.LastMessagePath) { Get-Content -Path $record.LastMessagePath -Raw -Encoding UTF8 } else { "" }
      $meta = if (Test-Path $record.MetaPath) { Get-Content -Path $record.MetaPath -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }
      if ($null -eq $events) { $events = "" }
      if ($null -eq $answer) { $answer = "" }
      $sessionId = $null
      $match = [regex]::Match($events, '"thread_id"\s*:\s*"([^"\\]+)"')
      if ($match.Success) { $sessionId = $match.Groups[1].Value }
      $guard = Query-GuardRow -SessionId $sessionId -StartId $startId
      $triggered = $null -ne $guard
      $repaired = $triggered -and $guard.guardOutcome -eq "continuation_repaired" -and [int]$guard.status -eq 200
      $effortMatches = $triggered -and $guard.requestReasoningEffort -eq $ReasoningEffort
      $has21 = [regex]::IsMatch($answer, '(^|[^0-9])21([^0-9]|$)')
      $exitCode = if ($null -ne $meta) { $meta.ExitCode } else { $null }
      $timedOut = if ($null -ne $meta) { [bool]$meta.TimedOut } else { $true }
      $counted = $triggered -and $repaired -and $effortMatches -and $has21 -and ($exitCode -eq 0) -and (-not $timedOut)
      $badTriggered = $triggered -and (-not $counted)
      $result = [pscustomobject]@{
        Attempt = $record.Attempt
        SessionId = $sessionId
        ExitCode = $exitCode
        TimedOut = $timedOut
        Triggered = $triggered
        Repaired = $repaired
        EffortMatches = $effortMatches
        Has21 = $has21
        CountedSuccess = $counted
        BadTriggered = $badTriggered
        GuardRow = $guard
        Answer = $answer
        EventsPath = $record.EventsPath
        LastMessagePath = $record.LastMessagePath
      }
      $allResults += $result
      $guardDesc = if ($triggered) { "guard=$($guard.guardOutcome) status=$($guard.status) tokens=$($guard.reasoningTokens) effort=$($guard.requestReasoningEffort)" } else { "guard=none" }
      Write-ProgressLine "attempt=$($record.Attempt) session=$sessionId exit=$exitCode timeout=$timedOut $guardDesc effortMatches=$effortMatches has21=$has21 counted=$counted badTriggered=$badTriggered"
    }

    Dump-RequestRows -StartId $startId
    $triggeredCount = @($allResults | Where-Object { $_.Triggered }).Count
    $successCount = @($allResults | Where-Object { $_.CountedSuccess }).Count
    $badCount = @($allResults | Where-Object { $_.BadTriggered }).Count
    Write-JsonFile -Path (Join-Path $ArtifactDir "summary.partial.json") -Depth 30 -Value ([pscustomobject]@{
      ArtifactDir = $ArtifactDir
      StartId = $startId
      AttemptsRun = $allResults.Count
      TriggeredCount = $triggeredCount
      CountedSuccess = $successCount
      BadTriggeredCount = $badCount
      TargetTriggered = $TargetTriggered
      ReasoningEffort = $ReasoningEffort
      Complete = $successCount -ge $TargetTriggered -and $badCount -eq 0
      Results = Convert-ValidationResultSummaries $allResults
    })

    if ($badCount -gt 0) {
      $failureReason = "bad triggered attempts observed: $badCount"
      Write-ProgressLine $failureReason
      break
    }
    if ($successCount -ge $TargetTriggered) {
      $success = $true
      Write-ProgressLine "success criterion met: countedSuccess=$successCount triggered=$triggeredCount"
      break
    }
    $attempt += $thisBatch
  }

  if (-not $success -and [string]::IsNullOrWhiteSpace($failureReason)) {
    $failureReason = "insufficient counted successes after $($allResults.Count) attempts"
    Write-ProgressLine $failureReason
  }
} catch {
  $failureReason = $_.Exception.Message
  Write-ProgressLine "fatal: $failureReason"
} finally {
  try { Dump-RequestRows -StartId $startId } catch { Write-ProgressLine "request row dump failed: $($_.Exception.Message)" }
  try {
    $restore = $originalSettingsRaw | ConvertFrom-Json
    $restore.codex_reasoning_guard_enabled = $false
    if ($KeepWindowVisible) {
      $restore.start_minimized = $false
    }
    Write-SettingsJson -Settings $restore
    Write-ProgressLine "settings restored with codex_reasoning_guard_enabled=false and start_minimized=$($restore.start_minimized)"
  } catch {
    Write-ProgressLine "settings restore failed: $($_.Exception.Message)"
  }
  if ($UseDebugGateway -and $KeepDebugGateway) {
    try {
      $owner = Wait-Port 5
      if ($null -eq $owner) {
        Write-ProgressLine "debug gateway missing after restore; starting debug app"
        Start-DebugGateway
      } elseif ($owner.Path -ne $DebugExe) {
        Stop-PortOwner "replace-non-debug-owner-after-validation"
        Start-DebugGateway
      } else {
        Write-ProgressLine "keeping debug gateway pid=$($owner.Id) path=$($owner.Path)"
      }
    } catch {
      Write-ProgressLine "keep debug gateway failed: $($_.Exception.Message)"
    }
    Restore-WatchdogEnv
  } elseif ($UseDebugGateway) {
    try { Stop-PortOwner "restore-installed-build" } catch { Write-ProgressLine "debug stop failed: $($_.Exception.Message)" }
    Restore-WatchdogEnv
    try {
      $installed = Start-Process -FilePath $InstalledExe -WindowStyle Hidden -PassThru
      Write-ProgressLine "started installed app pid=$($installed.Id)"
      $owner = Wait-Port 30
      if ($null -ne $owner) { Write-ProgressLine "post-restore port owner pid=$($owner.Id) path=$($owner.Path)" }
    } catch {
      Write-ProgressLine "installed app restart failed: $($_.Exception.Message)"
    }
  } else {
    Restore-WatchdogEnv
  }
  $triggeredCount = @($allResults | Where-Object { $_.Triggered }).Count
  $successCount = @($allResults | Where-Object { $_.CountedSuccess }).Count
  $badCount = @($allResults | Where-Object { $_.BadTriggered }).Count
  Write-JsonFile -Path (Join-Path $ArtifactDir "summary.json") -Depth 30 -Value ([pscustomobject]@{
    ArtifactDir = $ArtifactDir
    StartId = $startId
    AttemptsRun = $allResults.Count
    TriggeredCount = $triggeredCount
    CountedSuccess = $successCount
    BadTriggeredCount = $badCount
    Success = $success
    FailureReason = $failureReason
    ReasoningEffort = $ReasoningEffort
    Results = Convert-ValidationResultSummaries $allResults
  })
  Write-ProgressLine "done success=$success triggered=$triggeredCount counted=$successCount bad=$badCount reason=$failureReason"
}

if (-not $success) {
  exit 1
}
