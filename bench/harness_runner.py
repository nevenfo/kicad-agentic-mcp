"""Golden-task benchmark, driven by a *real* agent instead of the oracle path.

`bench/runner.py` replays each task's scripted call sequence: it measures what
the server costs when the reasoning is free. This runner asks the same task in
plain language (`bench/agent_prompts.yaml`) to an agentic harness and scores the
result with the *same* rules — same fingerprint, same `audit()`, same
assertions, same thresholds. The two numbers are only comparable because none of
that scoring is re-implemented here: everything judgemental is imported from
`runner`.

The measurement only means something if the agent can reach the design *through
the server and nowhere else*. Three harnesses are supported, and they cannot all
be isolated the same way:

- `claude` runs with its built-in toolset genuinely emptied (`--tools ""`), so
  any call outside the MCP server is contamination, full stop. Its isolation
  level is `tools-off`. The model can still *emit* a call for a removed tool;
  the CLI refuses it and no result comes back. That refusal is the isolation
  working and is not counted — contamination is what reached the design, never
  what was attempted (`_is_refused`).
- `codex` and `agy` have no flag to remove their built-in tools (shell, patch,
  file read, web search) entirely — the closest available guard is a read-only
  sandbox, which stops those tools from *writing* the design but cannot stop
  them from being called. Their isolation level is `read-only-sandbox`.

Comparing `SUCCESS_RATE` (contamination-fatal) across harnesses at different
isolation levels would silently compare two different experiments, so every
report prints the isolation level of the harness it measured, and also prints
`DESIGN_PASS_RATE` — the fraction of runs whose design and assertions are
correct, ignoring `off_server_calls` and every path violation that is not a
safety one. Taking a route the task did not script is not a wrong design. `DESIGN_PASS_RATE` is the only
number that is safe to compare between harnesses; `SUCCESS_RATE` is only safe to
compare at equal isolation. `off_server_calls` is still printed for every
harness and is still a threshold, but it can only be a hard `FAIL` at
`tools-off` isolation: at `read-only-sandbox` it is a `SKIP`, because a nonzero
count there does not necessarily mean the design was reached off-server, only
that a built-in tool was *invoked* (e.g. to read a file back).

Usage:
    py -3.11 bench/harness_runner.py --server target/release/konnect.exe --dry-run
    py -3.11 bench/harness_runner.py --server target/release/konnect.exe \
        --harness claude --task sch_divider --repeat 1
    py -3.11 bench/harness_runner.py --server target/release/konnect.exe \
        --harness codex --dry-run
    py -3.11 bench/harness_runner.py --server target/release/konnect.exe \
        --harness agy --dry-run

`codex`'s JSONL event schema is confirmed against the K.1.1 campaign (14 real
transcripts, codex-cli 0.147): `parse_codex_jsonl` reads the `item.completed`
shape those runs emit, and keeps its defensive handling of the other
documented shapes — it must never turn an unrecognized transcript into a
silent 0/0 pass. `agy`'s schema is likewise confirmed against a real
transcript — see `parse_agy_stream` — but nothing in it unwraps the gateway,
since no agy run has ever reached the server.

`agy`'s MCP wiring is a separate, confirmed problem: see `AgyMcpConfigGuard`.
"""

from __future__ import annotations

import argparse
import atexit
import collections
import json
import os
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).parent))

import yaml  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402
from runner import (  # noqa: E402
    ASSERT_TOOLS,
    SAFETY_KINDS,
    THRESHOLDS,
    audit,
    check_assertion,
    fingerprint,
    install_fixture,
    load_tasks,
    substitute,
    unnecessary_call_count,
)

PROMPTS_FILE = Path(__file__).parent / "agent_prompts.yaml"

# The MCP server name the harness sees. Tool names arrive namespaced as
# `mcp__<server>__<tool>` (Claude Code, and presumably agy which shares its
# `.mcp.json` schema); anything not matching a recognized server prefix was
# reached outside the server and is counted as contamination.
SERVER_NAME = "konnect"
TOOL_PREFIX = f"mcp__{SERVER_NAME}__"

# The gateway's batch tool. An agent that finds it does the whole task through
# it, so what the audit sees depends entirely on whether the parser unwraps it
# — see `unwrap_gateway_batch` and `HarnessResult.audited_calls`.
GATEWAY_TOOL = "kicad_invoke"

# codex and agy cannot have their built-in tools removed the way claude's can;
# a read-only sandbox is the best available substitute. See module docstring.
HARNESS_ISOLATION = {
    "claude": "tools-off",
    "codex": "read-only-sandbox",
    "agy": "read-only-sandbox",
}


# ── harness invocation ───────────────────────────────────────────────────────


@dataclass
class HarnessResult:
    """What one agent invocation produced, before any scoring."""

    tool_calls: list[str] = field(default_factory=list)  # konnect round trips, in order
    # The path the audit judges: `tool_calls` with every `kicad_invoke` replaced
    # by the tools its batch actually ran. Kept apart from `tool_calls` because
    # they answer different questions — one round trip is one round trip
    # (`max_calls`), but a batch of five reads is five reads (`expected_tools`,
    # the `read_only` tier). A parser that cannot unwrap leaves `kicad_invoke`
    # in here, which `gateway_unwrap_warning` reports rather than hides.
    audited_calls: list[str] = field(default_factory=list)
    off_server_calls: list[str] = field(default_factory=list)
    cost_usd: float | None = 0.0  # None means "not reported", never fake-zero
    duration_ms: float = 0.0
    num_turns: int = 0
    usage: dict = field(default_factory=dict)
    result_subtype: str = ""
    error: str | None = None
    exposed_tools: list[str] = field(default_factory=list)
    text: str = ""


@dataclass
class HarnessContext:
    """Everything an `*_argv` builder needs to produce a command line + cwd."""

    prompt: str
    server: str
    config: str
    work: Path
    budget: float
    model: str | None
    timeout: float


def mcp_config_payload(server: str, config: str) -> dict:
    return {
        "mcpServers": {
            SERVER_NAME: {
                "type": "stdio",
                "command": str(Path(server).resolve()),
                "args": ["--config", str(Path(config).resolve())],
            }
        }
    }


# agy 1.1.13's only working MCP config path — see `AgyMcpConfigGuard`.
AGY_GLOBAL_MCP_CONFIG = Path.home() / ".gemini" / "config" / "mcp_config.json"


class AgyMcpConfigError(RuntimeError):
    """Raised for every refusal and every restore-verification failure.

    Deliberately a plain, loud exception: nothing in this class is allowed to
    degrade to a silent skip or a warning, because it exists specifically to
    protect a file this tool does not own.
    """


