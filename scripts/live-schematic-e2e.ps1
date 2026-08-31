<#
.SYNOPSIS
    Live DocumentType routing check against a running KiCad Schematic Editor.

.DESCRIPTION
    The PCB half of the live suite is `live-pcb-e2e.ps1`. This is the schematic
    half, and it exists because a schematic context used to be answered with
    `DOCTYPE_PCB`: `save_project` invoked from Eeschema wrote the *board*. The
    check therefore asserts three things against a real editor, not a mock:

      A. `--document-type schematic` sees a schematic and saves nothing to the
         board — KiCad persists schematic edits itself.
      B. `--document-type pcb`, with no board open, fails explicitly. There is
         no PCB fallback, so an indeterminate context must refuse.
      C. no flag at all (`Auto`) resolves to the single live handler.

    Then it stops the editor and compares both files byte for byte: the board
    must be untouched, which is exactly what the old routing got wrong.

    Two preconditions cost a day to find, so they are enforced here rather than
    documented:

    - **No duplicate plugin identifier under `3rdparty`.** Three directories
      declaring the same identifier — a live plugin plus rollback copies beside
      it — crash the editor about 3 s in with `0xC0000005` inside
      `wxbase332u_vc_x64_custom.dll`. The pipe appears first and then goes away,
      so the failure reads as "connection refused" rather than as a crash. Keep
      rollback copies outside `3rdparty`.
    - **No modal dialog at startup.** KiCad's `KiCad Setup` wizard and the
      library-table dialogs answer `AS_NOT_READY` on a pipe that is otherwise
      up. This script runs the editor against its own `KICAD_CONFIG_HOME`, a
      copy of the real profile with those prompts pre-answered, so the user's
      profile is never modified.

.PARAMETER Schematic
    Schematic to open. Defaults to a disposable copy of the V.3.1 fixture.

.PARAMETER Konnect
    Konnect binary under test. Defaults to `target/release/konnect.exe`.

.PARAMETER TimeoutSeconds
    How long to wait for the editor to answer on the API. Default 90.
#>
[CmdletBinding()]
param(
    [string]$Schematic,
    [string]$Konnect,
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not $Konnect) { $Konnect = Join-Path $repo 'target\release\konnect.exe' }
if (-not (Test-Path $Konnect)) { throw "konnect binary missing: $Konnect" }

$eeschema = Join-Path $env:LOCALAPPDATA 'Programs\KiCad\10.0\bin\eeschema.exe'
if (-not (Test-Path $eeschema)) { throw "eeschema missing: $eeschema" }

$work = Join-Path ([IO.Path]::GetTempPath()) "konnect-live-sch-$PID"
New-Item -ItemType Directory -Force $work | Out-Null

if (-not $Schematic) {
    $fixture = Join-Path $env:USERPROFILE 'Documents\KiCad\KonnectValidationV31'
    if (-not (Test-Path $fixture)) { throw "schematic fixture missing: $fixture" }
    Copy-Item "$fixture\*" $work -Force
    $Schematic = Join-Path $work 'konnect_v31_eeschema_pipe_fixture.kicad_sch'
}
$board = [IO.Path]::ChangeExtension($Schematic, '.kicad_pcb')

# A profile of our own: the real one, with the modal prompts already answered
# and the API server on. Nothing is written back to the user's profile.
$profileHome = Join-Path $work 'config'
New-Item -ItemType Directory -Force (Join-Path $profileHome '10.0') | Out-Null
Copy-Item (Join-Path $env:APPDATA 'kicad\10.0\*') (Join-Path $profileHome '10.0') -Recurse -Force
$common = Join-Path $profileHome '10.0\kicad_common.json'
$json = Get-Content $common -Raw | ConvertFrom-Json
if (-not $json.api) { $json | Add-Member api ([pscustomobject]@{}) -Force }
$json.api | Add-Member enable_server $true -Force
if (-not $json.do_not_show_again) { $json | Add-Member do_not_show_again ([pscustomobject]@{}) -Force }
foreach ($prompt in @('data_collection_prompt', 'update_check_prompt', 'env_var_overwrite_warning',
                      'migrate_wrl_prompt', 'scaled_3d_models_warning', 'zone_fill_warning')) {
    $json.do_not_show_again | Add-Member $prompt $true -Force
}
$json | ConvertTo-Json -Depth 32 | Set-Content $common -Encoding utf8

