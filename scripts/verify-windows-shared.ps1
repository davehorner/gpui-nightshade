param(
    [int]$Frames = 1200,
    [int]$Port = 19790,
    [int]$TimeoutSeconds = 600
)
$ErrorActionPreference = 'Stop'
if ($Frames -lt 600) { throw 'Use at least 600 frames to cover both scenes.' }
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target/debug/gpui-nightshade-viewer.exe'
if (!(Test-Path -LiteralPath $exe)) { throw 'Build gpui-nightshade-viewer first.' }
$run = Join-Path $root ('target/shared-validation/' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$log = Join-Path $run 'stderr.log'
$oldTrace = $env:GPUI_NIGHTSHADE_TRACE_GPU
$oldLayout = $env:GPUI_NIGHTSHADE_LAYOUT
$process = $null
$published = 0
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,$Port)
try { $listener.Start() } finally { $listener.Stop() }
function Invoke-ViewerTool([string]$Name, $Arguments) {
    $body = @{jsonrpc='2.0'; id=1; method='tools/call'; params=@{name=$Name; arguments=$Arguments}} | ConvertTo-Json -Depth 12 -Compress
    $response = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/" -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 3
    if ($response.error) { throw ($response.error | ConvertTo-Json -Compress) }
    if ($response.result.isError) { throw ($response.result | ConvertTo-Json -Depth 8 -Compress) }
    return $response.result.structuredContent
}
try {
    $env:GPUI_NIGHTSHADE_TRACE_GPU = '1'
    $env:GPUI_NIGHTSHADE_LAYOUT = Join-Path $run 'layout.json'
    # This is an interactive compositor test: the viewer window must be shown,
    # otherwise visibility suspension correctly prevents all presentation work.
    $process = Start-Process -FilePath $exe -ArgumentList @('--presentation=windows-shared',"--agent-port=$Port") -WorkingDirectory $root -WindowStyle Normal -PassThru -RedirectStandardError $log -RedirectStandardOutput (Join-Path $run 'stdout.log')
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $ready = $false
    $switched = $false
    $peakWorkingSet = 0L
    $lastReported = 0
    while ((Get-Date) -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "Viewer exited with code $($process.ExitCode); see $log" }
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
        [string]$logText = Get-Content -LiteralPath $log -Raw -ErrorAction SilentlyContinue
        if ($logText -match 'worker stopped|device status: Err|panicked|D3D11_MESSAGE_SEVERITY\(1\)') {
            throw "GPU validation reported a failure; see $log"
        }
        $progressMatches = [regex]::Matches($logText, 'Shared GPU progress: published=(\d+)')
        if ($progressMatches.Count) { $published = [int]$progressMatches[$progressMatches.Count - 1].Groups[1].Value }
        if (!$ready) {
            try { $null = Invoke-ViewerTool 'get_scene' @{}; $ready = $true } catch { }
        }
        if ($published -ge 300 -and !$switched -and $ready) {
            $null = Invoke-ViewerTool 'load_file' @{path=(Join-Path $root 'upstream/nightshade/assets/gltf/fox.glb')}
            $switched = $true
        }
        if ($published -gt $lastReported) {
            Write-Output "Published shared GPU frames: $published"
            $lastReported = $published
        }
        if ($published -ge $Frames -and $switched) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($published -lt $Frames) { throw "Timed out at $published published frames; see $log" }
    $animation = Invoke-ViewerTool 'get_animation' @{}
    if (!$animation.playing -or $animation.clips.Count -lt 3) { throw 'Fox animation was not loaded/playing.' }
    Start-Sleep -Milliseconds 200
    $next = Invoke-ViewerTool 'get_animation' @{}
    if ($next.time -eq $animation.time) { throw 'Animation time did not advance.' }
    $report = @{status='passed'; backend='windows-shared'; cpuReadbackFallback=$false; publishedFrames=$published; resolution='see stderr progress'; peakProcessWorkingSetBytes=$peakWorkingSet; animationClips=$next.clips.Count; log=$log}
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'report.json')
    Write-Output "Shared-GPU validation passed: $run"
} catch {
    @{status='failed'; backend='windows-shared'; publishedFrames=$published; error=$_.Exception.Message; log=$log} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'report.json')
    throw
} finally {
    if ($process -and !$process.HasExited) { $process.Kill() }
    $env:GPUI_NIGHTSHADE_TRACE_GPU = $oldTrace
    $env:GPUI_NIGHTSHADE_LAYOUT = $oldLayout
}