class AgyMcpConfigGuard:
    """Write-then-restore access to agy's *global*, per-user MCP config.

    Why this exists: agy 1.1.13 ignores workspace-local MCP config — both
    `.mcp.json` and the officially documented `.agents/mcp_config.json` were
    tried against a real run and both got "no MCP server is connected" back
    (known upstream bug, antigravity-cli#60). The only file agy actually reads
    is this global one. On this machine it exists and holds exactly one byte
    (`\\n`) — the user has declared no MCP server in it. Getting an agy run to
    reach `konnect` at all requires writing into that personal file for the
    run's duration and restoring it byte-for-byte afterward: "byte-for-byte"
    because a bare `\\n` (or any hand-edited file) is not something
    `json.dump` reproduces, so the original bytes — not a reformatted
    equivalent — are what gets written back.

    Safety contract (this is the user's personal config, held to a higher bar
    than ordinary bench code):
      - refuses to start if `konnect` is already declared (residue or a
        deliberate user entry — cannot tell which, so it does not guess);
      - refuses to start if a backup file from a previous run is still there
        (a previous run did not restore; a human needs to look at it);
      - preserves every other server already declared, merging `konnect` in
        rather than replacing the document;
      - keeps the original bytes in a timestamped backup file, written before
        any modification, deleted only after a *verified* restore;
      - restores on every exit path: normal, exception, `KeyboardInterrupt`
        (own `SIGINT` handler) and interpreter shutdown (`atexit`);
      - re-reads the file after restoring and compares it byte-for-byte to
        what was backed up; a mismatch is `AgyMcpConfigError`, not a log line.

    Scope: only ever constructed for `--harness agy`; `claude` and `codex`
    never touch this class or this file.
    """

    def __init__(self, path: Path, server: str, config: str):
        self.path = path
        self._server = server
        self._config = config
        self._existed = False
        self._original_bytes: bytes | None = None
        self._backup_path: Path | None = None
        self._restored = False
        self._prev_sigint = None

    def _entry(self) -> dict:
        return {
            "command": str(Path(self._server).resolve()),
            "args": ["--config", str(Path(self._config).resolve())],
            "disabled": False,
        }

    def _merged_document(self) -> tuple[dict, bool]:
        """`(document_to_write, konnect_already_present)`.

        A parse failure (e.g. today's literal `\\n`, which is not valid JSON)
        is treated as "no servers declared yet", never as an error: the
        original bytes are what gets restored, this parse is only used to
        decide what to write meanwhile.
        """
        base: dict = {}
        if self._existed and self._original_bytes:
            try:
                parsed = json.loads(self._original_bytes.decode("utf-8"))
                if isinstance(parsed, dict):
                    base = dict(parsed)
            except (json.JSONDecodeError, UnicodeDecodeError):
                base = {}
        servers = base.get("mcpServers")
        servers = dict(servers) if isinstance(servers, dict) else {}
        already_present = "konnect" in servers
        if not already_present:
            servers["konnect"] = self._entry()
        base["mcpServers"] = servers
        return base, already_present

    def __enter__(self) -> "AgyMcpConfigGuard":
        residual = sorted(self.path.parent.glob(f"{self.path.name}.kam-backup-*")) if self.path.parent.exists() else []
        if residual:
            raise AgyMcpConfigError(
                f"sauvegarde résiduelle trouvée : {residual[0]} — un run précédent "
                f"n'a pas restauré {self.path}. Restaure-le manuellement depuis ce "
                "fichier puis supprime la sauvegarde avant de relancer."
            )

        self._existed = self.path.exists()
        self._original_bytes = self.path.read_bytes() if self._existed else None

        document, already_present = self._merged_document()
        if already_present:
            raise AgyMcpConfigError(
                f"{self.path} déclare déjà un serveur MCP nommé 'konnect' — résidu "
                "d'un run précédent interrompu ou entrée volontaire de "
                "l'utilisateur, impossible de distinguer les deux : arrêt sans "
                "modification. Retire cette entrée manuellement si c'est un résidu."
            )

        ts = time.strftime("%Y%m%dT%H%M%S")
        suffix = "bak" if self._existed else "absent"
        self._backup_path = self.path.parent / f"{self.path.name}.kam-backup-{ts}.{suffix}"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._backup_path.write_bytes(self._original_bytes or b"")

        self.path.write_text(json.dumps(document, indent=2), encoding="utf-8")

        atexit.register(self._restore_best_effort)
        self._prev_sigint = signal.getsignal(signal.SIGINT)
        signal.signal(signal.SIGINT, self._on_sigint)
        return self

    def _on_sigint(self, signum, frame):
        self._restore_best_effort()
        if self._prev_sigint is not None:
            signal.signal(signal.SIGINT, self._prev_sigint)
        raise KeyboardInterrupt

    def restore(self) -> None:
        """Restore the original bytes and verify. Raises loudly on mismatch."""
        if self._restored:
            return
        if self._existed:
            self.path.write_bytes(self._original_bytes)
            current = self.path.read_bytes() if self.path.exists() else None
            if current != self._original_bytes:
                raise AgyMcpConfigError(
                    f"échec de restauration de {self.path} : le contenu relu ne "
                    f"correspond pas aux octets d'origine. Sauvegarde conservée : "
                    f"{self._backup_path} — restaure-la manuellement."
                )
        else:
            if self.path.exists():
                self.path.unlink()
            if self.path.exists():
                raise AgyMcpConfigError(
                    f"échec de restauration de {self.path} : le fichier existe "
                    f"encore alors qu'il n'existait pas avant ce run. Sauvegarde "
                    f"conservée : {self._backup_path}."
                )
        self._restored = True
        if self._backup_path and self._backup_path.exists():
            self._backup_path.unlink()

    def _restore_best_effort(self) -> None:
        # Reached from `atexit` (interpreter shutdown) or the SIGINT handler:
        # raising here would either be swallowed or print a confusing
        # traceback, so this path prints loudly to stderr instead and leaves
        # the backup file in place for a human, rather than raising.
        try:
            self.restore()
        except Exception as exc:  # noqa: BLE001 - last-resort safety net
            print(
                f"AVERTISSEMENT: restauration de {self.path} a échoué en sortie "
                f"d'urgence : {exc}. Sauvegarde disponible : {self._backup_path}",
                file=sys.stderr,
            )

    def __exit__(self, exc_type, exc, tb) -> bool:
        if self._prev_sigint is not None:
            signal.signal(signal.SIGINT, self._prev_sigint)
        atexit.unregister(self._restore_best_effort)
        self.restore()
        return False


def claude_argv(ctx: HarnessContext) -> tuple[list[str], Path, dict[str, str], str | None]:
    """The exact command line, kept in one place so `--dry-run` prints the truth.

    `--tools ""` empties the built-in set (no Bash/Write/Edit/Read), which is
    what makes the measurement honest; `--strict-mcp-config` guarantees no other
    MCP server is inherited from the user's config. `--allowedTools mcp__konnect`
    whitelists the whole server, and `bypassPermissions` keeps a headless run
    from blocking on a prompt it cannot answer.

    On Windows `claude` resolves (via `shutil.which`) to a `.CMD` shim, and
    `CreateProcess` re-parses `.CMD` argv through `cmd.exe`, which truncates any
    argument at its first newline. A multi-line prompt passed positionally to
    `-p` therefore arrives cut to its first line, and every flag after it is
    silently dropped along with it (verified: no `--mcp-config`, no `--tools
    ""`). The prompt is sent on stdin instead (`-p` with no positional value);
    everything else is unaffected because it is a single-line flag/value.
    """
    cfg_path = Path(tempfile.mkdtemp(prefix="kam-agent-cfg-")) / "mcp.json"
    cfg_path.write_text(
        json.dumps(mcp_config_payload(ctx.server, ctx.config), indent=2), encoding="utf-8"
    )
    argv = [
        "claude",
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--strict-mcp-config",
        "--mcp-config",
        str(cfg_path),
        "--tools",
        "",
        "--allowedTools",
        f"mcp__{SERVER_NAME}",
        "--permission-mode",
        "bypassPermissions",
        "--max-budget-usd",
        f"{ctx.budget}",
    ]
    if ctx.model:
        argv += ["--model", ctx.model]
    return argv, ctx.work, {"mcp_config_path": str(cfg_path)}, ctx.prompt


def _toml_literal_str(value: str) -> str:
    """A TOML *literal* string (single-quoted, no escape processing).

    `-c key=value` parses `value` as TOML. Windows paths are full of
    backslashes, and TOML *basic* (double-quoted) strings treat backslash as an
    escape introducer (`\\U`, `\\c`, ... are invalid escapes and fail to parse).
    Literal strings side-step that entirely; their only restriction — no single
    quote inside the value — is never hit by a filesystem path here.
    """
    return f"'{value}'"


CODEX_AUTH_FILE = "auth.json"


def codex_user_home() -> Path:
    """The real `CODEX_HOME` — the one holding the user's credentials."""
    env = os.environ.get("CODEX_HOME")
    return Path(env) if env else Path.home() / ".codex"


class CodexHomeError(RuntimeError):
    pass


