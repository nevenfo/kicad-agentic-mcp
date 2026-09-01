<#
.SYNOPSIS
    Lifts B2.8 of the Hi-Fi benchmark through Konnect alone (W.3.4).

.DESCRIPTION
    `RV1` is the volume potentiometer, moved off the board in B2.1: its wiper
    goes to the chassis through `J4`, and the part itself must not be placed on
    the PCB. KiCAD spells that `(on_board no)` on the symbol block. No MCP tool
    could write it, so an earlier session left a custom property named
    `exclude_from_board` — a field KiCAD lists and ignores, which changes
    nothing about "Update PCB from schematic". B2.8 is therefore two operations:
    set the real attribute, and take the decoy away.

    W.3 adds both. This script proves them on the real project rather than on a
    fixture:

      A. `edit_schematic_component` sets `on_board` to false and passes
         `fields: { exclude_from_board: null }` in the same call.
      B. The result is read back through `get_schematic_component`, and read
         again straight out of the file, so the assertion does not rest on the
         tool that performed the change.
      C. `kicad-cli sch erc` runs before and after: KiCAD's own reading of the
         document, unchanged. A file this edit had corrupted would fail to
         parse there, and a symbol this edit had damaged would show up as a new
         violation.

    The symbol is addressed by uuid, not by designator, because that is what
    the project's own history recommends (D1.6): a designator is a property,
    a uuid is identity.

.PARAMETER Project
    The Hi-Fi project directory. Defaults to the benchmark's own.

.PARAMETER Konnect
    Konnect binary under test. Defaults to `target/release/konnect.exe`.

.PARAMETER KicadCli
    `kicad-cli.exe`, for the two ERC runs.

.PARAMETER InPlace
    Edit the project's own schematic. Without it — the default — the project is
    copied to a work directory first and the real one is never touched.
#>
[CmdletBinding()]
param(
    [string]$Project,
    [string]$Konnect,
    [string]$KicadCli,
    [switch]$InPlace
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not $Konnect) { $Konnect = Join-Path $repo 'target\release\konnect.exe' }
if (-not (Test-Path $Konnect)) { throw "konnect binary missing: $Konnect" }

if (-not $KicadCli) {
    $KicadCli = Join-Path $env:LOCALAPPDATA 'Programs\KiCad\10.0\bin\kicad-cli.exe'
}
if (-not (Test-Path $KicadCli)) { throw "kicad-cli missing: $KicadCli" }

if (-not $Project) {
    $Project = Join-Path $env:USERPROFILE 'Documents\Etabli\Projets\Chaine Hifi'
}
if (-not (Test-Path $Project)) { throw "project missing: $Project" }

$name = 'HifiAmp_TPA3255'
# RV1's uuid in the project schematic. A designator is a property of a symbol;
# a uuid is the symbol.
$rv1 = '44e1a6f1-937d-4a04-8d4b-a0855126f41d'

$work = Join-Path ([IO.Path]::GetTempPath()) "konnect-live-b28-$PID"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force $work | Out-Null

$target = $Project
if (-not $InPlace) {
    $target = Join-Path $work 'project'
    New-Item -ItemType Directory -Force $target | Out-Null
    Copy-Item (Join-Path $Project '*') $target -Recurse -Force
    # A lock left behind by a KiCAD that did not close cleanly is not this
    # copy's business, and W.1 would rightly refuse to touch a schematic beside
    # a `.kicad_sch.lck`. Only the project lock is dropped from the copy; a
    # schematic lock would mean KiCAD really is holding the document, and the
    # refusal it produces is correct.
    Get-ChildItem $target -Filter '~*.kicad_pro.lck' -Force -ErrorAction SilentlyContinue |
        Remove-Item -Force
}
$sch = Join-Path $target "$name.kicad_sch"
if (-not (Test-Path -LiteralPath $sch)) { throw "schematic missing: $sch" }
Write-Host "project under test: $target"

