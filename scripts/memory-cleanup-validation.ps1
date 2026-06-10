param(
    [int]$IdleSeconds = 5,
    [int]$ShellPid = 0,
    [switch]$KeepProcesses
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$CaptureExe = Join-Path $Root 'target\release\simple-stt-capture.exe'
$CtlExe = Join-Path $Root 'target\release\simple-stt-ctl.exe'
$InferExe = Join-Path $Root 'target\release\simple-stt-infer.exe'
foreach ($Path in @($CaptureExe,$CtlExe,$InferExe)) { if (-not (Test-Path -LiteralPath $Path)) { throw "Missing release binary: $Path. Run .\scripts\build-release.ps1 first." } }
$Artifacts = Join-Path $Root 'artifacts'
New-Item -ItemType Directory -Force -Path $Artifacts | Out-Null
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$RunDir = Join-Path $Artifacts "memory-run-$Stamp"
New-Item -ItemType Directory -Force -Path $RunDir | Out-Null
$Config = Join-Path $RunDir 'config.json'
$State = Join-Path $RunDir 'capture-state.json'
$Token = [Guid]::NewGuid().ToString('N')
$env:SIMPLE_STT_CONFIG = $Config
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Read-TabResponse([string]$Path) {
    $Values = @{}
    $Events = @()
    $Ok = $false
    $Message = ''
    foreach ($Line in Get-Content -LiteralPath $Path -Encoding UTF8) {
        $Parts = $Line -split "`t", -1
        switch ($Parts[0]) {
            'status' { $Ok = $Parts[1] -eq 'ok' }
            'message' { $Message = $Parts[1] }
            'value' { if ($Parts.Count -ge 3) { $Values[$Parts[1]] = $Parts[2] } }
            'event' { if ($Parts.Count -ge 6) { $Events += [ordered]@{ seq=$Parts[1]; kind=$Parts[2]; session_id=$Parts[3]; level=$Parts[4]; text=$Parts[5] } } }
        }
    }
    return [ordered]@{ ok=$Ok; message=$Message; values=$Values; events=$Events }
}
function Invoke-Ctl([string[]]$Arguments) {
    $Output = Join-Path $RunDir ("ctl-" + [Guid]::NewGuid().ToString('N') + '.txt')
    & $CtlExe --state-file $State --token $Token --output $Output @Arguments
    if (-not (Test-Path -LiteralPath $Output)) { throw "simple-stt-ctl did not create response file" }
    $Response = Read-TabResponse $Output
    Remove-Item -LiteralPath $Output -Force
    if (-not $Response.ok) { throw "simple-stt-ctl failed: $($Response.message)" }
    return $Response
}
function Snapshot-Process([int]$Id) {
    if (-not $Id) { return $null }
    $P = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if (-not $P) { return $null }
    return [ordered]@{ pid=$P.Id; name=$P.ProcessName; working_set_bytes=$P.WorkingSet64; private_memory_bytes=$P.PrivateMemorySize64; timestamp=(Get-Date).ToString('o') }
}
function Snapshot-Gpu {
    if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) { return [ordered]@{ available=$false } }
    $Raw = & nvidia-smi --query-compute-apps=pid,process_name,used_gpu_memory --format=csv,noheader,nounits 2>&1
    return [ordered]@{ available=$true; lines=@($Raw); timestamp=(Get-Date).ToString('o') }
}
function Wait-State([int]$TimeoutMs = 10000) {
    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $Deadline) { if (Test-Path -LiteralPath $State) { return }; Start-Sleep -Milliseconds 100 }
    throw "capture state file was not published: $State"
}
function Wait-WorkerPid([int]$TimeoutMs = 120000) {
    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $Deadline) {
        $Ping = Invoke-Ctl @('ping')
        if ($Ping.values.ContainsKey('worker_pid')) { return [int]$Ping.values['worker_pid'] }
        Start-Sleep -Milliseconds 250
    }
    throw "infer worker PID did not appear"
}
function Wait-ProcessExit([int]$Id, [int]$TimeoutMs = 15000) {
    $Deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $Deadline) { if (-not (Get-Process -Id $Id -ErrorAction SilentlyContinue)) { return $true }; Start-Sleep -Milliseconds 100 }
    return $false
}

