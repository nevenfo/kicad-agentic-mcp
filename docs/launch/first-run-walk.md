# First-run walk — v1.1.0, Windows

What a stranger actually experiences between the release page and a first task
verified by KiCad. Walked on 2026-08-26 against the **published** v1.1.0
artefacts; no local build was used at any point (plan item R.1, invariant
INV-R1).

This document is evidence, not marketing. Every step below either happened or
is recorded as not having happened.

## Machine and initial condition

| | |
|---|---|
| OS | Windows 11 Pro 26200 |
| KiCad | **10.0.3**, release build, wxWidgets 3.3.2 |
| KiCad install | `C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\` — a **per-user** install, which is what KiCad's installer produces when it is not run as administrator |
| `3rdparty\` before the walk | **empty** — no Konnect, no other plugin |
| MCP clients before the walk | none knew `konnect`: no `%APPDATA%\Claude\claude_desktop_config.json`, no `.mcp.json`, `claude mcp list` empty |

That initial condition is consumed by the first install and cannot be recreated
on this machine, which is why it is written down here.

## Artefact under test

```
konnect-pcm-v1.1.0-windows.zip
12 258 180 bytes
sha256 25fe29cac9b0f812dd337e5700e466db9dad769bdbbfa89c85b6e11d3d167dd0
```

Downloaded from the release page. 8 entries; `plugins/bin/konnect.exe` is
24 848 384 bytes, sha256 `57f272cb…1868c`.

## The walk

### 1 — Download

`gh release download v1.1.0 -p 'konnect-pcm-v1.1.0-windows.zip'`, or the same
file from the Releases page in a browser. **The release publishes no checksum
file**, so a user has nothing to compare a download against.

### 2 — Install through the Plugin and Content Manager

KiCad 10 → *Plugin and Content Manager* → **Install from File…** → select the
zip. About **9 clicks and dialogs** from launching KiCad to an installed plugin,
including a first-launch configuration wizard KiCad shows once and that no
document mentions.

The install runs **immediately when the file is selected**. The *Apply Pending
Changes* button stays greyed out with an empty queue — a user who expects to
confirm will look for a confirmation that never comes.

What the PCM then shows for this plugin, verbatim:

| Field | Value shown |
|---|---|
| Name | Konnect |
| Identifier | `com.github.mixelpixx.konnect` |
| Author | **mixelpixx** (`https://github.com/mixelpixx`) |
| Homepage | **`https://github.com/mixelpixx/Konnect`** |
| Version | 1.1.0 · 34,1 MB · stable · Compatible ✔ |

The author and homepage are the **upstream** project's, not this fork's. A user
who installs this package and then looks for its issue tracker lands in a
different repository.

### 3 — Where it landed

Read from disk, not assumed:

```
C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect\
  bin\konnect.exe            23.7 MB
  bin\schematic-viewer.exe    8.8 MB
  plugin.json  __init__.py  settings_dialog.py  resources\icon.png
```

That is character for character the path the README and both files in
`examples/` publish. The installed `konnect.exe` is **byte-identical** to the
one inside the published zip (same sha256), and answers `konnect 1.1.0`.

### 4 — Plugin visible inside KiCad

Not established during this walk. Two attempts to open *PCB Editor → Tools →
External Plugins* were interrupted by a Windows UAC secure-desktop dialog
unrelated to KiCad. Recorded as **unproven**, not as passed.

A second friction point surfaced on the way there: **pcbnew refuses to open at
all without a project** — *« Créer (ou ouvrir) un projet pour modifier un
pcb. »* The README's verification step ("open the PCB Editor → Tools → External
Plugins") therefore cannot be followed by a user who has just installed KiCad
and has no project yet.

### 5 — MCP connection

The installed binary was driven over stdio with a plain JSON-RPC handshake —
`initialize`, `notifications/initialized`, `tools/list`:

```
protocolVersion 2025-06-18
serverInfo      { "name": "konnect", "version": "1.1.0" }
tools/list      21 tools
```

**21 starter-kit tools**, exactly the figure the README claims. The connection
needs nothing beyond the path from step 3.

### 6 — A real KiCad project

`C:\Users\FlowUP\Documents\r1-walk-test\` — created by KiCad itself during step
4, not a repository fixture. Pre-state: an empty A4 sheet, `.kicad_sch` of
**230 bytes**, `lib_symbols` empty, `.kicad_pro` literally `{}`.

### 7 — First task

Through the gateway, in one call:

```json
{"name":"kicad_invoke","arguments":{
  "calls":[{"tool":"apply_template","args":{
    "schematic":"…\\r1-walk-test.kicad_sch",
    "template_id":"ldo_3v3","position_x":100,"position_y":80}}],
  "verify":"auto"}}