# The document as it was, whatever happens next.
$before = Join-Path $work 'before.kicad_sch'
Copy-Item -LiteralPath $sch -Destination $before -Force
Write-Host "original kept in  : $before"

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
        clientInfo = @{ name = 'live-b28-on-board'; version = '0' }
    })
    # Toolsets are opt-in: an unloaded tool answers `toolset_not_loaded`, and an
    # assertion would pass for the wrong reason.
    foreach ($set in @('sch_components')) {
        $loaded = Invoke-Tool $state 'load_toolset' @{ name = $set }
        if ($loaded.IsError) { throw "load_toolset('$set') failed: $($loaded.Text)" }
    }
    return $state
}

function Invoke-Mcp {
    param($State, [string]$Method, $Params)
    $State.Id++
    $State.Proc.StandardInput.WriteLine((@{
        jsonrpc = '2.0'; id = $State.Id; method = $Method; params = $Params
    } | ConvertTo-Json -Depth 20 -Compress))
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

function Get-Component {
    param($State, [string]$Uuid)
    $r = Invoke-Tool $State 'get_schematic_component' @{ schematic = $sch; uuid = $Uuid }
    if ($r.IsError) { throw "get_schematic_component failed: $($r.Text)" }
    return $r.Text | ConvertFrom-Json
}

# RV1's own top-level `(symbol …)` block, and the document either side of it.
# The `lib_symbols` definition at the top of the file carries `(on_board …)` of
# its own, so everything here is anchored on the uuid rather than searched for
# globally.
function Split-AroundRv1 {
    param([string]$Path)
    $text = Get-Content -LiteralPath $Path -Raw
    $anchor = $text.IndexOf("(uuid `"$rv1`")")
    if ($anchor -lt 0) { throw "RV1's uuid is not in $Path" }
    $start = $text.LastIndexOf("`n`t(symbol", $anchor)
    if ($start -lt 0) { throw 'RV1 is not inside a top-level symbol block' }
    # A top-level symbol block ends at the first line that closes at its own
    # indentation — one tab — which no nested child ever reaches.
    $endRel = $text.Substring($start + 1).IndexOf("`n`t)")
    if ($endRel -lt 0) { throw "RV1's symbol block is not closed" }
    $end = $start + 1 + $endRel + 3
    [pscustomobject]@{
        Before = $text.Substring(0, $start)
        Block  = $text.Substring($start, $end - $start)
        After  = $text.Substring($end)
    }
}

# The `(on_board …)` tag of RV1's own symbol block.
function Get-OnBoardTag {
    param([string]$Path)
    $block = (Split-AroundRv1 $Path).Block
    if ($block -match '\(on_board (yes|no)\)') { return $Matches[1] }
    return $null
}

# KiCAD's own reading of the document: the ERC violation count, per severity.
function Invoke-Erc {
    param([string]$Label)
    $out = Join-Path $work "erc-$Label.json"
    & $KicadCli sch erc --format json --severity-all --exit-code-violations -o $out $sch 2>&1 |
        Out-String | Write-Verbose
    if (-not (Test-Path $out)) { throw "kicad-cli produced no ERC report for $Label" }
    $report = Get-Content $out -Raw | ConvertFrom-Json
    $violations = @()
    foreach ($sheet in $report.sheets) {
        foreach ($v in $sheet.violations) { $violations += $v }
    }
    [pscustomobject]@{
        Errors   = @($violations | Where-Object { $_.severity -eq 'error' }).Count
        Warnings = @($violations | Where-Object { $_.severity -eq 'warning' }).Count
    }
}

$failures = @()
function Assert {
    param([string]$Name, [bool]$Condition, [string]$Detail)
    if ($Condition) { Write-Host "PASS $Name" -ForegroundColor Green }
    else { Write-Host "FAIL $Name — $Detail" -ForegroundColor Red; $script:failures += $Name }
}

$ercBefore = Invoke-Erc 'before'
Write-Host "ERC before: $($ercBefore.Errors) errors, $($ercBefore.Warnings) warnings"

$mcp = New-Mcp
try {
    # ── The defect ───────────────────────────────────────────────────────────
    $rv1Before = Get-Component $mcp $rv1
    Assert 'RV1 starts on the board, which is what B2.8 is about' `
        ($rv1Before.on_board -eq $true) "on_board is $($rv1Before.on_board)"
    Assert 'RV1 starts carrying the decoy property' `
        ((Get-Content -LiteralPath $before -Raw) -match 'exclude_from_board') `
        'no exclude_from_board property found'

    # ── One call: set the attribute, remove the decoy ─────────────────────────
    $r = Invoke-Tool $mcp 'edit_schematic_component' @{
        schematic = $sch
        uuid = $rv1
        on_board = $false
        fields = @{ exclude_from_board = $null }
    }
    Assert 'the edit is accepted' (-not $r.IsError) $r.Text
    if (-not $r.IsError) {
        $changes = ($r.Text | ConvertFrom-Json).changes
        Assert 'both operations are reported' `
            (($changes -join '; ') -match 'on_board' -and ($changes -join '; ') -match 'removed') `
            ($changes -join '; ')
    }

    # ── Read back, through the tool and past it ──────────────────────────────
    $rv1After = Get-Component $mcp $rv1
    Assert 'RV1 is off the board' ($rv1After.on_board -eq $false) `
        "on_board is $($rv1After.on_board)"
    Assert 'nothing else about RV1 moved' `
        ($rv1After.lib_id -eq $rv1Before.lib_id -and
         $rv1After.value -eq $rv1Before.value -and
         $rv1After.footprint -eq $rv1Before.footprint -and
         $rv1After.x -eq $rv1Before.x -and $rv1After.y -eq $rv1Before.y -and
         $rv1After.in_bom -eq $rv1Before.in_bom -and $rv1After.dnp -eq $rv1Before.dnp) `
        'a field other than on_board changed'

    Assert 'the file itself says (on_board no)' ((Get-OnBoardTag $sch) -eq 'no') `
        "the tag reads $(Get-OnBoardTag $sch)"
    $after = Get-Content -LiteralPath $sch -Raw
    Assert 'the decoy property is gone from the document' `
        (-not ($after -match 'exclude_from_board')) 'exclude_from_board is still there'

    # Every other symbol is untouched. Not "no unexpected lines differ" — a
    # removed property spans several lines and a line-wise filter ends up
    # excusing whatever it happens to see — but: the document either side of
    # RV1's own block is byte for byte what it was.
    $split = Split-AroundRv1 $sch
    $splitBefore = Split-AroundRv1 $before
    Assert 'nothing before RV1 in the document changed' `
        ($split.Before -eq $splitBefore.Before) 'the text above RV1 differs'
    Assert 'nothing after RV1 in the document changed' `
        ($split.After -eq $splitBefore.After) 'the text below RV1 differs'
    # And inside the block, only the two things asked for: dropping the decoy
    # property's lines and flipping the tag turns the old block into the new.
    # The whole `(property "exclude_from_board" …)` block, not just the line
    # naming it: eeschema writes one over eight lines here, and a comparison
    # that dropped only the named line would call leftover `(at …)` and
    # `(effects …)` orphans a match.
    $expected = $splitBefore.Block -replace
        '(?s)\r?\n\t\t\(property "exclude_from_board".*?\r?\n\t\t\)', ''
    $actual = $split.Block -replace '\(on_board no\)', '(on_board yes)'
    Assert 'RV1''s block differs only by the tag and the removed property' `
        ($actual -eq $expected) 'the block changed in some other way'

}
finally {
    Close-Mcp $mcp
}

# ── KiCAD reads the result ───────────────────────────────────────────────────
$ercAfter = Invoke-Erc 'after'
Write-Host "ERC after : $($ercAfter.Errors) errors, $($ercAfter.Warnings) warnings"
Assert 'kicad-cli still parses the schematic and reports the same ERC' `
    ($ercAfter.Errors -eq $ercBefore.Errors -and $ercAfter.Warnings -eq $ercBefore.Warnings) `
    "before $($ercBefore.Errors)/$($ercBefore.Warnings), after $($ercAfter.Errors)/$($ercAfter.Warnings)"

Write-Host ''
if ($failures) {
    Write-Host ("FAILED: " + ($failures -join ', ')) -ForegroundColor Red
    Write-Host "work dir kept for inspection: $work"
    exit 1
}
Write-Host 'ALL CHECKS PASSED' -ForegroundColor Green
Write-Host "original kept in $before"
# kicad-cli was asked for --exit-code-violations, so the last native exit code
# is the ERC warning count. Say what this script found, not what it last ran.
exit 0
