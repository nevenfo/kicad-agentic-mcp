<#
.SYNOPSIS
    Replays the two defective Hi-Fi footprints through Konnect alone (W.2.5).

.DESCRIPTION
    Two footprints on a real board carry the defects phase W.2 exists to fix,
    and both were written by an earlier `create_footprint`:

      - `CF_Film_Box_P5.00mm_7.2x3.5mm` — a 3.5 mm-deep film capacitor whose
        courtyard is 2.6 mm deep, because the courtyard was derived from the
        pad envelope alone. Its F.Fab outline, drawn from the body, then sits
        *outside* its own courtyard: three layers, three different claims about
        how much room the part needs.
      - `Fuse_Schurter_UMT-H_5.3x16mm` — a fuse, which has no pin 1, carrying a
        pin-1 silk dot whose centre is at x = -9.3 while the courtyard stops at
        -9.0.

    The point of this script is that both are corrected without leaving the MCP
    — no text editor, no footprint editor. It exercises the two halves W.2 adds:

      A. `create_footprint` regenerates each footprint from the same pad layout
         plus the body it always had, and the courtyard now covers both.
      B. `set_footprint_graphics` corrects the *existing* fuse in place, on one
         layer, leaving every pad and every other layer byte for byte as they
         were — the path for a footprint you do not want to regenerate.

    No KiCAD process is needed: both tools are pure S-expression paths.

.PARAMETER Library
    The `.pretty` directory holding the two footprints. Defaults to the Hi-Fi
    benchmark's local library.

.PARAMETER Konnect
    Konnect binary under test. Defaults to `target/release/konnect.exe`.

.PARAMETER InPlace
    Write the corrected footprints into `-Library` itself. Without it — the
    default — the library is copied to a work directory first and the real one
    is never touched.
#>
[CmdletBinding()]
param(
    [string]$Library,
    [string]$Konnect,
    [switch]$InPlace
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not $Konnect) { $Konnect = Join-Path $repo 'target\release\konnect.exe' }
if (-not (Test-Path $Konnect)) { throw "konnect binary missing: $Konnect" }

if (-not $Library) {
    $Library = Join-Path $env:USERPROFILE 'Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255_Local.pretty'
}
if (-not (Test-Path $Library)) { throw "footprint library missing: $Library" }

$work = Join-Path ([IO.Path]::GetTempPath()) "konnect-live-fp-$PID"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force $work | Out-Null

# The originals are kept whatever happens, so a run that ends badly still leaves
# the two files it started from.
$before = Join-Path $work 'before'
New-Item -ItemType Directory -Force $before | Out-Null
Copy-Item (Join-Path $Library '*.kicad_mod') $before -Force

$target = $Library
if (-not $InPlace) {
    $target = Join-Path $work 'lib.pretty'
    New-Item -ItemType Directory -Force $target | Out-Null
    Copy-Item (Join-Path $Library '*.kicad_mod') $target -Force
}
Write-Host "library under test: $target"
Write-Host "originals kept in : $before"

$capName = 'CF_Film_Box_P5.00mm_7.2x3.5mm'
$fuseName = 'Fuse_Schurter_UMT-H_5.3x16mm'
$capPath = Join-Path $target "$capName.kicad_mod"
$fusePath = Join-Path $target "$fuseName.kicad_mod"
foreach ($p in @($capPath, $fusePath)) {
    if (-not (Test-Path -LiteralPath $p)) { throw "footprint missing: $p" }
}

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
        clientInfo = @{ name = 'live-footprint-fix'; version = '0' }
    })
    # Toolsets are opt-in: an unloaded tool answers `toolset_not_loaded`, which
    # would make a refusal assertion pass for the wrong reason.
    $loaded = Invoke-Tool $state 'load_toolset' @{ name = 'library' }
    if ($loaded.IsError) { throw "load_toolset('library') failed: $($loaded.Text)" }
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

# ── Reading a footprint back ─────────────────────────────────────────────────
function Get-Info {
    param($State, [string]$Path, [string]$Layer)
    $callArgs = @{ footprint_path = $Path }
    if ($Layer) { $callArgs.graphics_layer = $Layer }
    $r = Invoke-Tool $State 'get_footprint_info' $callArgs
    if ($r.IsError) { throw "get_footprint_info failed for ${Path}: $($r.Text)" }
    return $r.Text | ConvertFrom-Json
}