# Duplicate identifiers are a crash, not a warning: refuse to start on one.
$thirdParty = Join-Path $env:USERPROFILE 'Documents\KiCad\10.0\3rdparty'
if (Test-Path $thirdParty) {
    $identifiers = Get-ChildItem $thirdParty -Recurse -Filter 'plugin.json' -ErrorAction SilentlyContinue |
        ForEach-Object { (Get-Content $_.FullName -Raw | ConvertFrom-Json).identifier } |
        Where-Object { $_ }
    $duplicates = $identifiers | Group-Object | Where-Object Count -gt 1
    if ($duplicates) {
        throw ("duplicate plugin identifiers under ${thirdParty}: " +
               (($duplicates | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join ', ') +
               ' — move rollback copies outside 3rdparty, they crash the API server')
    }
}

$socket = 'ipc://' + (Join-Path $env:LOCALAPPDATA 'Temp\kicad\api.sock')

# ── MCP over stdio ───────────────────────────────────────────────────────────
function New-Mcp {
    param([string[]]$Arguments)
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Konnect
    foreach ($a in $Arguments) { [void]$psi.ArgumentList.Add($a) }
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['KICAD_API_SOCKET'] = $socket
    $state = [pscustomobject]@{ Proc = [Diagnostics.Process]::Start($psi); Id = 0 }
    [void](Invoke-Mcp $state 'initialize' @{
        protocolVersion = '2025-06-18'; capabilities = @{}
        clientInfo = @{ name = 'live-schematic-e2e'; version = '0' }
    })
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

# ── Editor ───────────────────────────────────────────────────────────────────
function Start-Editor {
    $env:KICAD_CONFIG_HOME = $profileHome
    $proc = Start-Process -FilePath $eeschema -ArgumentList "`"$Schematic`"" -PassThru
    Remove-Item Env:\KICAD_CONFIG_HOME
    $probe = New-Mcp @('--document-type', 'schematic')
    try {
        for ($i = 1; $i -le $TimeoutSeconds; $i++) {
            Start-Sleep -Seconds 1
            if ($proc.HasExited) { throw "eeschema exited with $($proc.ExitCode) before answering" }
            if ((Invoke-Tool $probe 'open_project').Text -match '"kicad_ui_running":true') {
                Write-Host "eeschema answered after ${i}s."
                return $proc
            }
        }
    }
    finally { Close-Mcp $probe }
    if (-not $proc.HasExited) { $proc.Kill() }
    throw "eeschema never answered on $socket within ${TimeoutSeconds}s"
}

function Stop-Editor {
    param($Proc)
    if ($Proc -and -not $Proc.HasExited) { $Proc.Kill(); [void]$Proc.WaitForExit(5000) }
}

$failures = @()
function Assert {
    param([string]$Name, [bool]$Condition, [string]$Detail)
    if ($Condition) { Write-Host "PASS $Name" }
    else { Write-Host "FAIL $Name — $Detail"; $script:failures += $Name }
}

$boardBefore = (Get-FileHash $board -Algorithm SHA256).Hash
$schBefore   = (Get-FileHash $Schematic -Algorithm SHA256).Hash

$editor = Start-Editor
try {
    $a = New-Mcp @('--document-type', 'schematic')
    $open = Invoke-Tool $a 'open_project'
    Assert 'schematic context sees a schematic' `
        ($open.Text -match '"type":"schematic"') $open.Text
    $save = Invoke-Tool $a 'save_project'
    Assert 'schematic context saves no board' `
        ((-not $save.IsError) -and $save.Text -match 'already persisted') $save.Text
    Close-Mcp $a

    $b = New-Mcp @('--document-type', 'pcb')
    $save = Invoke-Tool $b 'save_project'
    Assert 'pcb context refuses without a board' $save.IsError $save.Text
    Close-Mcp $b

    $c = New-Mcp @()
    $open = Invoke-Tool $c 'open_project'
    Assert 'auto context resolves the live handler' `
        ($open.Text -match '"type":"schematic"') $open.Text
    Close-Mcp $c
}
finally { Stop-Editor $editor }

Assert 'board untouched' ((Get-FileHash $board -Algorithm SHA256).Hash -eq $boardBefore) 'board hash changed'
Assert 'schematic untouched' ((Get-FileHash $Schematic -Algorithm SHA256).Hash -eq $schBefore) 'schematic hash changed'

# Reopen: the routing must survive a full editor restart, not just one session.
$editor = Start-Editor
try {
    $d = New-Mcp @('--document-type', 'schematic')
    $open = Invoke-Tool $d 'open_project'
    Assert 'schematic still routed after reopen' `
        ($open.Text -match '"type":"schematic"') $open.Text
    Close-Mcp $d
}
finally { Stop-Editor $editor }

if ($failures) {
    Write-Host "`n$($failures.Count) check(s) failed: $($failures -join ', ')"
    exit 1
}
Write-Host "`nAll checks passed."