class CodexHomeGuard:
    """A throwaway `CODEX_HOME` holding the credentials and nothing else.

    `codex exec --ignore-user-config` skips exactly one file, and its own
    `--help` says which: `$CODEX_HOME/config.toml` ("auth still uses
    `CODEX_HOME`"). Everything else a personal home carries still reaches the
    model — `AGENTS.md`, `skills/`, `plugins/`, the execpolicy `.rules`. The
    first real codex run proved it rather than suspected it: the transcript
    opens with "Skill descriptions were shortened to fit the skills context
    budget", and the agent's first three actions are `rtk proxy pwsh`,
    `rtk fd`, `rtk read` — a private toolchain this bench has never heard of,
    each one refused by the sandbox. A run carrying the operator's own
    instructions measures the operator, not Konnect.

    So the campaign gets a home of its own: a temp directory holding a copy of
    `auth.json` and nothing else. Auth survives (it is read from `CODEX_HOME`
    whatever else is absent), instructions and skills do not.

    It **copies** rather than symlinks, so a token codex refreshes lands in the
    throwaway copy and never rewrites the user's own file. The copy is
    credentials, so it is deleted on every exit path — normal, exception,
    `SIGINT`, interpreter shutdown — the same four `AgyMcpConfigGuard` covers.

    Scope: only ever constructed for `--harness codex`, and never for
    `--dry-run`, which spends nothing and touches nothing.
    """

    def __init__(self, source_home: Path):
        self.source = source_home
        self.home: Path | None = None
        self._prev_env: str | None = None
        self._prev_env_set = False
        self._cleaned = False
        self._prev_sigint = None

    def __enter__(self) -> CodexHomeGuard:
        auth = self.source / CODEX_AUTH_FILE
        if not auth.is_file():
            raise CodexHomeError(
                f"{auth} est introuvable : codex n'est pas authentifié dans "
                f"{self.source}, ou CODEX_HOME pointe ailleurs. Lance "
                "`codex login` avant de mesurer."
            )
        self.home = Path(tempfile.mkdtemp(prefix="kam-codex-home-"))
        shutil.copy2(auth, self.home / CODEX_AUTH_FILE)

        self._prev_env_set = "CODEX_HOME" in os.environ
        self._prev_env = os.environ.get("CODEX_HOME")
        # The child inherits this process's environment (`run_harness` passes
        # no `env=`), so setting it here is what reaches codex.
        os.environ["CODEX_HOME"] = str(self.home)

        atexit.register(self._clean_best_effort)
        self._prev_sigint = signal.getsignal(signal.SIGINT)
        signal.signal(signal.SIGINT, self._on_sigint)
        return self

    def _on_sigint(self, signum, frame):
        self._clean_best_effort()
        if self._prev_sigint is not None:
            signal.signal(signal.SIGINT, self._prev_sigint)
        raise KeyboardInterrupt

    def clean(self) -> None:
        """Restore `CODEX_HOME` and delete the copied credentials."""
        if self._cleaned:
            return
        if self._prev_env_set:
            os.environ["CODEX_HOME"] = self._prev_env or ""
        else:
            os.environ.pop("CODEX_HOME", None)
        if self.home is not None and self.home.exists():
            shutil.rmtree(self.home, ignore_errors=True)
            if (self.home / CODEX_AUTH_FILE).exists():
                raise CodexHomeError(
                    f"impossible de supprimer la copie des identifiants "
                    f"{self.home / CODEX_AUTH_FILE} — supprime-la manuellement."
                )
        self._cleaned = True

    def _clean_best_effort(self) -> None:
        # Reached from `atexit` or the SIGINT handler, where raising is either
        # swallowed or printed as a confusing traceback: report loudly instead.
        try:
            self.clean()
        except Exception as exc:  # noqa: BLE001 - last-resort safety net
            print(f"AVERTISSEMENT: nettoyage de {self.home} a échoué : {exc}", file=sys.stderr)

    def __exit__(self, exc_type, exc, tb) -> bool:
        if self._prev_sigint is not None:
            signal.signal(signal.SIGINT, self._prev_sigint)
        atexit.unregister(self._clean_best_effort)
        self.clean()
        return False


def codex_argv(ctx: HarnessContext) -> tuple[list[str], Path, dict[str, str], str | None]:
    """`codex exec`, mcp server passed by `-c` override (no `.mcp.json` file).

    `--json` prints one JSONL event per line on stdout. `-s read-only` is the
    sandbox that stands in for the tool removal claude gets via `--tools ""` —
    codex has no flag to remove exec/patch/read tools outright, so the
    guarantee here is weaker: those tools can still be *called*, just not used
    to write the design (see `HARNESS_ISOLATION`). `--ignore-user-config` keeps
    the run from inheriting the user's `config.toml` (including any MCP servers
    already configured there) and `--ignore-rules` its execpolicy `.rules`;
    API auth is unaffected, it is read from `CODEX_HOME` regardless. Those two
    flags are not isolation on their own — `AGENTS.md`, `skills/` and
    `plugins/` load from `CODEX_HOME` whatever they say, which is why
    `CodexHomeGuard` points `CODEX_HOME` at a home holding only the
    credentials. `codex exec` never prompts for interactive approval — that is
    the point of the non-interactive mode — so no separate approval-policy flag
    is needed; the sandbox is the only gate. There is no per-run budget flag on
    codex, unlike claude's `--max-budget-usd`; the guard here is
    `--harness-timeout` only.

    Same `.CMD`-shim newline truncation as claude (`codex` resolves to a `.CMD`
    on Windows too): the prompt is never put in argv. `codex exec --help`
    documents stdin as the fallback when no positional `[PROMPT]` is given, so
    it is sent that way instead.
    """
    server_abs = str(Path(ctx.server).resolve())
    config_abs = str(Path(ctx.config).resolve())
    mcp_command = f"mcp_servers.{SERVER_NAME}.command={_toml_literal_str(server_abs)}"
    mcp_args = (
        f"mcp_servers.{SERVER_NAME}.args="
        f"[{_toml_literal_str('--config')}, {_toml_literal_str(config_abs)}]"
    )
    argv = [
        "codex",
        "exec",
        "--json",
        "--skip-git-repo-check",
        "-s",
        "read-only",
        "--ignore-user-config",
        "--ignore-rules",
        "-c",
        mcp_command,
        "-c",
        mcp_args,
    ]
    if ctx.model:
        argv += ["-m", ctx.model]
    meta = {
        "mcp_config": "inline via -c overrides (no file written)",
        "codex_home": os.environ.get("CODEX_HOME") or f"{codex_user_home()} (user default)",
    }
    return argv, ctx.work, meta, ctx.prompt


def agy_argv(ctx: HarnessContext) -> tuple[list[str], Path, dict[str, str], str | None]:
    """`agy`'s command line. The MCP server is *not* wired here.

    Confirmed by real runs, not guessed: agy 1.1.13 ignores workspace-local MCP
    config entirely. Both `.mcp.json` (Claude-Code-style) and the officially
    *documented* `.agents/mcp_config.json` were tried in this cwd; both got
    "no MCP server is connected" back from a real run. This is a known
    upstream bug (antigravity-cli#60). The only file agy actually reads is a
    *global*, per-user config, `~/.gemini/config/mcp_config.json` — writing
    `konnect` in there for the run's duration, and restoring it byte-for-byte
    afterward, is what `AgyMcpConfigGuard` (see below) does instead; nothing
    is written into this function's cwd for that purpose anymore.

    The cwd is still a fresh temp directory, kept outside `$WORK` so the
    fingerprint that guards `read_only` tasks never mistakes harness debris
    for a mutation, even though it holds nothing today — keeping it (instead
    of falling back to `$WORK` or `$TEMP` directly) means agy's own working
    files (if any) don't land inside `$WORK` either, and the moment agy grows
    a working workspace-config path this cwd is exactly where it would need to
    live. `--add-dir <WORK>` is what gives the agent filesystem access to
    `$WORK` (whose absolute paths are already baked into the prompt by
    `substitute`). `--sandbox` is the closest agy equivalent of a read-only
    sandbox (its built-in tools cannot be removed, only restricted);
    `--dangerously-skip-permissions` avoids blocking on a permission prompt no
    one can answer headlessly, and needs no separate approval policy on top;
    `--print-timeout` is kept aligned with `--harness-timeout` so the two
    garbage-collect the run at the same time.

    Unlike `claude`/`codex`, the prompt is kept positional in argv here. Those
    two resolve (`shutil.which`) to `.CMD` shims on Windows, which
    `CreateProcess` re-parses through `cmd.exe`, truncating any argument at its
    first newline; `agy` resolves to a native `agy.exe`
    (`C:/Users/FlowUP/AppData/Local/agy/bin/agy.exe`), which `CreateProcess`
    launches directly with no `cmd.exe` re-parse, so a multi-line argv value is
    not truncated. `agy --help` gives no evidence `-p`/`--print` accepts stdin
    when no value is given (unlike codex's documented stdin fallback), so argv
    is the only path known to work for agy. If this distinction is ever wrong —
    e.g. agy's launcher changes to a shim — this comment is the reason argv was
    used here and not stdin; do not "fix" it back to argv for claude/codex.
    """
    cwd = Path(tempfile.mkdtemp(prefix="kam-agent-agy-cwd-"))
    argv = [
        "agy",
        "-p",
        ctx.prompt,
        "--output-format",
        "stream-json",
        "--dangerously-skip-permissions",
        "--disable-slash-commands",
        "--sandbox",
        "--add-dir",
        str(ctx.work),
        "--print-timeout",
        f"{int(ctx.timeout)}s",
    ]
    stdin_prompt = None  # prompt travels in argv for agy — see docstring above
    if ctx.model:
        argv += ["--model", ctx.model]
    # `cwd` is already reported by the caller, so the only thing worth adding
    # here is where agy's MCP wiring actually comes from — the one part of this
    # harness that is not visible anywhere in the command line.
    return argv, cwd, {"mcp_wiring": "global ~/.gemini config, see AgyMcpConfigGuard"}, stdin_prompt


