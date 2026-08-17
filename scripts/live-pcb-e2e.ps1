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
        # A profile KiCad has never written — a fresh CI runner. Writing the keys
        # we need is enough: KiCad fills the rest from its defaults on load.
        #
        # `canvas_type` 2 is Cairo, i.e. software rendering. It is set only here,
        # in the profile this script creates, because a machine with no profile
        # is a machine we know nothing about — and a `windows-latest` runner has
        # no OpenGL 2.1. pcbnew's response to that is a modal `Information`
        # dialog ("Could not use OpenGL, falling back to software rendering")
        # which it serves *before* the API, so the pipe comes up, the PCB Editor
        # window stays hidden behind the dialog, and every request is answered
        # `AS_NOT_READY` until something clicks OK. Choosing software rendering
        # up front means there is nothing to report. No live test reads a
        # rendered canvas, so nothing is given up. An existing profile is left
        # alone: that is a real user's rendering preference.
        New-Item -ItemType Directory -Force (Split-Path $config) | Out-Null
        '{ "api": { "enable_server": true }, "graphics": { "canvas_type": 2 } }' |
            Set-Content $config -Encoding utf8
        Write-Host "Created $config with the API server enabled and software rendering."
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

# A profile with no library tables is a profile KiCad stops to talk about: it
# creates a default copy and says so in a modal `Information` dialog, and a modal
# dialog is why a runner's pcbnew answers `AS_NOT_READY` on a pipe that is
# otherwise up. Writing the tables first is the same content KiCad would have
# written, minus the dialog. Idempotent: existing tables are left untouched.
function Initialize-LibraryTables {
    param([string]$PcbnewPath)

    # ...\bin\pcbnew.exe → the installation root that holds share\kicad\template.
    $install = Split-Path (Split-Path $PcbnewPath)
    $template = Join-Path $install 'share\kicad\template'
    $profile = Join-Path $env:APPDATA 'kicad\10.0'
    New-Item -ItemType Directory -Force $profile | Out-Null

    foreach ($table in @(
            @{ File = 'fp-lib-table';  Tag = 'fp_lib_table' },
            @{ File = 'sym-lib-table'; Tag = 'sym_lib_table' })) {
        $target = Join-Path $profile $table.File
        if (Test-Path $target) { continue }

        $source = Join-Path $template $table.File
        $entry = if (Test-Path $source) {
            # The same single "Table" entry a fresh KiCad profile gets: an
            # indirection to the installation's own table, forward slashes and
            # all.
            $uri = $source -replace '\\', '/'
            "`n`t(lib (name `"KiCad`") (type `"Table`") (uri `"$uri`") (options `"`") (descr `"Default KiCad libraries`"))"
        }
        else {
            # Nothing installed to point at. An empty table still keeps the
            # dialog away.
            ''
        }
        "($($table.Tag)`n`t(version 7)$entry`n)" | Set-Content $target -Encoding utf8
        Write-Host "Wrote $target (KiCad would have created it and said so in a dialog)."
    }
}