# Create and patch isolated schema-v2 config without a resident service.
$ResetOut = Join-Path $RunDir 'config-reset.txt'
& $CtlExe --output $ResetOut config-reset
$SaveIn = Join-Path $RunDir 'config-save.txt'
[System.IO.File]::WriteAllLines(
    $SaveIn,
    @("idle_worker_timeout_secs`t$IdleSeconds", "worker_shutdown_grace_ms`t2000"),
    $Utf8NoBom
)
$SaveOut = Join-Path $RunDir 'config-save-result.txt'
& $CtlExe --output $SaveOut config-save --input $SaveIn
$SaveResponse = Read-TabResponse $SaveOut
if (-not $SaveResponse.ok) { throw "unable to prepare isolated config: $($SaveResponse.message)" }

$Evidence = [ordered]@{ schema='simple-stt-memory-validation-v1'; started=(Get-Date).ToString('o'); idle_seconds=$IdleSeconds; shell_before=(Snapshot-Process $ShellPid); gpu_before=(Snapshot-Gpu) }
$Capture = Start-Process -FilePath $CaptureExe -ArgumentList @('--token',$Token,'--state-file',$State,'--config',$Config) -PassThru -WindowStyle Hidden
try {
    Wait-State
    $null = Invoke-Ctl @('ping')
    $Evidence.capture_baseline = Snapshot-Process $Capture.Id
    $Evidence.gpu_capture_baseline = Snapshot-Gpu
    $null = Invoke-Ctl @('test-model')
    $WorkerPid = Wait-WorkerPid
    $Evidence.worker_loaded = Snapshot-Process $WorkerPid
    $Evidence.gpu_worker_loaded = Snapshot-Gpu
    $LatestSeq = 0
    $Deadline = [DateTime]::UtcNow.AddMinutes(5)
    do {
        Start-Sleep -Milliseconds 250
        $Poll = Invoke-Ctl @('poll-events','--after-seq',"$LatestSeq")
        if ($Poll.values.ContainsKey('latest_seq')) { $LatestSeq = [int]$Poll.values['latest_seq'] }
        $Done = $Poll.events | Where-Object { $_.kind -in @('model_test_complete','notice') }
    } while (-not $Done -and [DateTime]::UtcNow -lt $Deadline)
    $Evidence.after_smoke_test = [ordered]@{ capture=(Snapshot-Process $Capture.Id); worker=(Snapshot-Process $WorkerPid); gpu=(Snapshot-Gpu); terminal_events=@($Done) }
    $null = Invoke-Ctl @('unload-model')
    $Exited = Wait-ProcessExit $WorkerPid 15000
    $Evidence.after_unload = [ordered]@{ worker_pid=$WorkerPid; worker_exited=$Exited; worker=(Snapshot-Process $WorkerPid); capture=(Snapshot-Process $Capture.Id); gpu=(Snapshot-Gpu) }
    if (-not $Exited) { throw "infer worker $WorkerPid did not exit after unload request" }
    $Evidence.completed = (Get-Date).ToString('o')
} finally {
    if (-not $KeepProcesses -and -not $Capture.HasExited) {
        try { $null = Invoke-Ctl @('shutdown') } catch { Write-Warning $_ }
        if (-not $Capture.WaitForExit(3000)) { Stop-Process -Id $Capture.Id -Force }
    }
}
$Output = Join-Path $Artifacts "memory-cleanup-validation-$Stamp.json"
[System.IO.File]::WriteAllText(
    $Output,
    (($Evidence | ConvertTo-Json -Depth 8) + [Environment]::NewLine),
    $Utf8NoBom
)
Write-Host "Validation evidence: $Output"
Write-Host "Worker exited: $($Evidence.after_unload.worker_exited)"
Write-Host "Capture baseline WS: $($Evidence.capture_baseline.working_set_bytes)"
Write-Host "Capture after cleanup WS: $($Evidence.after_unload.capture.working_set_bytes)"