# The courtyard rectangle. An object rather than a four-element array, because
# PowerShell unrolls arrays through the pipeline and a silently flattened bound
# would make every containment check pass.
function Get-Courtyard {
    param($Info)
    $rect = $Info.graphics | Where-Object { $_.layer -eq 'F.CrtYd' -and $_.type -eq 'rect' } |
        Select-Object -First 1
    if (-not $rect) { throw 'no F.CrtYd rectangle in the footprint' }
    [pscustomobject]@{
        MinX = [Math]::Min($rect.start.x, $rect.end.x)
        MinY = [Math]::Min($rect.start.y, $rect.end.y)
        MaxX = [Math]::Max($rect.start.x, $rect.end.x)
        MaxY = [Math]::Max($rect.start.y, $rect.end.y)
    }
}

# Courtyard width and depth, in mm.
function Get-CourtyardSize {
    param($Info)
    $c = Get-Courtyard $Info
    [pscustomobject]@{ Width = $c.MaxX - $c.MinX; Depth = $c.MaxY - $c.MinY }
}

# Every coordinate a footprint draws, on any layer. Texts are not graphics here:
# `get_footprint_info` reports shapes, and reference/value sit outside the
# courtyard by design, exactly as they do in KiCAD's own libraries.
function Get-DrawnPoints {
    param($Info)
    $points = [Collections.Generic.List[object]]::new()
    function Add-Point { param($X, $Y) $points.Add([pscustomobject]@{ X = [double]$X; Y = [double]$Y }) }
    foreach ($g in $Info.graphics) {
        foreach ($key in @('start', 'mid', 'end', 'center')) {
            if ($g.PSObject.Properties.Name -contains $key -and $null -ne $g.$key) {
                Add-Point $g.$key.x $g.$key.y
            }
        }
        if ($g.type -eq 'circle' -and $null -ne $g.radius_mm) {
            # A circle reaches `radius` past its centre in every direction.
            Add-Point ($g.center.x - $g.radius_mm) $g.center.y
            Add-Point ($g.center.x + $g.radius_mm) $g.center.y
            Add-Point $g.center.x ($g.center.y - $g.radius_mm)
            Add-Point $g.center.x ($g.center.y + $g.radius_mm)
        }
        if ($null -ne $g.points) {
            foreach ($p in $g.points) { Add-Point $p.x $p.y }
        }
    }
    return , $points.ToArray()
}

# Every drawn point that falls outside the courtyard, described for the report.
function Get-PointsOutsideCourtyard {
    param($Info)
    $c = Get-Courtyard $Info
    $outside = [Collections.Generic.List[string]]::new()
    foreach ($p in (Get-DrawnPoints $Info)) {
        if (($p.X -lt ($c.MinX - 1e-9)) -or ($p.X -gt ($c.MaxX + 1e-9)) -or
            ($p.Y -lt ($c.MinY - 1e-9)) -or ($p.Y -gt ($c.MaxY + 1e-9))) {
            $outside.Add("($($p.X), $($p.Y))")
        }
    }
    return , $outside.ToArray()
}

# The `(pad …)` lines of a .kicad_mod, in order. W.2 is about graphics: a
# regenerated footprint that moved a pad would be a different land pattern.
function Get-PadLines {
    param([string]$Path)
    Get-Content -LiteralPath $Path | ForEach-Object { $_.Trim() } |
        Where-Object { $_.StartsWith('(pad ') }
}

$failures = @()
function Assert {
    param([string]$Name, [bool]$Condition, [string]$Detail)
    if ($Condition) { Write-Host "PASS $Name" -ForegroundColor Green }
    else { Write-Host "FAIL $Name — $Detail" -ForegroundColor Red; $script:failures += $Name }
}

