<#
.SYNOPSIS
    W.1.6 — proves the KiCad editor-lock guard against a real Eeschema.

.DESCRIPTION
    The unit and integration tests plant a lock file themselves. That proves the
    guard, not the premise: it says nothing about whether a real editor actually
    creates the file Konnect looks for, at the path Konnect builds, and removes
    it on the way out. This script is the premise.

    On a disposable copy of the validation fixture it:

      1. records the schematic's SHA-256 and its directory listing;
      2. launches `eeschema.exe` on it and waits for the sibling lock to appear;
      3. calls a real write tool over MCP stdio and requires `conflict`;
      4. requires the schematic to be byte-identical, the lock untouched, and
         no scratch or transaction-journal sibling to have appeared;
      5. requires a read tool to still answer while the lock exists;
      6. closes the editor cleanly, waits for KiCad to remove its own lock,
         and requires the identical write to succeed;
      7. re-reads through a different tool to confirm the change is on disk.

    Nothing is ever deleted from a KiCad project by this script: step 6 waits
    for KiCad to release the lock itself, which is the same thing a user does.

    The editor runs against its own KICAD_CONFIG_HOME, a copy of the real
    profile with the modal start-up prompts already answered, so nothing is
    written back to the user's profile.

.PARAMETER Konnect
    Path to konnect.exe. Defaults to target\release\konnect.exe.

.PARAMETER TimeoutSeconds
    How long to wait for the editor and for its lock. Default 90.
#>
[CmdletBinding()]
param(
    [string]$Konnect,
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not $Konnect) { $Konnect = Join-Path $repo 'target\release\konnect.exe' }
if (-not (Test-Path $Konnect)) { throw "konnect binary missing: $Konnect" }

$eeschema = Join-Path $env:LOCALAPPDATA 'Programs\KiCad\10.0\bin\eeschema.exe'
if (-not (Test-Path $eeschema)) { throw "eeschema missing: $eeschema" }

# One editor per machine owns the API socket, and a leftover one answers
# requests meant for the editor this script launches. Refuse to guess.
$stale = Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in @('kicad', 'eeschema', 'pcbnew') }
if ($stale) {
    throw ('another KiCad process is running: ' +
           (($stale | ForEach-Object { "$($_.ProcessName)($($_.Id))" }) -join ', ') +
           ' — close it before running this script')
}

$work = Join-Path ([IO.Path]::GetTempPath()) "konnect-live-lock-$PID"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force $work | Out-Null

$fixture = Join-Path $env:USERPROFILE 'Documents\KiCad\KonnectValidationV31'
if (-not (Test-Path $fixture)) { throw "schematic fixture missing: $fixture" }
Copy-Item "$fixture\*" $work -Force
$sch = Join-Path $work 'konnect_v31_eeschema_pipe_fixture.kicad_sch'
$lock = Join-Path $work '~konnect_v31_eeschema_pipe_fixture.kicad_sch.lck'

$profileHome = Join-Path $work 'config'
New-Item -ItemType Directory -Force (Join-Path $profileHome '10.0') | Out-Null
Copy-Item (Join-Path $env:APPDATA 'kicad\10.0\*') (Join-Path $profileHome '10.0') -Recurse -Force
$common = Join-Path $profileHome '10.0\kicad_common.json'
$json = Get-Content $common -Raw | ConvertFrom-Json
if (-not $json.do_not_show_again) { $json | Add-Member do_not_show_again ([pscustomobject]@{}) -Force }
foreach ($prompt in @('data_collection_prompt', 'update_check_prompt', 'env_var_overwrite_warning',
                      'migrate_wrl_prompt', 'scaled_3d_models_warning', 'zone_fill_warning')) {
    $json.do_not_show_again | Add-Member $prompt $true -Force
}
$json | ConvertTo-Json -Depth 32 | Set-Content $common -Encoding utf8

