param([int]$Iterations = 100)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$Ctl = Join-Path $Root "target\release\simple-stt-ctl.exe"
if (!(Test-Path $Ctl)) { throw "Build target\release\simple-stt-ctl.exe first" }
$Temp = Join-Path $env:TEMP ("simple-stt-bench-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null
$watch = [System.Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt $Iterations; $i++) {
  $out = Join-Path $Temp ("config-" + $i + ".txt")
  & $Ctl --output $out config-show | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "simple-stt-ctl failed at iteration $i" }
}
$watch.Stop()
$avg = $watch.Elapsed.TotalMilliseconds / $Iterations
"simple-stt-ctl config-show: {0:N2} ms average over {1} launches" -f $avg, $Iterations
"Temporary files: $Temp"