$mcp = New-Mcp
try {
    # ── The defects, read through the tool that will also prove them gone ────
    $capBefore = Get-Info $mcp $capPath
    $capCrtBefore = Get-CourtyardSize $capBefore
    Assert 'the film capacitor starts with a courtyard shallower than its body' `
        ($capCrtBefore.Depth -lt 3.5) `
        "courtyard is $($capCrtBefore.Depth) mm deep for a 3.5 mm body"
    Assert 'the film capacitor starts with graphics outside its own courtyard' `
        ((Get-PointsOutsideCourtyard $capBefore).Count -gt 0) 'nothing was outside to begin with'

    $fuseBefore = Get-Info $mcp $fusePath
    $fuseDotBefore = $fuseBefore.graphics |
        Where-Object { $_.layer -eq 'F.SilkS' -and $_.type -eq 'circle' -and $_.fill -eq 'solid' }
    Assert 'the fuse starts with a pin-1 dot it has no pin 1 for' `
        ($null -ne $fuseDotBefore) 'no filled silk circle found'
    Assert 'that dot starts outside the courtyard' `
        ((Get-PointsOutsideCourtyard $fuseBefore).Count -gt 0) 'nothing was outside to begin with'

    $capPadsBefore = Get-PadLines $capPath
    $fusePadsBefore = Get-PadLines $fusePath

    # ── A. Regenerate both through `create_footprint` ────────────────────────
    # Same pads, same description, and the body each part always had. The film
    # capacitor's original was generated with the *height* of the package
    # (6.5 mm) as `body_height`; the name, the description and the datasheet
    # all say the footprint depth is 3.5 mm.
    $capArgs = @{
        output = $capPath
        name = $capName
        description = $capBefore.description
        pads = @(
            @{ number = '1'; type = 'thru_hole'; shape = 'rect';   x = -2.5; y = 0.0; width = 1.6; height = 1.6; drill = 0.8 },
            @{ number = '2'; type = 'thru_hole'; shape = 'circle'; x =  2.5; y = 0.0; width = 1.6; height = 1.6; drill = 0.8 }
        )
        body_width = 7.2
        body_height = 3.5
        package_type = 'through_hole'
        # A film capacitor is not polarised: it has no pin 1 to point at.
        pin1_marker = $false
    }
    $r = Invoke-Tool $mcp 'create_footprint' $capArgs
    Assert 'the film capacitor is regenerated through the MCP' (-not $r.IsError) $r.Text

    $fuseArgs = @{
        output = $fusePath
        name = $fuseName
        description = $fuseBefore.description
        pads = @(
            @{ number = '1'; type = 'smd'; shape = 'rect'; x = -6.875; y = 0.0; width = 3.75; height = 5.6 },
            @{ number = '2'; type = 'smd'; shape = 'rect'; x =  6.875; y = 0.0; width = 3.75; height = 5.6 }
        )
        body_width = 15.4
        body_height = 5.35
        package_type = 'smd'
        pin1_marker = $false
    }
    $r = Invoke-Tool $mcp 'create_footprint' $fuseArgs
    Assert 'the fuse is regenerated through the MCP' (-not $r.IsError) $r.Text

    # ── What the regenerated files now say ───────────────────────────────────
    $capAfter = Get-Info $mcp $capPath
    $capCrt = Get-CourtyardSize $capAfter
    Assert 'the film capacitor courtyard covers its 3.5 mm body plus clearance' `
        ($capCrt.Depth -ge (3.5 + 1.0 - 1e-9)) `
        "courtyard is $($capCrt.Depth) mm deep"
    Assert 'the film capacitor courtyard covers its 7.2 mm body plus clearance' `
        ($capCrt.Width -ge (7.2 + 1.0 - 1e-9)) `
        "courtyard is $($capCrt.Width) mm wide"
    $outside = Get-PointsOutsideCourtyard $capAfter
    Assert 'nothing the film capacitor draws leaves its courtyard' `
        ($outside.Count -eq 0) ($outside -join ', ')
    Assert 'the film capacitor keeps its land pattern' `
        (((Get-PadLines $capPath) -join "`n") -eq ($capPadsBefore -join "`n")) `
        'pad lines changed'
    Assert 'the film capacitor keeps its description' `
        ($capAfter.description -eq $capBefore.description) $capAfter.description

    $fuseAfter = Get-Info $mcp $fusePath
    $fuseCrt = Get-CourtyardSize $fuseAfter
    Assert 'the fuse carries no pin-1 mark at all' `
        (($fuseAfter.graphics | Where-Object { $_.fill -eq 'solid' }).Count -eq 0) `
        'a filled graphic is still drawn'
    Assert 'the fuse fab outline is a plain rectangle, not a chamfered polygon' `
        (($fuseAfter.graphics | Where-Object { $_.layer -eq 'F.Fab' -and $_.type -eq 'poly' }).Count -eq 0) `
        'a chamfered fab polygon is still drawn'
    $outside = Get-PointsOutsideCourtyard $fuseAfter
    Assert 'nothing the fuse draws leaves its courtyard' `
        ($outside.Count -eq 0) ($outside -join ', ')
    Assert 'the fuse courtyard covers its 15.4 mm body plus clearance' `
        ($fuseCrt.Width -ge (15.4 + 0.5 - 1e-9)) `
        "courtyard is $($fuseCrt.Width) mm wide"
    Assert 'the fuse keeps its land pattern' `
        (((Get-PadLines $fusePath) -join "`n") -eq ($fusePadsBefore -join "`n")) `
        'pad lines changed'

    # ── B. Correct the *original* fuse in place, one layer only ──────────────
    # The other half of the loop: a footprint you do not want to regenerate —
    # because it came from a vendor library, or because its pads were hand-fitted
    # — corrected on the one layer that is wrong.
    $inPlaceDir = Join-Path $work 'inplace'
    New-Item -ItemType Directory -Force $inPlaceDir | Out-Null
    $fuseCopy = Join-Path $inPlaceDir "$fuseName.kicad_mod"
    Copy-Item (Join-Path $before "$fuseName.kicad_mod") $fuseCopy -Force

    $silk = (Get-Info $mcp $fuseCopy 'F.SilkS').graphics
    Assert 'the original fuse silk holds the outline and the false dot' `
        ($silk.Count -eq 2) "F.SilkS holds $($silk.Count) primitives"

    $keep = @($silk | Where-Object { $_.type -ne 'circle' })
    $r = Invoke-Tool $mcp 'set_footprint_graphics' @{
        footprint_path = $fuseCopy
        selector = @{ layer = 'F.SilkS' }
        mode = 'replace'
        graphics = $keep
    }
    Assert 'set_footprint_graphics replaces the silk layer' (-not $r.IsError) $r.Text

    $silkAfter = (Get-Info $mcp $fuseCopy 'F.SilkS').graphics
    Assert 'the false dot is gone and the outline stayed' `
        ($silkAfter.Count -eq 1 -and $silkAfter[0].type -eq 'rect') `
        "F.SilkS now holds $($silkAfter.Count) primitives"

    # Everything that was not F.SilkS is untouched, line for line: the pads, the
    # fab outline, the courtyard, the description, the texts.
    $originalLines = Get-Content -LiteralPath (Join-Path $before "$fuseName.kicad_mod")
    $patchedLines = Get-Content -LiteralPath $fuseCopy
    $originalOther = $originalLines | Where-Object { $_ -notmatch 'F\.SilkS' }
    $patchedOther = $patchedLines | Where-Object { $_ -notmatch 'F\.SilkS' }
    Assert 'an in-place silk edit leaves every other line of the file alone' `
        (($originalOther -join "`n") -eq ($patchedOther -join "`n")) `
        'lines outside F.SilkS changed'

    # And the refusal, so the assertions above cannot be passing because the
    # tool silently accepts anything.
    $r = Invoke-Tool $mcp 'set_footprint_graphics' @{
        footprint_path = $fuseCopy
        selector = @{ layer = 'F.SilkS' }
        mode = 'replace'
    }
    Assert 'a replace with no graphics is refused rather than emptying the layer' `
        ($r.IsError) $r.Text
    $silkStill = (Get-Info $mcp $fuseCopy 'F.SilkS').graphics
    Assert 'the refused call left the silk layer as it was' `
        ($silkStill.Count -eq 1) "F.SilkS holds $($silkStill.Count) primitives"
}
finally {
    Close-Mcp $mcp
}

Write-Host ''
if ($failures) {
    Write-Host ("FAILED: " + ($failures -join ', ')) -ForegroundColor Red
    Write-Host "work dir kept for inspection: $work"
    exit 1
}
Write-Host 'ALL CHECKS PASSED' -ForegroundColor Green
Write-Host "originals kept in $before"