# ── MCP over stdio ───────────────────────────────────────────────────────────
function New-Mcp {
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Konnect
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $state = [pscustomobject]@{ Proc = [Diagnostics.Process]::Start($psi); Id = 0 }
    [void](Invoke-Mcp $state 'initialize' @{
        protocolVersion = '2025-06-18'; capabilities = @{}
        clientInfo = @{ name = 'live-editor-lock'; version = '0' }
    })
    # Toolsets are opt-in: a client loads what it needs before calling into it,
    # and an unloaded tool answers `toolset_not_loaded`, which would make every
    # assertion below pass for the wrong reason.
    foreach ($toolset in @('sch_wiring', 'sch_analysis')) {
        $loaded = Invoke-Tool $state 'load_toolset' @{ name = $toolset }
        if ($loaded.IsError) { throw "load_toolset('$toolset') failed: $($loaded.Text)" }
    }
    return $state
}

function Invoke-Mcp {
    param($State, [string]$Method, $Params)
    $State.Id++
    $State.Proc.StandardInput.WriteLine((@{
        jsonrpc = '2.0'; id = $State.Id; method = $Method; params = $Params
    } | ConvertTo-Json -Depth 12 -Compress))
    $State.Proc.StandardInput.Flush()
    while ($true) {
        $line = $State.Proc.StandardOutput.ReadLine()
        if ($null -eq $line) { throw "konnect exited before replying to $Method" }
        if (-not $line.Trim()) { continue }
        $response = $line | ConvertFrom-Json
        if ($response.id -eq $State.Id) { return $response }
    }
}

function Invoke-Tool {
    param($State, [string]$Name, $Arguments = @{})
    $r = Invoke-Mcp $State 'tools/call' @{ name = $Name; arguments = $Arguments }
    [pscustomobject]@{
        IsError = [bool]$r.result.isError
        Text    = (($r.result.content | ForEach-Object { $_.text }) -join "`n")
    }
}

function Close-Mcp {
    param($State)
    try { $State.Proc.StandardInput.Close() } catch { }
    if (-not $State.Proc.WaitForExit(4000)) { $State.Proc.Kill() }
}

$failures = @()
function Assert {
    param([string]$Name, [bool]$Condition, [string]$Detail)
    if ($Condition) { Write-Host "PASS $Name" -ForegroundColor Green }
    else { Write-Host "FAIL $Name — $Detail" -ForegroundColor Red; $script:failures += $Name }
}

# The exact write this script requires to be refused, then to succeed. A wire
# on an otherwise empty fixture: it needs no library, no symbol, and no prior
# state, so the only thing that can decide its outcome is the lock.
$editArgs = @{ schematic = $sch; x1 = 63.5; y1 = 63.5; x2 = 88.9; y2 = 63.5 }

$schBefore  = (Get-FileHash $sch -Algorithm SHA256).Hash
$dirBefore  = (Get-ChildItem $work -Force -File | ForEach-Object Name | Sort-Object) -join '|'

