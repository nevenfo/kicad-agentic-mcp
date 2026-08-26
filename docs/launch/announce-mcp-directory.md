# Draft — MCP directories and server lists

**Not posted.** Requirements and the go/no-go list are in
[`launch-kit.md`](launch-kit.md). Most of these take a one-line PR; a few take a
web form with a longer field. All four lengths below are ready to paste.

## One line (for an `awesome-mcp-servers`-style list)

```markdown
- [KiCad Agentic MCP](https://github.com/nevenfo/kicad-agentic-mcp) — 🦀 🏠 Native KiCad 10 plugin exposing 202 tools for schematic capture, PCB layout and routing, ERC/DRC and manufacturing output; verification comes from `kicad-cli` rather than the model.
```

Most such lists use emoji legends for language and scope. `🦀` is Rust and `🏠`
is local service in the common legend; **check the list's own legend before
submitting** — several use different symbols, and a wrong one is the usual
reason a PR gets bounced.

Category, where the list has them: *Developer tools* or *Design / CAD*, not
*Data*.

## Short description (form field, ~200 characters)

> Native KiCad 10 plugin — one Rust binary — giving MCP clients 202 tools for
> schematics, PCB layout, routing, ERC/DRC and manufacturing output. KiCad
> itself verifies the result.

## Long description (form field, no length pressure)

> **KiCad Agentic MCP** is a native KiCad 10 plugin that exposes 202 tools
> across 22 on-demand toolsets over the Model Context Protocol. An MCP client
> can place and wire schematic parts by pin name, place and route footprints in
> the running PCB editor through KiCad's IPC API (so KiCad's own undo applies),
> run ERC, DRC, decoupling and power-rail audits, search a local JLCPCB parts
> catalogue, and produce Gerbers, drill files, BOM, pick-and-place, 3D models
> and PDF.
>
> Verification is KiCad's: `kicad-cli` returns the verdict, and a check that
> could not run is reported as an error rather than as a clean board.
>
> The catalogue does not have to be loaded to be used. Routing through two
> meta-tools measured 1 995 external tokens per task against 12 373 for the
> equivalent flat surface (−83.9 %), with the success rate unchanged on the
> repository's golden suite; the harness, the machine and the artefacts are all
> committed. Those figures were measured on v1.0.0 and have not been re-run
> since.
>
> Requirements: KiCad 10. PCB tools need KiCad running with its API enabled and
> the board open — pcbnew has no headless path. Windows is the tested platform;
> macOS binaries are not signed or notarised; Linux compiles and passes CI but
> has had no QA against a running KiCad. Licence: AGPL-3.0. It is a fork of
> [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2 under the
> same licence.

## Installation snippet (most directories ask for one)

```json
{
  "mcpServers": {
    "konnect": {
      "command": "C:\\Users\\<you>\\Documents\\KiCad\\10.0\\3rdparty\\plugins\\com_github_mixelpixx_konnect\\bin\\konnect.exe"
    }
  }
}
```

Keep this snippet in step with the README's Quick start; if v1.1.1 ships before
these entries are submitted, the two manual configuration keys the current
release needs are gone and the snippet above is complete as it stands.

## Metadata most forms ask for

| Field | Value |
|---|---|
| Name | KiCad Agentic MCP |
| Repository | `https://github.com/nevenfo/kicad-agentic-mcp` |
| Licence | AGPL-3.0 |
| Language | Rust |
| Transport | stdio (HTTP also available) |
| Scope | local |
| Requires | KiCad 10 installed; a running KiCad for the PCB tools |
| Author account | the maintainer's GitHub account — the user's decision, per R.4.6 |
