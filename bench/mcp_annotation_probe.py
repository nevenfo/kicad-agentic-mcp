"""A four-tool stand-in MCP server that answers one question about a client.

Which MCP `annotations` does a client require before it will run a tool without
a human present? The four tools differ in nothing but their annotations, and a
single run calling all four separates the cases the way no amount of reading a
CLI's `--help` can.

Measured against `codex-cli 0.147.0` on 2026-08-20, one `codex exec --json
-s read-only` run, all four called in order (K.1.8):

    echo_ro                 readOnlyHint: true                        ran
    echo_bare               no annotations at all                     cancelled
    echo_write_soft         readOnlyHint false, destructiveHint false  ran
    echo_write_destructive  destructiveHint: true                     cancelled

"cancelled" is codex's own `user cancelled MCP tool call` — an approval request
with no responder in non-interactive `exec`, not a refusal anyone chose. Ruled
out first, each by its own run: `approval_policy="never"`,
`mcp_servers.<name>.default_tools_approval_mode="auto"`, and project
`trust_level`.

Run it as a server, never directly — point a client at it:

    codex exec --json --skip-git-repo-check -s read-only       -c "mcp_servers.annotprobe.command='<python>'"       -c "mcp_servers.annotprobe.args=['<abs path to this file>']"

then ask the agent to call all four tools and report each result.
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
