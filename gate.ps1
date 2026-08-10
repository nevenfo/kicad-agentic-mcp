# The quality gate. Mirrors .github/workflows/ci.yml so a local pass means the
# same thing CI means, and adds the benchmark so a token/latency regression is
# as loud as a test failure.
#
#   .\gate.ps1            format + clippy + tests + release build
#   .\gate.ps1 -Bench     the above, plus the golden benchmark suite
#
# PROTOC is set explicitly: konnect-ipc/build.rs needs it, and winget's PATH
# edit does not apply to an already-open shell.

param(
    [switch]$Bench,
    [int]$Repeat = 3
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
Set-Location $root

if (-not $env:PROTOC) {
    $protoc = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter protoc.exe -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($protoc) { $env:PROTOC = $protoc.FullName }
}

function Step($name, $block) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit $LASTEXITCODE)" }
}

Step "fmt"     { cargo fmt --all -- --check }
Step "clippy"  { cargo clippy --workspace --locked -- -D warnings }
Step "test"    { cargo test --workspace --locked --lib --tests }
Step "doctest" { cargo test --workspace --locked --doc }
Step "build"   { cargo build --release -p konnect }

if ($Bench) {
    # Both loading strategies, every time. `toolsets` is what existing clients
    # and skills do; `tools` is the recommended path and the headline number.
    # Tracking only one of them lets the other rot.
    Step "benchmark (toolsets)" {
        python bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-toolsets" --repeat $Repeat `
            --load-mode toolsets --out "bench\results\latest-toolsets.json"
    }
    Step "benchmark (tools)" {
        python bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-tools" --repeat $Repeat `
            --load-mode tools --out "bench\results\latest-tools.json"
    }
    Step "retrieval" {
        python bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-search" --repeat 1 `
            --load-mode search --out "bench\results\latest-search.json"
    }
    Step "surface" {
        python bench\surface.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)" `
            --out "bench\results\latest-surface.json"
    }
}

Write-Host "`nGATE PASSED" -ForegroundColor Green
