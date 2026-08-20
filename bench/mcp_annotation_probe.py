"""A stand-in MCP server that answers one question about a client.

Which MCP `annotations` does a client require before it will run a tool with no
human present? The tools below differ in nothing but their annotations, so a
run that calls several of them separates the cases the way no amount of reading
a CLI's `--help` can.

Measured against `codex-cli 0.147.0` on 2026-08-20, `codex exec --json
-s read-only`, three runs (K.1.8):

    echo_ro                 readOnly T  destructive F  idempotent T  openWorld F   ran
    echo_ro_min             readOnly T                                             ran
    echo_bare               (no annotations at all)                          cancelled
    echo_ow_only                                                     openWorld F   cancelled
    echo_write_soft         readOnly F  destructive F  idempotent T  openWorld F   ran
    echo_write_ow           readOnly F  destructive F                openWorld F   ran
    echo_write_min          readOnly F  destructive F                            cancelled
    echo_write_idem         readOnly F  destructive F  idempotent T             cancelled
    echo_write_destructive  readOnly F  destructive T  idempotent F  openWorld F   cancelled

So: a read needs `readOnlyHint: true` and nothing else; a write needs
`destructiveHint: false` **and** `openWorldHint: false` beside its
`readOnlyHint: false` — drop either and the call is cancelled; `idempotentHint`
never changes an outcome; and `destructiveHint: true` is refused as flatly as
no annotations at all. `openWorldHint` alone does not qualify a tool, so
`readOnlyHint` is the field the gate reads.

"cancelled" is codex's own `user cancelled MCP tool call` — an approval request
with no responder in non-interactive `exec`, not a refusal anyone chose. Ruled
out first, each by its own run: `approval_policy="never"`,
`mcp_servers.<name>.default_tools_approval_mode="auto"`, and project
`trust_level`.

Run it as a server, never directly — point a client at it:

    codex exec --json --skip-git-repo-check -s read-only \
      -c "mcp_servers.annotprobe.command='<python>'" \
      -c "mcp_servers.annotprobe.args=['<abs path to this file>']"

then ask the agent to call the tools you care about and report each result.
"""

import json, sys

TOOLS = [
    {
        "name": "echo_ro",
        "description": "Echo the text back. Annotated read-only.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"title": "Echo (read only)", "readOnlyHint": True, "destructiveHint": False,
                        "idempotentHint": True, "openWorldHint": False},
    },
    {
        "name": "echo_bare",
        "description": "Echo the text back. No annotations at all.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
    },
    {
        "name": "echo_write_soft",
        "description": "Echo the text back. Annotated as a write, non-destructive, closed world.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"title": "Echo (write)", "readOnlyHint": False, "destructiveHint": False,
                        "idempotentHint": True, "openWorldHint": False},
    },
    {
        "name": "echo_ro_min",
        "description": "Echo the text back. Annotated with readOnlyHint alone.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "echo_ow_only",
        "description": "Echo the text back. Annotated with openWorldHint alone.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"openWorldHint": False},
    },
    {
        "name": "echo_write_min",
        "description": "Echo the text back. Write, non-destructive, no openWorldHint.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"readOnlyHint": False, "destructiveHint": False},
    },
    {
        "name": "echo_write_ow",
        "description": "Echo the text back. Write, non-destructive, openWorldHint false, no idempotentHint.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"readOnlyHint": False, "destructiveHint": False, "openWorldHint": False},
    },
    {
        "name": "echo_write_idem",
        "description": "Echo the text back. Write, non-destructive, idempotentHint true, no openWorldHint.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True},
    },
    {
        "name": "echo_write_destructive",
        "description": "Echo the text back. Annotated as a destructive write.",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "annotations": {"title": "Echo (destructive)", "readOnlyHint": False, "destructiveHint": True,
                        "idempotentHint": False, "openWorldHint": False},
    },
]


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    mid, method = req.get("id"), req.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "annotprobe", "version": "0"}}})
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"tools": TOOLS}})
    elif method == "tools/call":
        name = req["params"]["name"]
        text = (req["params"].get("arguments") or {}).get("text", "")
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "content": [{"type": "text", "text": f"{name} ran: {text}"}], "isError": False}})
    elif mid is not None:
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
