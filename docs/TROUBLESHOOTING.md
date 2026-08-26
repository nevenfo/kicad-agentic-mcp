# Troubleshooting

## "KiCAD IPC socket path not configured"

Any tool that talks to a live KiCAD session (`save_project`, PCB editing,
`check_kicad_ui`, …) needs the IPC socket address. Two separate configurations
must both be correct — neither happens automatically:

1. **The socket path in Konnect's plugin settings** (inside KiCAD)
2. **The Konnect server registration in your AI client's MCP config**

Step by step (based on the diagnostic guide contributed in
[#18](https://github.com/mixelpixx/Konnect/issues/18)):

1. Open KiCAD normally.
2. Go to **Edit → Preferences → Plugins** and check **"Enable KiCad API"**.
   Confirm a line like this appears:

   ```
   Listening on ipc://C:\Users\<you>\AppData\Local\Temp\kicad\api.sock
   ```

   Copy the whole address including the `ipc://` prefix — it is unique to
   your machine and user.
3. In KiCAD, open **Tools → External Plugins → Konnect** to open the settings
   dialog.
4. Paste the address into the **IPC Socket** field and click **Save**.
5. Confirm your AI client (Claude Code, Claude Desktop, …) has the `konnect`
   MCP server registered in its own config (`.mcp.json` or
   `claude_desktop_config.json`) pointing at the `konnect` binary — see
   [examples/](../examples/). This registration is separate from the KiCAD
   plugin settings.
6. Restart the AI client session so it spawns a fresh Konnect process that
   reads the saved settings.
7. Verify: have the AI call `open_project`. Expected:

   ```json
   { "kicad_ui_running": true, "message": "KiCAD is running and IPC is available." }
   ```

Alternative: launching the server from within KiCAD sets `KICAD_API_SOCKET`
automatically, and a `konnect-settings.json` passed via `--config` can carry
`ipc_socket_path` directly.

## PCB tools return "IPC connect failed" / "No PCB document is open"

The IPC tools talk to KiCAD's **running PCB editor**. Open your board file in
KiCAD first, and make sure the API is enabled (previous section).

## A footprint I just placed is not where the pad tools say it is

PCB **writes** go to the running PCB editor over IPC; two PCB **reads** —
`get_component_pads` and `get_pad_position` — read the board **file** on disk.
So a footprint placed or moved through a running KiCAD keeps its old
coordinates in those two answers until KiCAD saves the board.

Both answers say which source they used (`"source": "file"`), and the tools
that read over IPC say `"source": "ipc"`. If the two disagree, save the board
in KiCAD (Ctrl+S) and read again — that is the whole of it. `get_component_list`
reads over IPC and therefore always reflects the live board.

## "kicad-cli not found"

Common install paths are auto-detected (including the Windows registry). If
your install is somewhere unusual, set the path in the plugin settings dialog
or in `konnect-settings.json` (`kicad_cli`).

## Transaction recovery is blocked by divergent content

Multi-file schematic changes persist a `.konnect-transaction-<id>.json`
write-ahead journal in the project before changing any target. On restart,
Konnect safely completes files that still match either the recorded before
image or intended replacement. It never overwrites a file changed by KiCad or
another process after the journal was written.

Inspect active journals without printing their contents:

```text
konnect transaction status <project-dir>
```

Each target is reported as `pending`, `applied`, or `divergent`. Retry safe
recovery with:

```text
konnect transaction recover <project-dir>
```

If a target is divergent, first inspect the schematic in KiCad and preserve
the version you want. To unblock future transactions without changing any
schematic file, explicitly abandon the journal:

```text
konnect transaction abandon <project-dir> <transaction-id> --force
```

Abandonment renames the journal to
`.konnect-transaction-<id>.abandoned.json`; it does not restore, replace, or
delete a target. The abandoned file is retained as recovery evidence and is
ignored by future transactions. Delete it only after you have made any backup
you need.

Active and abandoned journals contain complete before/after images of every
affected schematic. Treat them as sensitive, do not attach them to bug reports
without reviewing their contents, and do not commit them. Both forms are
ignored by the repository `.gitignore`.

Cooperative document locks are stored outside the project under the platform
local-data directory. Set `KONNECT_STATE_DIR` to an absolute directory to
override that location. A relative override is rejected rather than falling
back to project-local sidecars.

## Tools don't appear after `load_toolset`

After a successful `load_toolset` call the server sends a
`notifications/tools/list_changed` notification, and MCP clients are expected to
re-fetch `tools/list` in response. If newly loaded tools never show up:

1. Check your client honors `notifications/tools/list_changed` (most current MCP
   clients do; some cache the initial tool list forever).
2. Disable any competing tool-search or tool-filter layer sitting between the
   model and the server. A Chrome-extension "tool search" that shadowed the real
   tool list caused exactly this in
   [#67](https://github.com/mixelpixx/Konnect/issues/67).
3. Re-issue `tools/list` (e.g. restart the client session) — the loaded toolset
   state lives in the server process and survives a list refresh.

## Plugin doesn't appear in KiCAD

Install via **Plugin and Content Manager → Install from File** with the
`konnect-pcm-*.zip` release asset (not the bare binary archives), then restart
KiCAD.

Look in the **PCB editor**, under *Tools → External Plugins* — and open a
project first, because pcbnew refuses to open at all without one
(*« Créer (ou ouvrir) un projet pour modifier un pcb. »*). The package also
declares a toolbar action, which KiCad 10 does not render; the menu entry is the
one that works.

## Still stuck?

**[File a first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml)**
— six questions, about two minutes. A report from someone who gave up is worth
more than one from someone who succeeded: it is the only way a failure on a
machine that is not the maintainer's ever gets seen. What comes back is tallied
in [adoption.md](adoption.md).