$pcbnewPath = Resolve-Pcbnew -Explicit $Pcbnew
Enable-ApiServer | Out-Null
Initialize-LibraryTables -PcbnewPath $pcbnewPath

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
#
# And the name is matched by *shape*, not by equality with the path this script
# computed. KiCad builds its own socket path, and it does not have to spell it
# the way `$env:LOCALAPPDATA` does: on a GitHub runner it came up as
# `C:\Users\RUNNER~1\...` — the 8.3 short name — against a `runneradmin` in the
# environment. A pipe name is a literal in a namespace with no path resolution,
# so the client has to be handed the name that exists rather than the one that
# ought to. Returns the real name, or $null.
function Get-ApiPipe {
    try {
        return [System.IO.Directory]::GetFiles('\\.\pipe\') |
            Where-Object { $_ -like '*\kicad\api.sock' } |
            Select-Object -First 1
    }
    catch {
        return $null
    }
}

$socketFile = Join-Path $env:LOCALAPPDATA 'Temp\kicad\api.sock'
$pipe = "\\.\pipe\$socketFile"
$env:KICAD_API_SOCKET = "ipc://$socketFile"
$env:KONNECT_LIVE_KICAD_BOARD = $Board

Write-Host "pcbnew : $pcbnewPath"
Write-Host "board  : $Board"
Write-Host "socket : $env:KICAD_API_SOCKET"

# A pcbnew that holds a modal dialog answers `AS_NOT_READY` on a pipe that is
# perfectly up, and the dialog's text is the only thing that says which dialog.
# Enumerate every window the process owns, and every static/button child, because
# the message lives in the children rather than in the caption.
function Write-WindowDiagnostics {
    param([int]$ProcessId)
    if (-not ('Konnect.Win32' -as [type])) {
        Add-Type -Namespace Konnect -Name Win32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc cb, IntPtr p);
[DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
public delegate bool EnumProc(IntPtr h, IntPtr p);
'@
    }
    $caption = {
        param([IntPtr]$Handle)
        $sb = New-Object System.Text.StringBuilder 512
        [void][Konnect.Win32]::GetWindowText($Handle, $sb, $sb.Capacity)
        return $sb.ToString()
    }
    Write-Host "windows owned by pcbnew:"
    # `$script:` on both sides: the callback runs as a delegate, and a plain
    # assignment inside it would write to its own scope and be lost.
    $script:seen = $false
    $onWindow = [Konnect.Win32+EnumProc] {
        param([IntPtr]$handle, [IntPtr]$_unused)
        $owner = 0
        [void][Konnect.Win32]::GetWindowThreadProcessId($handle, [ref]$owner)
        if ($owner -ne $ProcessId) { return $true }
        $title = & $caption $handle
        $visible = [Konnect.Win32]::IsWindowVisible($handle)
        if (-not $title -and -not $visible) { return $true }
        $script:seen = $true
        Write-Host "  [$(if ($visible) { 'visible' } else { 'hidden ' })] '$title'"
        $onChild = [Konnect.Win32+EnumProc] {
            param([IntPtr]$child, [IntPtr]$_alsoUnused)
            $text = & $caption $child
            if ($text) { Write-Host "      child: '$text'" }
            return $true
        }
        [void][Konnect.Win32]::EnumChildWindows($handle, $onChild, [IntPtr]::Zero)
        return $true
    }
    [void][Konnect.Win32]::EnumWindows($onWindow, [IntPtr]::Zero)
    if (-not $script:seen) { Write-Host '  (none)' }
}

# When the pipe never appears, "it timed out" is not a diagnosis. A live pcbnew
# with no main window and no CPU time is stuck before it reaches its API server;
# one with a window and a spinning CPU is stuck on something modal. Both readings
# are lost once the process is killed, so take them first.
function Write-PipeDiagnostics {
    param([System.Diagnostics.Process]$Process)
    Write-Host "--- diagnostics: the API pipe never appeared ---"
    try {
        $live = Get-Process -Id $Process.Id -ErrorAction Stop
        $live.Refresh()
        Write-Host "pcbnew pid       : $($live.Id)"
        Write-Host "responding       : $($live.Responding)"
        Write-Host "main window title: '$($live.MainWindowTitle)'"
        Write-Host "main window handle: $($live.MainWindowHandle)"
        Write-Host "cpu seconds      : $([math]::Round($live.TotalProcessorTime.TotalSeconds, 2))"
        Write-Host "working set MB   : $([math]::Round($live.WorkingSet64 / 1MB, 1))"
    }
    catch {
        Write-Host "pcbnew is no longer queryable: $_"
    }
    try {
        $kicadPipes = [System.IO.Directory]::GetFiles('\\.\pipe\') |
            Where-Object { $_ -like '*kicad*' }
        Write-Host "kicad pipes      : $(if ($kicadPipes) { $kicadPipes -join ', ' } else { '(none)' })"
    }
    catch {
        Write-Host "the pipe namespace could not be enumerated: $_"
    }
    Write-WindowDiagnostics -ProcessId $Process.Id
    Write-Host "-----------------------------------------------"
}

$proc = Start-Process -FilePath $pcbnewPath -ArgumentList $Board -PassThru
$exit = 0
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $actualPipe = Get-ApiPipe
    while (-not $actualPipe) {
        if ($proc.HasExited) { throw "pcbnew exited with $($proc.ExitCode) before the API pipe appeared" }
        if ((Get-Date) -ge $deadline) {
            Write-PipeDiagnostics -Process $proc
            throw "no \\.\pipe\*\kicad\api.sock appeared within ${TimeoutSeconds}s (expected around $pipe)"
        }
        Start-Sleep -Milliseconds 500
        $actualPipe = Get-ApiPipe
    }

    # Hand the suites the name KiCad actually opened. They read
    # KICAD_API_SOCKET and connect to it verbatim, so a mismatch here is not a
    # cosmetic difference — it is a connection to a pipe that does not exist.
    $actualSocket = $actualPipe.Substring('\\.\pipe\'.Length)
    if ($actualSocket -ne $socketFile) {
        Write-Host "API pipe is up under a different spelling than expected:"
        Write-Host "  expected: $socketFile"
        Write-Host "  actual  : $actualSocket"
        $env:KICAD_API_SOCKET = "ipc://$actualSocket"
    }
    else {
        Write-Host "API pipe is up."
    }

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
        if ($LASTEXITCODE -ne 0) {
            # A suite failing while the pipe is up is the `AS_NOT_READY` shape:
            # KiCad is reachable and not answering, which a modal dialog explains
            # and nothing else in the log does. Take the reading once, before the
            # next suite muddies it.
            if ($exit -eq 0) {
                Write-Host "--- diagnostics: a suite failed with the pipe up ---"
                Write-WindowDiagnostics -ProcessId $proc.Id
                Write-Host "----------------------------------------------------"
                $exit = $LASTEXITCODE
            }
        }
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