```

**108 ms**, process startup included. Five symbols placed — `U1`
`Regulator_Linear:AMS1117-3.3`, `C1`/`C2` 10 µF, `C3`/`C4` 100 nF — with the
design notes and the list of connections still to wire. The schematic went from
230 to 2 576 bytes.

Two things a reader should not have to guess:

- `kicad_invoke` takes `calls: [{tool, args}]`. Calling it with `name` and
  `arguments`, the shape every other MCP tool uses, fails with
  `Argument 'calls' is invalid: missing`.
- `apply_template` **places, it does not wire**. It returns
  `connections_to_wire` and a `next_steps` telling you to call `connect_to_net`.
  The tool's own description says "places all components and wires them
  according to the template's connection map", which is not what it did.

### 8 — KiCad's verdict

`verify:"auto"` was asked for and **did not run**:

```json
"validators":[{"check":"erc","document":"r1-walk-test.kicad_sch",
  "error":"Failed to spawn kicad-cli: kicad-cli.exe","error_kind":"io"}]
```

Run by hand against the real binary, KiCad's own ERC gives the verdict the
server could not:

```
kicad-cli.exe sch erc --exit-code-violations r1-walk-test.kicad_sch
Violations trouvées 0        exit 0
```

So the write is sound and KiCad reads the file it produced — but the server
could not say so itself, on a stock KiCad install. See F-01.

## Friction list

Each entry carries exactly one class (invariant INV-R3) and the surface that has
to change.

| # | Class | What a user hits | Where it lives |
|---|---|---|---|
| **F-01** | **product** | Every `kicad-cli`-backed capability — ERC, DRC, all exports, `verify:"auto"`, the viewer's rendering — fails with `Failed to spawn kicad-cli: kicad-cli.exe` on a stock per-user KiCad install | see below |
| **F-02** | documentation | The README says *"common install paths are auto-detected"*. For the server binary that is false: there is no runtime detection at all | `README.md` Troubleshooting |
| **F-03** | UX | The Plugin Manager shows author `mixelpixx` and homepage `github.com/mixelpixx/Konnect` — the upstream project. A user looking for this fork's issue tracker leaves for another repository | `packaging/metadata.json` |
| **F-04** | packaging | Seven assets, **no checksum file**. A download cannot be verified without rebuilding | `.github/workflows/release.yml` |
| **F-05** | documentation | The install-verification step (*PCB Editor → Tools → External Plugins*) is impossible before a project exists: pcbnew refuses to open without one | `README.md` Installation |
| **F-06** | documentation | `kicad_invoke` takes `calls: [{tool, args}]`, not `{name, arguments}`. Nothing a first-time reader sees says so before the error does | `README.md` / `tool-directory.md` |
| **F-07** | product | `apply_template`'s description claims it wires the components. It places them and returns the connections as work still to do | `crates/konnect-core/src/tools/templates.rs` |
| **F-08** | UX | The PCM install fires on file selection while *Apply Pending Changes* stays greyed out and empty — no confirmation step where a user expects one | KiCad's own UI; documentation only |

### F-01 in detail

The server's default is a bare command name:

```rust
// crates/konnect/src/config.rs:75
fn default_kicad_cli() -> String {
    if cfg!(target_os = "windows") { "kicad-cli.exe".to_string() } else { … }
}
```

resolved through `PATH` — and **KiCad's Windows installer does not put its `bin`
on `PATH`**. There is no fallback: `detect_kicad()`
(`crates/konnect/src/install.rs:402`) is called only by `run_install` and
`print_status`, never by the MCP server, and even when it is called it misses
this machine three times over:

- its Windows path list covers `C:\KiCad`, `C:\Program Files\KiCad` and
  `C:\Program Files (x86)\KiCad`, and **not** `%LOCALAPPDATA%\Programs\KiCad`,
  which is where the per-user installer puts KiCad. The macOS branch of the same
  function *does* handle its per-user case;
- its registry fallback queries `HKLM\SOFTWARE\KiCad\10.0`. On this machine
  neither `HKLM` nor `HKCU` has that key at all;
- `plugin/settings_dialog.py::detect_kicad_cli` also reads `HKEY_LOCAL_MACHINE`
  only.

The install that *is* recorded, and the anchor a fix would use:

```
HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\KiCad 10.0
  DisplayName     KiCad 10.0 (current user)
  InstallLocation C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0
```

This is classified **product** and it **directly blocks first use**: without it
the verdict that invariant INV1 makes the whole project's premise — "the verdict
is KiCad's" — cannot be obtained by the server on a default Windows install.
It is therefore inside Phase R's narrow exception for defects that block
installation, first use or the demo.

## What this walk did not cover

- macOS and Linux. Neither was installed or tested here; the release notes'
  existing caveats stand unchanged.
- A model driving the tools. Step 7 was executed by a scripted MCP client, which
  proves the path, not the experience. A model-driven run is the subject of the
  canonical demo (R.3).
- The plugin's own settings dialog and its *start server* button.
