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

# The bench needs tiktoken. `python` on PATH is whatever was installed last --
# here a 3.14 that does not have it -- so resolve an interpreter that can
# actually import it, once and loudly, instead of failing four steps later
# with a ModuleNotFoundError that looks like a bench bug. Only -Bench needs
# one, so a machine without it can still run the code half of the gate.
function Resolve-BenchPython {
    foreach ($candidate in @(@("py", "-3.11"), @("python"))) {
        $exe = $candidate[0]
        $pre = @($candidate | Select-Object -Skip 1)
        if (-not (Get-Command $exe -ErrorAction SilentlyContinue)) { continue }
        & $exe @pre -c "import tiktoken" 2>$null
        if ($LASTEXITCODE -eq 0) { return @($exe) + $pre }
    }
    throw "no Python interpreter on PATH can import tiktoken (tried 'py -3.11', 'python'); install it with: py -3.11 -m pip install tiktoken"
}

function Step($name, $block) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$name failed (exit $LASTEXITCODE)" }
}

Step "fmt"     { cargo fmt --all -- --check }
Step "clippy"  { cargo clippy --workspace --locked --all-targets -- -D warnings }
Step "test"    { cargo test --workspace --locked --lib --tests }
Step "doctest" { cargo test --workspace --locked --doc }
Step "build"   { cargo build --release -p konnect }

if ($Bench) {
    $py = Resolve-BenchPython
    $PyExe = $py[0]
    $PyArgs = @($py | Select-Object -Skip 1)
    Write-Host "bench interpreter: $PyExe $PyArgs" -ForegroundColor DarkGray

    # Both loading strategies, every time. `toolsets` is what existing clients
    # and skills do; `tools` is the recommended path and the headline number.
    # Tracking only one of them lets the other rot.
    Step "benchmark (toolsets)" {
        & $PyExe @PyArgs bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-toolsets" --repeat $Repeat `
            --load-mode toolsets --out "bench\results\latest-toolsets.json"
    }
    Step "benchmark (tools)" {
        & $PyExe @PyArgs bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-tools" --repeat $Repeat `
            --load-mode tools --out "bench\results\latest-tools.json"
    }
    # The headline mode. It is where the transaction guarantees (rollback,
    # base_revisions, operation_id) are actually exercised, so a regression here
    # is a correctness regression, not only a token one.
    Step "benchmark (gateway)" {
        & $PyExe @PyArgs bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-gateway" --repeat $Repeat `
            --load-mode gateway --out "bench\results\latest-gateway.json"
    }
    Step "retrieval" {
        & $PyExe @PyArgs bench\runner.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)-search" --repeat 1 `
            --load-mode search --out "bench\results\latest-search.json"
    }
    Step "surface" {
        & $PyExe @PyArgs bench\surface.py --server ".\target\release\konnect.exe" `
            --label "fork-$(git rev-parse --short HEAD)" `
            --out "bench\results\latest-surface.json"
    }
}

Write-Host "`nGATE PASSED" -ForegroundColor Green