$mcp = New-Mcp
$proc = $null
try {
    # ── Editor open, lock present ────────────────────────────────────────────
    $env:KICAD_CONFIG_HOME = $profileHome
    $proc = Start-Process -FilePath $eeschema -ArgumentList "`"$sch`"" -PassThru
    Remove-Item Env:\KICAD_CONFIG_HOME

    $sawLock = $false
    for ($i = 1; $i -le $TimeoutSeconds; $i++) {
        Start-Sleep -Seconds 1
        if ($proc.HasExited) { throw "eeschema exited with $($proc.ExitCode) before locking" }
        if (Test-Path -LiteralPath $lock) { $sawLock = $true; Write-Host "lock appeared after ${i}s"; break }
    }
    Assert 'kicad creates the sibling lock konnect looks for' $sawLock `
        "no $lock after ${TimeoutSeconds}s"
    if (-not $sawLock) { throw 'premise failed: nothing to test against' }

    # A window has to exist before `CloseMainWindow` has anywhere to post
    # WM_CLOSE, and the lock appears before the frame is up.
    for ($i = 1; $i -le $TimeoutSeconds; $i++) {
        $proc.Refresh()
        if ($proc.MainWindowHandle -ne 0) { Write-Host "editor window after ${i}s"; break }
        Start-Sleep -Seconds 1
    }
    if ($proc.MainWindowHandle -eq 0) { throw "eeschema never showed a window within ${TimeoutSeconds}s" }

    $lockBefore = Get-Content -LiteralPath $lock -Raw
    Write-Host "lock contents: $lockBefore"

    $blocked = Invoke-Tool $mcp 'add_wire' $editArgs
    Assert 'a write is refused while the editor holds the schematic' $blocked.IsError $blocked.Text
    Assert 'the refusal is the conflict kind' ($blocked.Text -match '"kind"\s*:\s*"conflict"') $blocked.Text
    Assert 'the refusal names the lock file' ($blocked.Text -match '\.lck') $blocked.Text

    Assert 'the schematic is bit-identical after the refusal' `
        ((Get-FileHash $sch -Algorithm SHA256).Hash -eq $schBefore) 'sha256 changed'
    Assert 'the kicad lock is untouched' `
        ((Get-Content -LiteralPath $lock -Raw) -eq $lockBefore) 'lock contents changed'
    $dirAfter = (Get-ChildItem $work -Force -File | ForEach-Object Name | Sort-Object) -join '|'
    $newFiles = (Compare-Object ($dirBefore -split '\|') ($dirAfter -split '\|') |
        Where-Object SideIndicator -eq '=>' | ForEach-Object InputObject)
    $debris = $newFiles | Where-Object { $_ -notlike '~*' -and $_ -notlike '_autosave-*' }
    Assert 'the refusal created no scratch and no transaction journal' (-not $debris) `
        ("unexpected files: " + ($debris -join ', '))

    $read = Invoke-Tool $mcp 'list_schematic_wires' @{ schematic = $sch }
    Assert 'reads still work while the schematic is locked' (-not $read.IsError) $read.Text

    # ── Editor closed, lock released by KiCad itself ─────────────────────────
    # A clean close, never a kill: killing skips KiCad's own shutdown, which is
    # what releases the lock, and this script must not remove that lock itself
    # — the whole policy under test is that Konnect never does.
    #
    # WM_CLOSE is posted, not delivered synchronously: a frame that has just
    # appeared can still be finishing its own start-up and drop it, so the post
    # is retried rather than trusted once.
    Start-Sleep -Seconds 3
    $closed = $false
    for ($attempt = 1; $attempt -le 4 -and -not $closed; $attempt++) {
        $proc.Refresh()
        [void]$proc.CloseMainWindow()
        $closed = $proc.WaitForExit(15000)
        if (-not $closed) { Write-Host "close attempt $attempt did not take; retrying" }
    }
    if (-not $closed) { throw 'eeschema did not close cleanly; refusing to remove its lock' }
    for ($i = 1; $i -le 15 -and (Test-Path -LiteralPath $lock); $i++) { Start-Sleep -Seconds 1 }
    Assert 'a clean close makes kicad release its own lock' (-not (Test-Path -LiteralPath $lock)) `
        "$lock still present after a clean close"

    $applied = Invoke-Tool $mcp 'add_wire' $editArgs
    Assert 'the identical write succeeds once the editor is closed' (-not $applied.IsError) $applied.Text

    $reread = Invoke-Tool $mcp 'list_schematic_wires' @{ schematic = $sch }
    Assert 'an independent read sees the change on disk' ($reread.Text -match '88\.9') $reread.Text
}
finally {
    Close-Mcp $mcp
    if ($proc -and -not $proc.HasExited) { $proc.Kill(); [void]$proc.WaitForExit(5000) }
}

Write-Host ''
if ($failures) {
    Write-Host ("FAILED: " + ($failures -join ', ')) -ForegroundColor Red
    Write-Host "work dir kept for inspection: $work"
    exit 1
}
Write-Host 'ALL CHECKS PASSED' -ForegroundColor Green
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
