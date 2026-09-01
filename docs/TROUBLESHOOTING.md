# Troubleshooting

## "KiCAD IPC socket path not configured" / "IPC connect failed"

Any tool that talks to a live KiCAD session (`save_project`, PCB editing,
`check_kicad_ui`, …) needs the IPC socket address. Since v1.1.1, Konnect resolves
it automatically from an explicit setting, then `KICAD_API_SOCKET`, then KiCad's
platform-default address. A standard install needs no copied socket path and no
`konnect-settings.json`.

Check the runtime in this order:

1. Open KiCAD normally.
2. Go to **Edit → Preferences → Plugins** and check **"Enable KiCad API"**.
   Confirm a line like this appears:

   ```
   Listening on ipc://C:\Users\<you>\AppData\Local\Temp\kicad\api.sock
   ```

3. Confirm your AI client (Claude Code, Claude Desktop, …) has the `konnect`
   MCP server registered in its own config (`.mcp.json` or
   `claude_desktop_config.json`) pointing at the `konnect` binary — see
   [examples/](../examples/).
4. Restart the AI client session so it spawns a fresh Konnect process.
5. Verify: have the AI call `open_project`. Expected:

   ```json
   { "kicad_ui_running": true, "message": "KiCAD is running and IPC is available." }
   ```

For a non-standard socket, set **IPC Socket** in Konnect's plugin settings or
pass a `konnect-settings.json` with `ipc_address` (legacy
`ipc_socket_path` is also accepted). An explicit non-empty value is never
replaced by discovery.

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

## A schematic write is refused while KiCad has the file open

Konnect refuses to change a `.kicad_sch` while KiCad's sibling
`~<name>.kicad_sch.lck` exists, and answers with the `conflict` error kind
naming both the schematic and the lock. Close the schematic editor normally and
retry the identical call. Read-only schematic tools keep working throughout.

Why it is not smarter than that: KiCad's lock stores a username and a hostname
and nothing else — no process id, no start time, no document token. There is no
way to tell a live editor from one a crash left behind, and guessing wrong
costs whatever is unsaved in that editor. So Konnect treats valid,
foreign-host, empty, and malformed locks alike, and never removes one for you.

If KiCad crashed, the clean fix is to reopen the project and close it normally,
which makes KiCad release its own lock. Deleting `~<name>.kicad_sch.lck` by hand
is a last resort, and only after confirming no editor still owns the file.

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
