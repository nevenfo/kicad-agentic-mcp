<a name="top"></a>

<div align="center">

<img src="resources/images/KiCAD-MCP-Server-rust.svg" alt="KiCad Agentic MCP logo" height="240" />

# KiCad Agentic MCP

</div>

**KiCad Agentic MCP is an MCP server and an agentic control layer for KiCad 10.**
An MCP client — Claude Desktop, Claude Code, or anything else that speaks the
[Model Context Protocol](https://modelcontextprotocol.io) — can place and wire
schematic symbols, place and route footprints in the running PCB editor, run ERC
and DRC, search parts, and produce manufacturing output.

What sits between the model and your design files is the reason this project
exists. A model states an intent; a typed plan is compiled and reference-checked
*before* anything is written; execution is deterministic code run as a
transaction with revision checks and rollback; and the verdict on whether a
change is correct comes from KiCad — `kicad-cli` ERC/DRC, or the design read back
— never from the model's own account of what it did. Task state, evidence and
diffs are held server-side, outside any model's context.

The model is asked to reason where reasoning helps: turning a vague request into
a plan. Everything that has to be exact is code.

**Status: v1.1.4**, AGPL-3.0. The server binary is called `konnect`. Every figure
below traces to [docs/benchmark.md](docs/benchmark.md) or to the document named
beside it; what was missed is stated as missed. Issues and PRs are welcome —
[CONTRIBUTING.md](CONTRIBUTING.md), [naming conventions](docs/NAMING_CONVENTIONS.md).

## The control model

```
User intent
    │
    ▼
LLM planning                      where reasoning is useful
    │
    ▼
Plan IR                           typed, reference-checked
    │
    ▼
Deterministic compile + refusal   a plan that cannot finish never starts
    │
    ▼
Transactional execution           revisions, idempotency, rollback
    │
    ▼
KiCad
    │
    ▼
Independent validation + evidence  kicad-cli, and the design read back
```

The bottom half runs whether or not a model was involved: a client that calls
tools directly gets the same compilation, transaction and validation path.

KiCad 10 offers three access routes, and the layer uses each for what it can
actually do — this is fixed by KiCad, not chosen by preference:

```
                          ┌── Schematic ──→ S-expression engine (atomic writes, no running KiCad)
Agentic control layer ────┼── PCB ────────→ KiCad IPC API (NNG + protobuf, KiCad must be running)
                          └── Verify /
                              export ─────→ kicad-cli (ERC, DRC, Gerber, drill, BOM, PDF, 3D)
```

Schematic IPC is empty on KiCad 10 — `schematic_commands.proto` declares no
commands — so the S-expression engine is the only path that exists, not a
workaround. Details and the KiCad-11 re-evaluation are in
[RELEASE_NOTES.md](RELEASE_NOTES.md#kicad-10-status).

### AI where useful, determinism where required

| Responsibility | Owner |
|---|---|
| Turn an ambiguous or high-level request into a plan | An LLM — your client's model, or an opt-in local model |
| Resolve the current project state | Deterministic — an indexed project graph built from the files and the board |
| Check that a plan's references exist and resolve | Deterministic — the plan compiler refuses before the first mutation |
| Compute coordinates | Deterministic — every coordinate the op library emits is grid-snapped |
| Apply the mutation | Deterministic tool handlers |
| Concurrency, idempotency, rollback | Deterministic — revisions, an idempotency ledger, a snapshot journal |
| ERC / DRC | KiCad, through `kicad-cli` |
| Decide whether the operation succeeded | Validators and evidence records, not model prose |

The LLM row is the only one a model owns, and it is optional: by default the
planning model is whatever your MCP client runs. Nothing in the server calls a
model unless you ask it to.

## What one prompt does

<table>
<tr><th width="50%">Before</th><th width="50%">After</th></tr>
<tr>
<td><img src="resources/images/demo-before.png" alt="Three footprints on the board, two capacitors sitting away from the regulator, no copper" /></td>
<td><img src="resources/images/demo-after.png" alt="The two capacitors placed either side of the regulator, three nets closed in copper" /></td>
</tr>
</table>

One prompt, on the starting board committed in [`examples/demo/`](examples/demo/)
— reproducible from the same files, not an illustration. The prompt asks for the
two capacitors to be placed within 5 mm of the regulator, three nets routed, and
DRC run.

**The verdict is KiCad's:** `kicad-cli pcb drc` reports **5** unconnected items
on the pre-state and **0** afterwards, with 11 track segments and no errors. Run
twice from the same state, it produced the same circuit both times at different
coordinates. Board edits arrive through KiCad's IPC API, so Ctrl+Z walks them
back like any manual edit.

**Two numbers, because they measure two different things:** the board changes
land in **under a second** (0.686 s and 0.773 s of server time), while the prompt
around them takes **6 to 7 minutes**, because the model routes one segment per
turn. Both runs, call by call, are in [`demo-run-2.md`](docs/launch/demo-run-2.md)
and [`demo-run-3.md`](docs/launch/demo-run-3.md); they were run against the
published v1.1.0 binary.

## How execution is controlled

| Mechanism | What it does |
|---|---|
| Plan IR (`preview_plan` / `apply_plan`) | One operation expands into many tool calls, and a later one may reference an earlier one's output. A reference that does not resolve is refused before the first mutation; `preview_plan` returns the exact calls and changes nothing. |
| Gateway (`kicad_describe` / `kicad_invoke`) | Calls any tool by name without adding it to `tools/list`, so the client is never forced to re-fetch the catalogue. `kicad_invoke` takes a batch, stops at the first failure by default, and reports `failed_at` and `not_run`. |
| Transactions and rollback | A batch runs against a directory snapshot; a partial failure restores the before-images. Every mutation leaves an entry in an append-only run journal. |
| Optimistic concurrency and idempotency | `base_revisions` are content-addressed, so an edit made in another window is detected instead of overwritten; an `operation_id` ledger returns the first result on a retry. `changes_since` reports what happened after a revision token. |
| Editor-lock refusal | A `.kicad_sch` KiCad has open is refused (`error_kind: conflict`), re-checked immediately before the commit, leaving the file byte-identical. |
| Semantic diff and evidence handles | Changes are reported by stable item key; snapshots and diffs are addressable MCP resources (`kicad://snapshot/N`, 64 entries). Validator findings carry stable ids, so a fix is an id that disappeared. |
| Task state outside the context | Objective, constraints, verified facts, failed attempts and evidence live server-side and survive a context compaction or a model swap. |
| Project graph | An indexed world model with filtered lookups and spatial neighbours, so a query replaces dumping a whole design. |
| Operating modes | `ReadOnly`, `Write` (default), `Manufacturing`, `Experimental`, enforced before a handler runs so a refusal has nothing to roll back. |
| Structured refusal | A check that could not run is reported as an error, never as a check that passed. |

Architecture, crate by crate, is in [DEV.md](DEV.md#the-agent-layer).

## Measured results

**What was measured.** Seven scripted tasks (schematic authoring, hierarchical
sheets, a template, exports, inspection, and a recovery task where five wrong
inputs must each fail loudly), each starting from an empty directory, five
repeats. Run on 2026-08-24 on one machine — Windows 11, Ryzen 7 9800X3D, KiCad
10.0.3 — against **mixelpixx/Konnect v0.2.2 at commit `5cd6454`**, unmodified,
driven by the same scripted oracle. Success is decided by `kicad-cli` ERC and by
reading the design back, never by a model's prose. Full method, artefacts and
the reproduction commands: [docs/benchmark.md](docs/benchmark.md).

| | Konnect v0.2.2 baseline | This fork, gateway route |
|---|---|---|
| runs passed | 35/35 | 35/35 |
| MCP calls, median per task | 11 | 4 |
| external tokens, median per task | 14 337 | 2 249 |
| wall clock, P50 | 77 ms | 86 ms |

"External tokens" is tool output plus the `tools/list` refreshes the server
forces on the client — a cost the caller cannot decline, and the reason the two
columns differ so much: the gateway route adds nothing to `tools/list`.

**What the same run missed, unmoved:**

- **Wall clock is worse**: 86 ms against 77 ms. The fork loses where it
  guarantees something — the recovery task, which exercises the journal, the
  snapshot manifest and the evidence store, costs +109 ms — and wins where there
  is nothing to guarantee (inspection, 14 → 6 ms).
- **External tokens missed their own target** (≤ 2 000, measured 2 249), and
  **startup `tools/list` missed its target** (≤ ~1 000, measured 2 831 tokens for
  21 tools).
- **Success rate is equal, not ahead.** A scripted route succeeds by construction
  on both servers; what this measures is what the route costs.
- The baseline column was run with one extra toolset loaded, because this fork
  moved `export_bom` between toolsets. Without that, upstream fails the export
  task outright on a taxonomy difference rather than a missing capability; both
  runs are in the benchmark document.

**What this does not measure.** One machine, one KiCad version, scripted routes,
and schematic and export paths only — the golden suite contains no PCB task,
because PCB work needs a running KiCad. It says what a surface costs on that
suite. It does not predict success on an arbitrary KiCad project, and it is not a
statement about Konnect as it stands today.

**The local-model runtime** (`kicad_agent`) is the opt-in path where the server
itself calls a model on loopback: the caller states an objective, the model
writes a Plan IR, and the server compiles, applies and verifies it, with
`kicad-cli` returning the verdict. It has built two of the benchmark designs end
to end, one run each — **no success rate is claimed from that**. Where it was
measured with a sample big enough to carry a number, `gpt-oss-20b` at medium
reasoning wrote a fully correct plan on 16 of 60 one-shot attempts and a
compilable one on 53 of 60. Treat it as working and early, not as a hands-off
autopilot.

## Capabilities

| Area | What it covers |
|---|---|
| Schematic | Symbols, wires, junctions, net labels, power symbols, buses; pin-to-pin connection by name; hierarchical sheets and sheet pins; batch edits |
| PCB | Footprint placement, alignment and duplication; traces, vias, copper pours, net classes, differential pairs; outline, layers, zones, mounting holes, silkscreen |
| Inspection | Net connectivity, pin queries, trace paths, overlap and orphan detection, project-graph queries |
| Validation | ERC and DRC through `kicad-cli`; decoupling, connection, power-rail, DFM and BOM-health audits |
| Fabrication | Gerber, drill, pick-and-place, BOM, netlist, PDF, SVG, 3D, DXF/GenCAD/IPC-2581/ODB++; a fab package with cost estimation and fab-house validation |
| Parts and libraries | Library search and registration, footprint creation and graphics editing, a local JLCPCB parts cache, datasheet lookup, Freerouting autoroute |
| Reference circuits | USB-C, LDO, buck converter, STM32, I2C and LED templates with verified values |
| Bundled for the client | 6 skills and 2 agents carrying KiCad conventions |

**Surface, as a technical fact rather than a headline:** 203 domain tools across
22 toolsets, plus 13 always-visible meta-tools — 216 served in total. The client
does not see them all: startup exposes a **21-tool** starter kit, toolsets load on
demand, and the gateway calls anything by name without listing it at all. Every
tool, with its source file, is in [tool-directory.md](tool-directory.md).

Of the 186 tools inherited from Konnect v0.2.2, 137 (73.7 %) have a test that
actually runs in this repository; `#[ignore]`d tests needing a live KiCad GUI do
not count. That matrix is generated from the source and a test fails if it has
drifted — [docs/capability-matrix.md](docs/capability-matrix.md).

## Quick start

Five steps from the release page to a change KiCad itself confirms. Walked on a
machine that had never had this plugin installed; the record, including what went
wrong, is [docs/launch/first-run-walk.md](docs/launch/first-run-walk.md). That
walk measured about nine clicks and dialogs, two KiCad restarts, and a first task
that came back in 108 ms.

**Before you start** you need KiCad 10 and an MCP client. Nothing else: no Node,
no Python, no package tree. Windows is the platform with the most live testing —
see [Known limitations](#known-limitations).

**1 — Download the plugin package.** From
[Releases](https://github.com/nevenfo/kicad-agentic-mcp/releases), take
`konnect-pcm-v<version>-windows.zip` (or `-macos.zip` / `-linux.zip`). The
`konnect-pcm-*` assets are the KiCad plugin packages; the other archives are
standalone server binaries you do not need for this path.

**2 — Install it.** KiCad 10 → **Plugin and Content Manager** → **Install from
File…** → pick the zip. It installs the moment you select the file — the *Apply
Pending Changes* button stays greyed out and there is nothing further to confirm.
Restart KiCad.

**3 — Turn on the KiCad API.** *Preferences → Plugins* → check **Enable KiCad
API**, then restart KiCad. KiCad ships this **off**, and every PCB tool here
talks to KiCad through it. Schematic editing and exports work without it; live
board editing does not. After the restart the same page should read
`Listening on ipc://…`.

**4 — Point your MCP client at the server.** After a PCM install the binary lives
in your KiCad documents folder:

```
C:\Users\<YOU>\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect\bin\konnect.exe
```

Put that path in your client's MCP config — `%APPDATA%\Claude\claude_desktop_config.json`
for Claude Desktop, a `.mcp.json` in your project root for Claude Code:

```json
{
  "mcpServers": {
    "konnect": {
      "command": "C:\\Users\\<YOU>\\Documents\\KiCad\\10.0\\3rdparty\\plugins\\com_github_mixelpixx_konnect\\bin\\konnect.exe"
    }
  }
}
```

Copy-paste versions of both files are in [examples/](examples/). Restart the
client; the server should report **21 tools** at startup — the starter kit. The
rest of the catalogue loads on demand, or is called through the gateway without
ever appearing in `tools/list`. **No settings file is required** for a standard
KiCad 10 install: the server discovers `kicad-cli`, the KiCad GUI binary and the
IPC address. Explicit settings remain available for unusual or portable
installations.

**5 — Give it something to do**, with a KiCad project open. For example:

> *Add a 3.3 V LDO regulator subcircuit to my schematic and run ERC on it.*

The reply should name the parts it placed — a regulator, its input and output
capacitors — and carry an ERC result that came from `kicad-cli`, not from the
model. Open the schematic in KiCad: the symbols are there.

**Check the install itself** at any point: open a project (KiCad's PCB editor
refuses to open without one), then **PCB Editor → Tools → External Plugins**,
where you should see **Konnect**.

**If any of those five steps did not work for you**, that is the thing worth
reporting: [file a first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml).
It takes about two minutes and it is the only way any of this gets measured on a
machine that is not the maintainer's.

### Requirements

- **KiCad 10.** Built and measured against 10.0.3; the live schematic and PCB
  suites of v1.1.4 were also run against 10.0.6. Other 10.0.x releases are
  untested here rather than known-bad.
- **`kicad-cli`**, which ships with KiCad and is used for exports, ERC and DRC.
  KiCad's installer does not put it on `PATH`; the server searches the usual
  install locations and logs which one answered, and you can name it explicitly.
- **For PCB tools: KiCad running** with the target board open and the KiCad API
  switched on (*Preferences → Plugins → Enable KiCad API*, which ships off).

### Build from source

```bash
# protoc is required (protobuf code generation), and cmake (the nng crate
# compiles the NNG C library with it).
# Windows: choco install protoc cmake
# macOS:   brew install protobuf cmake
# Linux:   apt install protobuf-compiler cmake
cargo build --release -p konnect
```

The resulting `target/release/konnect` is the MCP server. The published v1.1.0
Windows binary measured 23.7 MB, unstripped; there is no `[profile.release]`
override, so a local build is the same shape.

### macOS

The PCM package bundles a universal binary; the standalone archives ship
Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`) builds.
**They are not code-signed or notarised**, so Gatekeeper refuses them on first
launch — clear the quarantine attribute:

```bash
xattr -dr com.apple.quarantine ./konnect   # or the folder KiCad installed it into
```

KiCad on macOS keeps its tools inside the app bundle and off `PATH`; the server
searches the standard bundle and uses KiCad's default IPC address. Only an
unusual or renamed install needs overrides in
`~/Library/Application Support/konnect/config.toml`:

```toml
kicad_cli = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"
kicad_binary = "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad"
# Optional override; the default is ipc:///tmp/kicad/api.sock.
ipc_address = "ipc:///tmp/kicad/api.sock"
```

Claude Desktop's config lives at
`~/Library/Application Support/Claude/claude_desktop_config.json` and takes the
same `mcpServers` snippet with a Unix path.

### Schematic viewer

A standalone viewer that auto-refreshes as the schematic file changes:

```bash
schematic-viewer.exe path\to\your\root_schematic.kicad_sch
```

Point it at the root sheet of a hierarchical design and every sub-sheet is
rendered too, with a depth-indented selector. Only the sheets that changed
re-render, against temp-folder snapshots, so the viewer never blocks KiCad from
saving. The AI can launch it via `open_schematic_viewer`. Needs the WebView2
runtime (pre-installed on Windows 10/11) and `kicad-cli`; built separately from
the main workspace — see [DEV.md](DEV.md).

### Transport

MCP JSON-RPC over stdio by default. Streamable HTTP is available with
`transport = "http"` (or `"both"`): POST and GET/SSE on a single `/mcp`
endpoint, with Origin validation and a `/health` probe.

## Known limitations

Stated as four different things, because they are:

**Tested — Windows.** The first-run walk, the demo runs, the benchmark and the
live schematic and PCB suites all ran on Windows 11 with KiCad 10.0.3 (and 10.0.6
for the live suites of this release).

**Supported but less tested — macOS and Linux.** macOS works from the release
binaries or a source build and is not signed or notarised. Linux compiles and
passes CI but has had **no per-platform QA against a running KiCad**. Both are on
the [roadmap](ROADMAP.md).

**Experimental — the local-model runtime.** It works, and its measured plan
quality is the figure quoted above (16/60 fully correct one-shot). It is not a
hands-off autopilot, and no success rate is claimed for it on real projects.

**Not yet built, or not possible here:**

- **PCB tools need a running KiCad** with the API on and the board open. pcbnew
  has no headless mode, so there is no headless PCB path. A desktop session is
  required; a human watching it is not.
- **Symbols and footprints are placed, not authored from scratch.** Footprints
  can be created and their graphics edited; authoring new library symbols is on
  the [roadmap](ROADMAP.md).
- **Schematic editing does not go through IPC** because KiCad 10 exposes no
  schematic IPC commands. This is re-evaluated at KiCad 11, not before.
- **A schematic open in Eeschema is refused, not merged.** Close the editor, or
  work on a copy.
- The benchmark covers a controlled seven-task suite on one machine. It does not
  generalise to arbitrary projects.
- The evidence store holds 64 entries; older handles age out and report
  differently from handles that never existed.

**No telemetry.** The binary reports nothing, anywhere, ever. Everything known
about how it behaves on other people's machines came from someone choosing to
write it down; the tally is [docs/adoption.md](docs/adoption.md).

## Troubleshooting

- **Plugin doesn't appear** — install via the Plugin and Content Manager, not a
  manual copy, then restart KiCad. The entry is under *Tools → External Plugins*
  in the **PCB editor**, which will not open until a project is open.
- **"IPC connect failed"** — two things must both be true: *Enable KiCad API* is
  checked, and KiCad is running with the board open. The API page reads
  `Listening on ipc://…` after a restart.
- **"Failed to spawn kicad-cli"** — the server tries your config value, `PATH`,
  the known install prefixes (including the per-user
  `%LOCALAPPDATA%\Programs\KiCad\<ver>\bin`), then the registry, and logs which
  one answered.
- **A validator reports an error instead of zero findings** — deliberate. A check
  that could not run is never reported as a check that passed.

Longer walkthroughs: [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md).

## Architecture and references

- [DEV.md](DEV.md) — architecture, the agent layer crate by crate, build steps
- [tool-directory.md](tool-directory.md) — every tool, generated from the source
- [docs/benchmark.md](docs/benchmark.md) — method, artefacts, every number and
  every missed target
- [docs/capability-matrix.md](docs/capability-matrix.md) — what has a test that
  runs, generated and gate-checked
- [docs/local-agents.md](docs/local-agents.md) — the local-model seam, and how it
  was measured
- [RELEASE_NOTES.md](RELEASE_NOTES.md) · [ROADMAP.md](ROADMAP.md) ·
  [CONTRIBUTING.md](CONTRIBUTING.md) ·
  [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)

## Relationship to Konnect

```
KiCAD-MCP-Server        Python/TypeScript, MIT, still open and maintained
       │
       ▼
    Konnect             Rust rewrite: one binary, KiCad 10 IPC, toolset router
       │
       ▼ forked from v0.2.2 (commit 5cd6454)
KiCad Agentic MCP       this repository
```

**What comes from upstream:** the Rust single-binary architecture, the KiCad 10
IPC path for the PCB, the S-expression schematic engine, `kicad-cli` for exports
and checks, and the toolset router — the client loads toolsets and calls tools by
name. All of that still works here, unchanged.

**What this fork adds:** the gateway (`kicad_describe` / `kicad_invoke`), the Plan
IR and its deterministic executor, transactions with rollback and a run journal,
revisions and an idempotency ledger, semantic diffs and evidence handles, task
state held outside any model's context, the indexed project graph, tool
capability annotations, and the opt-in local-model runtime. These are the `kam-*`
crates, clean-room and `MIT OR Apache-2.0` in their own manifests, with
KiCad-side adapters in `konnect-core`.

The benchmark above compares this fork against **Konnect v0.2.2 at commit
`5cd6454`**, the exact point it was forked from. It says nothing about Konnect as
it stands today, and no claim of superiority over the current upstream is made or
intended. Upstream remains a separate project under the same AGPL-3.0 licence.

The binary is still called `konnect` and the plugin identifier is still
`com.github.mixelpixx.konnect`, so an existing MCP client configuration keeps
working.

## License

AGPL-3.0-only, workspace-wide — see [LICENSE](LICENSE). Hobbyists, students,
freelancers and open-source projects can use it freely. For a business, the AGPL
requires that anything built on or around it — including software provided over a
network — be open-sourced under the same licence; if that does not work for you,
commercial licences are available: [COMMERCIAL.md](COMMERCIAL.md).

The generic `kam-*` crates (state, evidence, plan, graph, context, llm, runtime)
are clean-room and `MIT OR Apache-2.0` in their own manifests.

## Support

- **Tried it for the first time?**
  [File a first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml)
  — six questions, most of them one click. A report from someone who **gave up**
  is worth more than one from someone who succeeded.
- Bugs and feature requests: [GitHub Issues](https://github.com/nevenfo/kicad-agentic-mcp/issues)
- Installation or IPC problems: [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)
