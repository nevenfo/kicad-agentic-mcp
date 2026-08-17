<#
.SYNOPSIS
    Run the live-KiCad PCB suites end to end with no human at the keyboard.

.DESCRIPTION
    J.3.1 asked whether the PCB path needs a live GUI *session*. The answer this
    script encodes, measured on KiCad 10.0.3 / Windows 11:

      * KiCad does not hand `KICAD_API_SOCKET` to a process it did not spawn.
        It does not have to: the API server listens on a *deterministic* path,
        `%LOCALAPPDATA%\Temp\kicad\api.sock`, exposed as the Windows named pipe
        `\\.\pipe\<that path>`. An external process discovers it by construction
        and never has to read it out of the Preferences dialog.
      * `KICAD_API_TOKEN` may be empty. KiCad supplies a token to plugins it
        launches itself; it does not require one from other clients.
      * No interaction is required — pcbnew is started, driven and stopped by
        this script. A *desktop session* is still required, because pcbnew is a
        GUI binary and has no headless mode. That is the platform constraint;
        see docs/TROUBLESHOOTING.md.

    The script starts pcbnew on a throwaway copy of the board fixture, waits for
    the pipe, runs both live suites, then stops pcbnew. Exit code is the first
    failing suite's, or 0.

.PARAMETER Pcbnew
    Path to pcbnew.exe. Defaults to a sibling of $env:KICAD_CLI, then to the
    usual install roots.

.PARAMETER Board
    Board to open. Defaults to a throwaway copy of the checked-in fixture, so a
    run never dirties the working tree — the suites save the open board.

.PARAMETER TimeoutSeconds
    How long to wait for the API pipe. Default 90.
#>
[CmdletBinding()]
param(
    [string]$Pcbnew,
    [string]$Board,
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Resolve-Pcbnew {
    param([string]$Explicit)
    if ($Explicit) {
        if (-not (Test-Path $Explicit)) { throw "pcbnew.exe not found at $Explicit" }
        return $Explicit
    }
    if ($env:KICAD_CLI) {
        $sibling = Join-Path (Split-Path $env:KICAD_CLI) 'pcbnew.exe'
        if (Test-Path $sibling) { return $sibling }
    }
    foreach ($root in @(
            "$env:LOCALAPPDATA\Programs\KiCad",
            "$env:ProgramFiles\KiCad",
            'C:\KiCad')) {
        if (-not (Test-Path $root)) { continue }
        $found = Get-ChildItem $root -Recurse -Filter pcbnew.exe -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName
        if ($found) { return $found }
    }
    throw 'pcbnew.exe not found. Pass -Pcbnew or set KICAD_CLI.'
}

# The API server is off by default in a fresh profile, and a client cannot turn
# it on over IPC — there is no server to ask. Enable it in the config KiCad
# reads at startup, before starting KiCad. Idempotent: an already-enabled
# profile is left untouched.
function Enable-ApiServer {
    $config = Join-Path $env:APPDATA 'kicad\10.0\kicad_common.json'
    if (-not (Test-Path $config)) {
        # A profile KiCad has never written — a fresh CI runner. Writing the one
        # key we need is enough: KiCad fills the rest from its defaults on load.
        New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
        '{ "api": { "enable_server": true } }' | Set-Content $config -Encoding utf8
        Write-Host "Created $config with the API server enabled."
        return $true
    }
    $json = Get-Content $config -Raw | ConvertFrom-Json
    if ($json.api -and $json.api.enable_server) {
        Write-Host 'API server already enabled in kicad_common.json.'
        return $true
    }
    if (-not $json.api) {
        $json | Add-Member -NotePropertyName api -NotePropertyValue ([pscustomobject]@{}) -Force
    }
    $json.api | Add-Member -NotePropertyName enable_server -NotePropertyValue $true -Force
    $json | ConvertTo-Json -Depth 32 | Set-Content $config -Encoding utf8
    Write-Host "Enabled api.enable_server in $config."
    return $true
}

$pcbnewPath = Resolve-Pcbnew -Explicit $Pcbnew
Enable-ApiServer | Out-Null

$work = Join-Path ([System.IO.Path]::GetTempPath()) "konnect-live-pcb-$PID"
New-Item -ItemType Directory -Force $work | Out-Null

if (-not $Board) {
    $fixture = Join-Path $repo 'crates\konnect-ipc\tests\fixtures\live_ipc.kicad_pcb'
    if (-not (Test-Path $fixture)) { throw "board fixture missing: $fixture" }
    $Board = Join-Path $work 'live_pcb_e2e.kicad_pcb'
    Copy-Item $fixture $Board -Force
}

# `Test-Path` reports False for a live named pipe whose name embeds a drive
# letter — the FileSystem provider chokes on the colon. Enumerating the pipe
# namespace is the only reading that matches reality.
function Test-ApiPipe {
    param([string]$Name)
    try {
        return [bool]([System.IO.Directory]::GetFiles('\\.\pipe\') |
                Where-Object { $_ -eq $Name })
    }
    catch {
        return $false
    }
}

$socketFile = Join-Path $env:LOCALAPPDATA 'Temp\kicad\api.sock'
$pipe = "\\.\pipe\$socketFile"
$env:KICAD_API_SOCKET = "ipc://$socketFile"
$env:KONNECT_LIVE_KICAD_BOARD = $Board

Write-Host "pcbnew : $pcbnewPath"
Write-Host "board  : $Board"
Write-Host "socket : $env:KICAD_API_SOCKET"

$proc = Start-Process -FilePath $pcbnewPath -ArgumentList $Board -PassThru
$exit = 0
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not (Test-ApiPipe $pipe)) {
        if ($proc.HasExited) { throw "pcbnew exited with $($proc.ExitCode) before the API pipe appeared" }
        if ((Get-Date) -ge $deadline) { throw "API pipe $pipe did not appear within ${TimeoutSeconds}s" }
        Start-Sleep -Milliseconds 500
    }
    Write-Host "API pipe is up."

    # The suites poll get_open_documents themselves until KiCad answers, so no
    # extra settle time is needed here.
    foreach ($suite in @(
            @{ Package = 'konnect-ipc'; Test = 'live_kicad_test' },
            @{ Package = 'konnect';     Test = 'live_kicad_tools' })) {
        Write-Host "`n=== $($suite.Package) :: $($suite.Test) ==="
        # `| Out-Host` keeps cargo's output interleaved with this script's own
        # in the order it happened; without it the two buffer separately and a
        # captured log reads as if the suites ran before their banners.
        & cargo test -p $suite.Package --test $suite.Test -- --ignored --test-threads=1 --nocapture 2>&1 | Out-Host
        if ($LASTEXITCODE -ne 0 -and $exit -eq 0) { $exit = $LASTEXITCODE }
    }
}
finally {
    if (-not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        $proc.WaitForExit(15000) | Out-Null
    }
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

exit $exit