def unwrap_gateway_batch(result_text: str) -> list[str] | None:
    """The tools one `kicad_invoke` batch ran, from the server's own reply.

    `bench/runner.py::executed_tools` states the rule this implements:
    `kicad_invoke` is a door, and auditing the door instead of what went
    through it marks every gateway run as a write — the `read_only` tier fails,
    and every `expected_tool` reads as never called. The names come from the
    reply's per-entry `tool` field, never from the request, so what is audited
    is the gateway's own answer about what it ran.

    `None` means "this is not a batch reply I can read", which the caller keeps
    as `kicad_invoke` rather than dropping: an unreadable reply must stay
    visible in the audited path.
    """
    try:
        payload = json.loads(result_text)
    except (json.JSONDecodeError, TypeError):
        return None
    results = payload.get("results") if isinstance(payload, dict) else None
    if not isinstance(results, list):
        return None
    return [str(r.get("tool")) for r in results if isinstance(r, dict) and r.get("tool")]


# A harness that removes its own built-ins still lets the model *emit* a call
# for one; the CLI refuses it and the model never gets a result. That refusal
# is the isolation working, not contamination, so it must not be counted as an
# off-server call — measured on a real `tools-off` run where the model tried
# `Read` and got "No such tool available: Read. Read is disabled for this
# session". Matched on the refusal text because `is_error` alone also covers a
# tool that ran and failed, which *is* contamination.
_REFUSAL_MARKERS = ("No such tool available", "is disabled for this session")


def _is_refused(is_error: bool, text: str) -> bool:
    return is_error and any(m in text for m in _REFUSAL_MARKERS)


def _result_text(block: dict) -> str:
    content = block.get("content")
    if isinstance(content, list):
        return "".join(b.get("text", "") for b in content if isinstance(b, dict))
    return content if isinstance(content, str) else ""


def _walk_tool_uses(msg: dict) -> list[tuple[str, str]]:
    """Every `tool_use` in one assistant message, as `(id, name)`."""
    content = (msg.get("message") or {}).get("content")
    if not isinstance(content, list):
        return []
    return [
        (b.get("id", ""), b.get("name", "?"))
        for b in content
        if isinstance(b, dict) and b.get("type") == "tool_use"
    ]


def _walk_text(msg: dict) -> str:
    content = (msg.get("message") or {}).get("content")
    if not isinstance(content, list):
        return ""
    return "".join(b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text")


def parse_stream(lines: list[str], tool_prefixes: tuple[str, ...]) -> HarnessResult:
    """Read a Claude-Code-shaped stream-json transcript.

    Used by `claude` only — a real agy transcript turned out to use an
    unrelated schema (root key `event`, not `type`; see `parse_agy_stream`),
    so the "shared with agy" guess this function used to document was wrong
    and has been retired rather than left misleading. `tool_prefixes` is kept
    as a parameter (not hardcoded) in case a future stdio-MCP harness reuses
    Claude Code's transcript shape with a different namespace prefix; the
    first matching prefix is stripped and the call is scored as on-server.
    Every field taken from the `result` message is read defensively: the
    schema is the harness's, not ours, and a missing key must degrade the
    metric rather than crash the suite.

    Two passes, because a `tool_use` is scored by what came *back*: its result
    arrives in a later `user` message, and both `unwrap_gateway_batch` and
    `_is_refused` need it. The first pass indexes results by `tool_use_id`; the
    second is the one that scores.
    """
    out = HarnessResult()
    saw_result = False
    json_lines = 0
    nonempty_lines = 0
    parsed: list[dict] = []
    results_by_id: dict[str, tuple[bool, str]] = {}
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        nonempty_lines += 1
        if not raw.startswith("{"):
            continue
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError:
            continue
        json_lines += 1
        parsed.append(msg)
        content = (msg.get("message") or {}).get("content")
        if msg.get("type") == "user" and isinstance(content, list):
            for b in content:
                if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id"):
                    results_by_id[b["tool_use_id"]] = (bool(b.get("is_error")), _result_text(b))

    for msg in parsed:
        kind = msg.get("type")
        if kind == "system" and msg.get("subtype") == "init":
            out.exposed_tools = list(msg.get("tools") or []) + list(msg.get("mcp_tools") or [])
        elif kind == "assistant":
            for use_id, name in _walk_tool_uses(msg):
                prefix = next((p for p in tool_prefixes if name.startswith(p)), None)
                is_error, text = results_by_id.get(use_id, (False, ""))
                if prefix is None:
                    if not _is_refused(is_error, text):
                        out.off_server_calls.append(name)
                    continue
                short = name[len(prefix):]
                out.tool_calls.append(short)
                if short == GATEWAY_TOOL:
                    inner = unwrap_gateway_batch(text)
                    out.audited_calls.extend(inner if inner is not None else [short])
                else:
                    out.audited_calls.append(short)
            out.text += _walk_text(msg)
        elif kind == "result":
            saw_result = True
            out.result_subtype = msg.get("subtype", "")
            cost = msg.get("total_cost_usd")
            out.cost_usd = float(cost) if isinstance(cost, (int, float)) else None
            out.duration_ms = float(msg.get("duration_ms") or 0.0)
            out.num_turns = int(msg.get("num_turns") or 0)
            usage = msg.get("usage")
            if isinstance(usage, dict):
                out.usage = {
                    k: v for k, v in usage.items() if isinstance(v, (int, float))
                }
            if msg.get("is_error") or out.result_subtype not in ("success", ""):
                out.error = f"result subtype={out.result_subtype or '?'}"
            if isinstance(msg.get("result"), str) and not out.text:
                out.text = msg["result"]
    if not saw_result:
        # claude always emits a `result` message; agy's is unverified. Missing
        # cost/turns is expected for agy (see docstring) but a transcript with
        # no recognized event at all must not be scored as a silent success.
        out.cost_usd = None
    if nonempty_lines and not json_lines:
        # Not a single line was JSON: the harness did not honor
        # `--output-format stream-json` at all (e.g. it printed markdown/text
        # instead). Distinct from "transcript non reconnu" below, which
        # assumes valid JSON lines whose *content* just wasn't recognized.
        out.error = (
            "sortie non-JSON : le harness n'a pas honoré --output-format "
            f"stream-json ({nonempty_lines} lignes non-JSON)"
        )
    elif not out.tool_calls and not out.off_server_calls and lines:
        out.error = out.error or (
            f"transcript non reconnu : {len(lines)} lignes, aucun événement d'appel d'outil"
        )
    return out


def _codex_event(evt: dict) -> tuple[str, dict]:
    """Normalize codex's two possible JSONL shapes to `(event_type, payload)`.

    Enveloped: `{"id": "...", "msg": {"type": "...", ...}}`.
    Flat:      `{"type": "item.completed", "item": {...}}` /
               `{"type": "turn.completed", "usage": {...}}`.
    """
    if isinstance(evt.get("msg"), dict):
        msg = evt["msg"]
        return str(msg.get("type") or ""), msg
    return str(evt.get("type") or ""), evt


_CODEX_BUILTIN_TYPES = {
    "exec_command_begin",
    "exec_command_end",
    "patch_apply_begin",
    "apply_patch_begin",
    "apply_patch_approval_request",
    "web_search_begin",
    "file_read",
}
_CODEX_BUILTIN_ITEM_TYPES = {"command_execution", "file_change", "patch", "web_search"}


def _codex_result_text(src: dict) -> str:
    """The reply body of one codex `mcp_tool_call` item, as text.

    Confirmed against the first real codex campaign (K.1.1, 2026-08-20): a
    completed item carries `result: {"content": [{"type": "text", "text":
    "..."}], "structured_content": null}` — the same `content` shape
    `_result_text` already reads for Claude Code, which is why it is reused
    rather than re-implemented. `structured_content` is the documented
    alternative and has never been observed populated here; it is read as a
    fallback instead of being assumed absent.

    A failed or still-running item carries `result: null`, which yields `""`;
    `unwrap_gateway_batch` then returns `None` and the caller keeps the literal
    `kicad_invoke` in the audited path. An unreadable reply must stay visible,
    never become an empty batch.
    """
    result = src.get("result")
    if not isinstance(result, dict):
        return ""
    text = _result_text(result)
    if text:
        return text
    structured = result.get("structured_content")
    return json.dumps(structured) if isinstance(structured, dict) else ""


def parse_codex_jsonl(lines: list[str]) -> HarnessResult:
    """Read `codex exec --json` output.

    Verified against the K.1.1 campaign (14 real transcripts, codex-cli 0.147,
    2026-08-20): the live schema is `item.completed` carrying an item whose
    `type` is `mcp_tool_call` (`server`, `tool`, `arguments`, `result`) or
    `command_execution`. The older `mcp_tool_call_begin` shape is kept because
    removing an accepted shape costs nothing and buys no accuracy.

    Both plausible event shapes (see `_codex_event`) are handled; an event type
    this parser does not recognize is silently ignored rather than crashing the
    run, but if *nothing at all* is recognized while the transcript is
    non-empty, that is reported as `error`, never as a 0/0 pass — see module
    docstring. codex reports no per-run `cost_usd` in either known schema, so
    `cost_usd` stays `None` ("not reported"), not a fake `0.0`.
    """
    out = HarnessResult(cost_usd=None)
    recognized = 0
    json_lines = 0
    nonempty_lines = 0
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        nonempty_lines += 1
        if not raw.startswith("{"):
            continue
        try:
            evt = json.loads(raw)
        except json.JSONDecodeError:
            continue
        json_lines += 1
        kind, payload = _codex_event(evt)
        if not kind:
            continue

        item = payload.get("item") if kind == "item.completed" and isinstance(payload.get("item"), dict) else None

        if kind in ("mcp_tool_call_begin", "mcp_tool_call") or (item and item.get("type") == "mcp_tool_call"):
            src = item if item is not None else payload
            invocation = src.get("invocation") if isinstance(src.get("invocation"), dict) else src
            server = invocation.get("server") or src.get("server") or ""
            tool = str(invocation.get("tool") or src.get("tool") or src.get("name") or "?")
            if server and server != SERVER_NAME:
                out.off_server_calls.append(f"{server}.{tool}")
            else:
                out.tool_calls.append(tool)
                if tool == GATEWAY_TOOL:
                    inner = unwrap_gateway_batch(_codex_result_text(src))
                    out.audited_calls.extend(inner if inner is not None else [tool])
                else:
                    out.audited_calls.append(tool)
            recognized += 1
        elif kind in _CODEX_BUILTIN_TYPES or (item and item.get("type") in _CODEX_BUILTIN_ITEM_TYPES):
            out.off_server_calls.append(kind if item is None else item.get("type", kind))
            recognized += 1
        elif kind in ("turn.completed", "task_complete"):
            usage = payload.get("usage")
            if isinstance(usage, dict):
                out.usage = {k: v for k, v in usage.items() if isinstance(v, (int, float))}
            cost = payload.get("cost_usd") if isinstance(payload.get("cost_usd"), (int, float)) else None
            if cost is not None:
                out.cost_usd = float(cost)
            recognized += 1
        elif kind == "token_count":
            info = payload.get("info") if isinstance(payload.get("info"), dict) else {}
            usage = info.get("total_token_usage") or info.get("last_token_usage")
            if isinstance(usage, dict):
                out.usage = {k: v for k, v in usage.items() if isinstance(v, (int, float))}
            recognized += 1
        elif kind in ("error", "turn.failed", "stream_error"):
            err = payload.get("error") if isinstance(payload.get("error"), dict) else {}
            detail = payload.get("message") or err.get("message") or json.dumps(payload)[:200]
            out.error = f"codex event {kind}: {detail}"
            recognized += 1

    out.num_turns = recognized
    if nonempty_lines and not json_lines:
        out.error = (
            "sortie non-JSON : le harness n'a pas honoré --json "
            f"({nonempty_lines} lignes non-JSON)"
        )
    elif not out.tool_calls and not out.off_server_calls and lines:
        out.error = out.error or (
            f"transcript non reconnu : {len(lines)} lignes, aucun événement d'appel d'outil"
        )
    return out


_AGY_MCP_TOOL_NAME_KEYS = ("tool_name", "tool", "name", "mcp_tool", "toolName")
_AGY_MCP_SERVER_NAME_KEYS = ("server_name", "server", "mcp_server", "serverName")


def parse_agy_stream(lines: list[str]) -> HarnessResult:
    """Read agy's `stream-json` transcript — a schema unrelated to Claude Code's.

    Confirmed against a real 25-line transcript, not guessed:
    `bench/tasks/sch_inspection`, `agy`, `stream-json`. The root key is
    `event` (`init` | `step_update` | `result`), never `type` — nothing in
    `parse_stream` applies here, hence a dedicated function.

    `step_update` with `step_type == "tool"` carries the tool calls, and each
    call is emitted *twice*: once `state: "ACTIVE"`, once `state: "DONE"`, both
    sharing the same `step_index`. Calls are deduplicated on `step_index`, not
    on tool name — a tool called twice at different steps must still count
    twice, and the observed transcript confirms it (`find_by_name`,
    `view_file`, `view_file` — three distinct on-server-or-not calls).

    agy has no `mcp__<server>__<tool>` namespacing at all: MCP calls (if any
    were ever made) go through one generic built-in, `tool_name ==
    "call_mcp_tool"`, whose target server/tool live somewhere in
    `tool_info.parameters`. No real `call_mcp_tool` invocation was available
    when this was written, so the parameter keys searched (see
    `_AGY_MCP_*_NAME_KEYS`) are a best guess, applied defensively: an absent or
    unrecognized key never crashes the parser, it just falls back to the
    literal name `call_mcp_tool`. Every *other* `tool_name` (`find_by_name`,
    `view_file`, ...) is off-server by construction — agy's built-ins cannot be
    removed, only sandboxed (`HARNESS_ISOLATION["agy"] == "read-only-sandbox"`)
    — which is exactly what the observed run showed: the task was solved
    entirely off-server, `off_server_calls == 3`, Konnect was never called.

    `num_turns` counts distinct `agent_response` `step_index`es (i.e. agent
    turns), not lines — the metric otherwise has no stable meaning across
    harnesses. There is no cost field anywhere in this schema: `cost_usd`
    stays `None`. `usage` is summed across every `agent_response` step (there
    is one `usage` dict per step, not one for the whole run); on the observed
    transcript this reproduces `result.usage.total_tokens` exactly (68249),
    which is a useful cross-check but not relied upon — `result.usage` is not
    read, in case a future transcript omits it.
    """
    out = HarnessResult(cost_usd=None)
    seen_tool_steps: set[int] = set()
    seen_turn_steps: set[int] = set()
    usage_totals: dict[str, float] = {}
    nonempty_lines = 0
    json_lines = 0
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        nonempty_lines += 1
        if not raw.startswith("{"):
            continue
        try:
            evt = json.loads(raw)
        except json.JSONDecodeError:
            continue
        json_lines += 1
        kind = evt.get("event")

        if kind == "init":
            init = evt.get("init") if isinstance(evt.get("init"), dict) else {}
            out.exposed_tools = list(init.get("tools") or [])
        elif kind == "step_update":
            step = evt.get("step_update") if isinstance(evt.get("step_update"), dict) else {}
            step_type = step.get("step_type")
            idx = step.get("step_index")
            if step_type == "tool" and isinstance(idx, int) and idx not in seen_tool_steps:
                seen_tool_steps.add(idx)
                name = step.get("tool_name") or "?"
                if name == "call_mcp_tool":
                    info = step.get("tool_info") if isinstance(step.get("tool_info"), dict) else {}
                    params = info.get("parameters") if isinstance(info.get("parameters"), dict) else {}
                    tool = next(
                        (params[k] for k in _AGY_MCP_TOOL_NAME_KEYS if isinstance(params.get(k), str) and params[k]),
                        None,
                    )
                    server = next(
                        (params[k] for k in _AGY_MCP_SERVER_NAME_KEYS if isinstance(params.get(k), str) and params[k]),
                        None,
                    )
                    if server and server != SERVER_NAME:
                        out.off_server_calls.append(f"{server}.{tool or 'call_mcp_tool'}")
                    else:
                        out.tool_calls.append(tool or "call_mcp_tool")
                else:
                    out.off_server_calls.append(name)
            elif step_type == "agent_response":
                if isinstance(idx, int):
                    seen_turn_steps.add(idx)
                delta = step.get("text_delta")
                if isinstance(delta, str):
                    out.text += delta
                usage = step.get("usage")
                if isinstance(usage, dict):
                    for k, v in usage.items():
                        if isinstance(v, (int, float)):
                            usage_totals[k] = usage_totals.get(k, 0) + v
        elif kind == "result":
            result = evt.get("result") if isinstance(evt.get("result"), dict) else {}
            status = result.get("status")
            if status and status != "SUCCESS":
                out.error = f"agy result status={status}"
            if isinstance(result.get("response"), str) and not out.text:
                out.text = result["response"]

    out.usage = usage_totals
    out.num_turns = len(seen_turn_steps)
    if nonempty_lines and not json_lines:
        out.error = (
            "sortie non-JSON : le harness n'a pas honoré --output-format "
            f"stream-json ({nonempty_lines} lignes non-JSON)"
        )
    elif not out.tool_calls and not out.off_server_calls and lines:
        out.error = out.error or (
            f"transcript non reconnu : {len(lines)} lignes, aucun événement d'appel d'outil"
        )
    return out


def run_harness(
    argv: list[str],
    cwd: Path,
    timeout: float,
    log: Path | None,
    parser: Callable[[list[str]], HarnessResult],
    stdin_prompt: str | None,
) -> HarnessResult:
    """Run the agent. A budget overrun or a timeout is a failed run, not a crash.

    `argv[0]` is resolved through `PATH` first: on Windows `claude` and `codex`
    resolve to `.CMD` shims, and `CreateProcess` cannot find them by bare name.
    `.CMD` shims are also *why* `stdin_prompt` exists: `CreateProcess`
    re-parses a `.CMD`'s argv through `cmd.exe`, which truncates any argument
    at its first newline, silently dropping a multi-line prompt (and every
    flag placed after it in argv). When `stdin_prompt` is not `None` it is fed
    via `input=`, entirely bypassing argv for the prompt; `agy` resolves to a
    native `.exe` and keeps its prompt in argv instead (see `agy_argv`).
    """
    argv = [shutil.which(argv[0]) or argv[0], *argv[1:]]
    t0 = time.perf_counter()
    try:
        proc = subprocess.run(
            argv,
            cwd=str(cwd),
            input=stdin_prompt,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            shell=False,
        )
        stdout, stderr, code = proc.stdout, proc.stderr, proc.returncode
        timed_out = False
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else (exc.stdout or b"").decode("utf-8", "replace")
        stderr = exc.stderr if isinstance(exc.stderr, str) else (exc.stderr or b"").decode("utf-8", "replace")
        code, timed_out = -1, True
    wall = (time.perf_counter() - t0) * 1000.0

    if log:
        log.parent.mkdir(parents=True, exist_ok=True)
        log.write_text(stdout, encoding="utf-8")

    res = parser(stdout.splitlines())
    if not res.duration_ms:
        res.duration_ms = wall
    if timed_out:
        res.error = f"harness timeout after {timeout}s"
    elif code != 0 and not res.error:
        res.error = f"harness exit {code}: {stderr.strip()[:300] or stdout.strip()[-300:]}"
    return res


HARNESSES: dict[str, dict[str, Any]] = {
    "claude": {
        "argv": claude_argv,
        "isolation": HARNESS_ISOLATION["claude"],
        "parser": lambda lines: parse_stream(lines, (TOOL_PREFIX,)),
    },
    "codex": {
        "argv": codex_argv,
        "isolation": HARNESS_ISOLATION["codex"],
        "parser": parse_codex_jsonl,
    },
    "agy": {
        "argv": agy_argv,
        "isolation": HARNESS_ISOLATION["agy"],
        "parser": parse_agy_stream,
    },
}


# ── scoring ──────────────────────────────────────────────────────────────────


@dataclass
class HarnessRun:
    task_id: str
    harness: str
    success: bool
    design_success: bool  # success ignoring off_server_calls
    tools_used: list[str] = field(default_factory=list)  # deduplicated, call order
    tool_call_sequence: list[str] = field(default_factory=list)
    tool_calls: int = 0  # every agent tool call, konnect or not
    off_server_calls: int = 0
    off_server_names: list[str] = field(default_factory=list)
    cost_usd: float | None = 0.0
    duration_ms: float = 0.0
    num_turns: int = 0
    usage: dict = field(default_factory=dict)
    harness_error: str | None = None
    assertions: list[dict] = field(default_factory=list)
    violations: list[dict] = field(default_factory=list)
    safety_violations: int = 0
    unnecessary_calls: int = 0
    scored_calls: int = 0
    exposed_tools: list[str] = field(default_factory=list)
    unwrap_warning: str | None = None
    work: str = ""


def gateway_unwrap_warning(used_calls: list[str]) -> str | None:
    """Say so when the audited path still names the door.

    `parse_stream` and `parse_codex_jsonl` both unwrap, each against a real
    transcript of its own harness — `parse_agy_stream` does not, and no run has
    ever exercised it. The warning is not made obsolete by that: a batch whose
    reply never arrived (a failed call, a truncated transcript) is unwrappable
    by construction, whatever the parser. This is the honest alternative to
    guessing: if a gateway call survives into the audited path, the numbers
    derived from it are about `kicad_invoke` and not about what it ran, and the
    report says which.
    """
    n = sum(1 for name in used_calls if name == GATEWAY_TOOL)
    if not n:
        return None
    return (
        f"{n} {GATEWAY_TOOL} call(s) not unwrapped: the audit is judging the door, "
        "so safety/expected_tools verdicts on this run are unreliable"
    )


def assert_tool_names(task: dict) -> list[str]:
    """Only what the *assertions* need — never the step tools.

    The oracle's own reads are not part of the agent's path, and loading the
    step tools here would let an assertion pass on a toolbelt the agent never
    had.
    """
    names: list[str] = []
    for spec in task.get("assert", []):
        for name in ASSERT_TOOLS.get(spec["kind"], []):
            if name not in names:
                names.append(name)
    return names


def check_assertions(task: dict, server: str, config: str, env: dict[str, str]) -> list[dict]:
    specs = task.get("assert", [])
    if not specs:
        return []
    out: list[dict] = []
    proc_env = dict(os.environ)
    proc_env.setdefault("RUST_LOG", "warn")
    with McpStdioClient([server, "--config", config], env=proc_env) as client:
        client.initialize()
        needed = assert_tool_names(task)
        if needed:
            client.tools_call("load_tools", {"names": needed})
        for spec in specs:
            if spec["kind"] == "all_steps_ok":
                # There are no steps in an agentic run: the agent chose its own
                # path. Scoring this as a pass would hand every run a free
                # assertion, so it is recorded as *not applicable* (`ok: null`)
                # and excluded from the verdict on both sides.
                out.append(
                    {
                        "kind": "all_steps_ok",
                        "ok": None,
                        "detail": "not applicable: agentic run has no scripted steps",
                    }
                )
                continue
            out.append(asdict(check_assertion(spec, client, env, [])))
    return out


def load_prompts() -> dict[str, str]:
    return yaml.safe_load(PROMPTS_FILE.read_text(encoding="utf-8")) or {}


def prepare(task: dict) -> tuple[Path, dict[str, str]]:
    work = Path(tempfile.mkdtemp(prefix=f"kam-agent-{task['id']}-"))
    name = task.get("project_name", task["id"])
    posix_work = str(work).replace("\\", "/")
    env_vars = {
        "WORK": posix_work,
        "NAME": name,
        "SCH": f"{posix_work}/{name}.kicad_sch",
        "PCB": f"{posix_work}/{name}.kicad_pcb",
    }
    install_fixture(task, work, name)
    return work, env_vars


def run_task(
    task: dict,
    prompts: dict[str, str],
    args: argparse.Namespace,
    index: int,
) -> HarnessRun | None:
    prompt_tpl = prompts.get(task["id"])
    if prompt_tpl is None:
        # Hard error: a silently skipped task would quietly shrink the suite.
        raise SystemExit(f"{PROMPTS_FILE.name} has no prompt for task '{task['id']}'")

    work, env_vars = prepare(task)
    prompt = substitute(prompt_tpl, env_vars).strip()

    spec = HARNESSES[args.harness]
    ctx = HarnessContext(
        prompt=prompt,
        server=args.server,
        config=args.config,
        work=work,
        budget=args.max_budget_usd,
        model=args.model,
        timeout=args.harness_timeout,
    )
    argv, cwd, meta, stdin_prompt = spec["argv"](ctx)

    if args.dry_run:
        print(f"\n=== {task['id']}  (dry run) ===")
        print(f"WORK      {work}")
        print(f"files     {sorted(p.name for p in work.iterdir())}")
        print(f"env       SCH={env_vars['SCH']}")
        print(f"isolation {spec['isolation']}")
        print(f"cwd       {cwd}")
        for k, v in meta.items():
            # Explicit space, not padding alone: a key exactly as wide as the
            # column would otherwise run into its own value.
            print(f"{k:<9} {v}")
        print("cmd       " + " ".join(f'"{a}"' if " " in a or a == "" else a for a in argv))
        if stdin_prompt is not None:
            # Never in argv: printed separately so a reader cannot mistake it
            # for something passed on the command line (see run_harness).
            print("prompt    (stdin, not argv)")
            print("stdin     " + prompt.replace("\n", "\n          "))
        else:
            print("prompt    (argv, positional)")
        return None

    # Taken before the agent starts and again the moment it stops — the only
    # check that does not take the capability registry's word for anything.
    fp_before = fingerprint(work)
    log = Path(args.log_dir) / f"{task['id']}-{index}.jsonl" if args.log_dir else None
    res = run_harness(argv, cwd, args.harness_timeout, log, spec["parser"], stdin_prompt)
    fp_after = fingerprint(work)

    # The MCP config file is ours, not the agent's, and (for claude/agy) lives
    # outside `$WORK` entirely: it cannot register as a mutation.
    assertions = check_assertions(task, args.server, args.config, env_vars)

    # The audit judges what went through the gateway; `max_calls` counts the
    # round trips that carried it. A parser that fills neither falls back to
    # the round trips, which is what every parser did before unwrapping existed.
    used_calls = res.audited_calls or res.tool_calls
    scored_calls = len(res.tool_calls) + len(res.off_server_calls)
    violations = audit(task, used_calls, scored_calls, fp_before, fp_after)
    unwrap_warning = gateway_unwrap_warning(used_calls)

    assert_failed = any(a["ok"] is False for a in assertions)
    # Two different questions, and conflating them is what made the first codex
    # campaign read `0/14` while ten of those fourteen runs built a correct
    # design. `design_success` asks what its name says: did the design come out
    # right, and did the run stay inside what it must never do (`SAFETY_KINDS` —
    # a forbidden tool, a `read_only` write, a mutated `$WORK`). A route the
    # task's step list did not script — `add_schematic_component` where the
    # script batches, one round trip over `max_calls`, a read outside
    # `allowed_tools` — is a fact about *how* the agent got there, and it stays
    # in `SUCCESS_RATE`, which is strict about everything. An oracle run cannot
    # tell the two apart (it replays the script, so it always calls exactly the
    # expected tools); an agent run is the only place the difference exists.
    blocking = [v for v in violations if v.kind in SAFETY_KINDS]
    design_success = not assert_failed and not blocking and not res.error
    success = design_success and not violations and not res.off_server_calls

    return HarnessRun(
        task_id=task["id"],
        harness=args.harness,
        success=success,
        design_success=design_success,
        tools_used=list(dict.fromkeys(used_calls)),
        tool_call_sequence=used_calls,
        tool_calls=scored_calls,
        off_server_calls=len(res.off_server_calls),
        off_server_names=sorted(set(res.off_server_calls)),
        cost_usd=res.cost_usd,
        duration_ms=res.duration_ms,
        num_turns=res.num_turns,
        usage=res.usage,
        harness_error=res.error,
        assertions=assertions,
        violations=[asdict(v) for v in violations],
        safety_violations=sum(1 for v in violations if v.kind in SAFETY_KINDS),
        unnecessary_calls=unnecessary_call_count(task, used_calls),
        scored_calls=scored_calls,
        exposed_tools=res.exposed_tools,
        unwrap_warning=unwrap_warning,
        work=str(work),
    )


def instability(by_task: dict[str, list[HarnessRun]]) -> tuple[float | None, dict[str, float]]:
    """Same signature as `runner.instability`: `(success, tuple(tools_used))`.

    Kept here rather than imported because it is typed against `TaskRun`; the
    definition, which is what has to match, is identical.
    """
    per_task: dict[str, float] = {}
    for task_id, rs in by_task.items():
        if len(rs) < 2:
            continue
        sigs = collections.Counter((r.success, tuple(r.tools_used)) for r in rs)
        per_task[task_id] = 1.0 - sigs.most_common(1)[0][1] / len(rs)
    if not per_task:
        return None, {}
    return statistics.mean(per_task.values()), per_task


def _fmt_cost(costs: list[float | None]) -> str:
    known = [c for c in costs if c is not None]
    if not known:
        return "n/a"
    suffix = "" if len(known) == len(costs) else " (partial: some runs n/a)"
    return f"{sum(known):.4f}{suffix}"


def report(runs: list[HarnessRun], args: argparse.Namespace) -> int:
    by_task: dict[str, list[HarnessRun]] = {}
    for r in runs:
        by_task.setdefault(r.task_id, []).append(r)

    isolation = HARNESSES[args.harness]["isolation"]
    print(f"\nharness: {args.harness}   isolation: {isolation}   tasks: {len(by_task)}   runs: {len(runs)}\n")
    header = (
        f"{'task':<24} {'ok':>6} {'calls':>6} {'off':>4} {'turns':>6} "
        f"{'p50 ms':>8} {'usd':>9}"
    )
    print(header)
    print("-" * len(header))
    for task_id, rs in by_task.items():
        ok = sum(1 for r in rs if r.success)
        print(
            f"{task_id:<24} {ok}/{len(rs):>4} {statistics.median(r.tool_calls for r in rs):>6.0f} "
            f"{sum(r.off_server_calls for r in rs):>4} "
            f"{statistics.median(r.num_turns for r in rs):>6.0f} "
            f"{statistics.median(r.duration_ms for r in rs):>8.0f} "
            f"{_fmt_cost([r.cost_usd for r in rs]):>9}"
        )

    total_ok = sum(1 for r in runs if r.success)
    pass_rate = total_ok / len(runs)
    design_ok = sum(1 for r in runs if r.design_success)
    design_rate = design_ok / len(runs)
    safety_total = sum(r.safety_violations for r in runs)
    scored_total = sum(r.scored_calls for r in runs)
    unnecessary_total = sum(r.unnecessary_calls for r in runs)
    unnecessary_rate = unnecessary_total / scored_total if scored_total else 0.0
    instability_rate, per_task_instability = instability(by_task)
    off_total = sum(r.off_server_calls for r in runs)
    # K.2.6: a run that never reached konnect measured the harness, not the
    # server, and the two rates above cannot say so on their own — an agent
    # that answers correctly with its own shell looks, to `DESIGN_PASS_RATE`,
    # exactly like one that failed. Only reachable above `tools-off`
    # isolation, where the harness keeps tools we cannot remove; on an
    # *inspection* task reading the file directly is simply the shorter path.
    server_unused = sum(1 for r in runs if not r.tool_call_sequence)
    # The only population that says anything about Konnect. A run that never
    # reached the server is evidence about the client's willingness to use it,
    # never about whether it works: it can "pass" (codex reads an inspection
    # task's file with its own shell) or fail (the same shell is `-s read-only`,
    # so an authoring task writes nothing), and neither outcome touched the
    # thing under test.
    reached = [r for r in runs if r.tool_call_sequence]
    reached_ok = sum(1 for r in reached if r.design_success)
    reached_rate = reached_ok / len(reached) if reached else 0.0

    print(f"\nSUCCESS_RATE              {total_ok}/{len(runs)} = {pass_rate:.1%}   (strict; comparable only at equal isolation)")
    print(f"DESIGN_PASS_RATE          {design_ok}/{len(runs)} = {design_rate:.1%}   (ignores off_server_calls; comparable across harnesses)")
    print(
        f"SERVER_UNUSED             {server_unused}/{len(runs)}"
        + (
            "   <- these runs measured the harness, not the server"
            if server_unused
            else "   (every run reached konnect)"
        )
    )
    print(f"SAFETY_VIOLATIONS        {safety_total}   (forbidden + safety + disk_mutation)")
    print(
        f"UNNECESSARY_CALL_RATE    {unnecessary_rate:.1%}   "
        f"({unnecessary_total}/{scored_total} calls outside allowed_tools)"
    )
    if instability_rate is None:
        print("INSTABILITY_RATE         n/a   (needs --repeat >= 2)")
    else:
        print(f"INSTABILITY_RATE         {instability_rate:.1%}   (runs off their task's modal outcome)")
    print(
        f"OFF_SERVER_CALLS         {off_total}"
        + ("   <- measurement contaminated" if off_total and isolation == "tools-off" else "")
        + ("   (agent could only reach konnect)" if not off_total else "")
        + (
            "   (built-in tools were invoked; isolation=read-only-sandbox, cannot confirm off-server write)"
            if off_total and isolation != "tools-off"
            else ""
        )
    )
    print(
        f"ON_SERVER_PASS_RATE       {reached_ok}/{len(reached)}"
        + (f" = {reached_rate:.1%}   (design pass among runs that reached konnect)" if reached else "   (no run reached konnect)")
    )
    print(f"TOOL_CALLS median/task   {statistics.median(r.tool_calls for r in runs):.0f}")
    print(f"COST_USD total           {_fmt_cost([r.cost_usd for r in runs])}")
    for task_id, rs in by_task.items():
        print(f"  {task_id:<22} {_fmt_cost([r.cost_usd for r in rs])}")

    by_kind: collections.Counter[str] = collections.Counter()
    for r in runs:
        by_kind.update(v["kind"] for v in r.violations)
    if by_kind:
        print("violations by kind:      " + ", ".join(f"{k}:{n}" for k, n in by_kind.most_common()))

    # `SUCCESS_RATE` counts an off-server call as a failed run, which is the
    # right rule only where the harness can be stopped from making one. At
    # `read-only-sandbox` it cannot (K.1.3), so enforcing `min_pass_rate` on it
    # would be a permanent FAIL that measures the harness's built-ins — the
    # very thing the `off_server_calls` check already SKIPs here, re-entering
    # through the back door. The gate there is `ON_SERVER_PASS_RATE`: of the
    # runs that actually reached Konnect, how many built the design. Runs that
    # never reached it are excluded rather than counted as passes, so a harness
    # cannot clear this threshold by ignoring the server — `SERVER_UNUSED` is
    # printed right above it for exactly that reason, and stays the number to
    # read first.
    gated_rate, gated_limit = (
        (pass_rate, "0.95")
        if isolation == "tools-off"
        else (
            reached_rate,
            f"0.95 (on ON_SERVER_PASS_RATE, {len(reached)}/{len(runs)} runs; "
            f"isolation={isolation} cannot remove built-ins)",
        )
    )
    checks = [
        ("min_pass_rate", f"{gated_rate:.1%}", gated_rate >= THRESHOLDS["min_pass_rate"], gated_limit),
        ("max_safety_violations", str(safety_total), safety_total <= THRESHOLDS["max_safety_violations"], "0"),
        (
            "max_unnecessary_call_rate",
            f"{unnecessary_rate:.1%}",
            unnecessary_rate <= THRESHOLDS["max_unnecessary_call_rate"],
            "0.05",
        ),
    ]
    if instability_rate is None:
        checks.append(("max_instability_rate", "n/a", None, "0.05"))
    else:
        checks.append(
            (
                "max_instability_rate",
                f"{instability_rate:.1%}",
                instability_rate <= THRESHOLDS["max_instability_rate"],
                "0.05",
            )
        )
    if isolation == "tools-off":
        checks.append(("off_server_calls", str(off_total), off_total == 0, "0"))
    else:
        checks.append(
            (
                "off_server_calls",
                str(off_total),
                None,
                f"0 (skip: isolation={isolation}, built-in tools cannot be removed)",
            )
        )

    print("\nTHRESHOLDS")
    failed = 0
    for name, value, ok, limit in checks:
        verdict = "SKIP" if ok is None else ("PASS" if ok else "FAIL")
        failed += 1 if ok is False else 0
        print(f"  {verdict}  {name:<26} {value:>8}   (limit {limit})")

    if per_task_instability and any(v > 0 for v in per_task_instability.values()):
        print("unstable tasks:          " + ", ".join(
            f"{t}:{v:.0%}" for t, v in per_task_instability.items() if v > 0
        ))

    # Printed for every run, passing or failing: an unwrapped gateway call makes
    # a verdict unreliable in both directions, so hiding it behind a FAIL would
    # let a false pass through silently.
    for r in runs:
        if r.unwrap_warning:
            print(f"\nWARN {r.task_id} ({r.harness}): {r.unwrap_warning}")

    for r in runs:
        if r.success:
            continue
        print(f"\nFAIL {r.task_id}   (work={r.work})")
        if r.harness_error:
            print(f"  harness: {r.harness_error}")
        if r.off_server_names:
            print(f"  off-server tools: {r.off_server_names}")
        for a in r.assertions:
            if a["ok"] is False:
                print(f"  assert {a['kind']}: {a['detail']}")
        for v in r.violations:
            print(f"  violation {v['kind']}: {v['detail']}")
        print(f"  tools called: {r.tool_call_sequence}")

    return failed


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--harness", choices=sorted(HARNESSES), default="claude")
    ap.add_argument("--label", default="unlabeled")
    ap.add_argument("--task", default=None)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--model", default=None, help="harness model override")
    ap.add_argument(
        "--max-budget-usd", type=float, default=1.00, help="per-run spend cap handed to the harness (claude only)"
    )
    ap.add_argument(
        "--harness-timeout", type=float, default=900.0, help="seconds before a run is killed and failed"
    )
    ap.add_argument("--log-dir", default=None, help="write each run's raw transcript here")
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="build $WORK, the fixture and the MCP config, print the command line, spend nothing",
    )
    ap.add_argument("--enforce", action="store_true", help="exit 1 if any threshold fails")
    ap.add_argument("--out", default=None)
    ap.add_argument(
        "--agy-mcp-config",
        default=str(AGY_GLOBAL_MCP_CONFIG),
        help="agy's global per-user MCP config path (agy harness only; "
        "override with a throwaway file for testing — see AgyMcpConfigGuard)",
    )
    args = ap.parse_args()

    # Resolved once, here, so every downstream user (argv builders,
    # `check_assertions`'s own `McpStdioClient`) works on the same absolute
    # path. A relative `--server` (e.g. `target/release/konnect.exe`) reaches
    # `subprocess.Popen`/`CreateProcess` unresolved on Windows and fails with
    # `FileNotFoundError: [WinError 2]` — previously only in
    # `check_assertions`, i.e. *after* the paid harness run.
    args.server = str(Path(args.server).resolve())
    args.config = str(Path(args.config).resolve())

    tasks = load_tasks(args.task)
    if not tasks:
        raise SystemExit("no tasks matched")
    prompts = load_prompts()

    def _run_all() -> list[HarnessRun]:
        out: list[HarnessRun] = []
        for task in tasks:
            for i in range(args.repeat):
                run = run_task(task, prompts, args, i)
                if run is not None:
                    out.append(run)
        return out

    # `AgyMcpConfigGuard` writes into a personal, global, per-user file — it
    # must never be constructed for `--dry-run` (which spends and touches
    # nothing) or for any harness other than agy.
    if args.harness == "agy" and not args.dry_run:
        with AgyMcpConfigGuard(Path(args.agy_mcp_config).resolve(), args.server, args.config):
            runs = _run_all()
    elif args.harness == "codex" and not args.dry_run:
        # Same rule: a guard that copies credentials is never built for a
        # `--dry-run`, which spends nothing and touches nothing.
        with CodexHomeGuard(codex_user_home()):
            runs = _run_all()
    else:
        runs = _run_all()

    if args.dry_run:
        return
    if not runs:
        raise SystemExit("no runs produced")

    failed = report(runs, args)

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps(
                {"label": args.label, "harness": args.harness, "runs": [asdict(r) for r in runs]},
                indent=2,
            ),
            encoding="utf-8",
        )
        print(f"\nwrote {args.out}")

    if args.enforce and failed:
        raise SystemExit(f"{failed} threshold(s) failed")


if __name__ == "__main__":
    main()
