"""Read the capability registry's read/write classification.

The registry lives in Rust (`konnect_core::capability::MANIFEST` and
`tool_effect`), and `docs/capability-matrix.md` is its rendered form — generated
by `crates/konnect-core/tests/capability_matrix.rs`, which fails if the document
has drifted from the code. Reading the document is therefore reading the
registry: there is no second place for the classification to be wrong in.

The alternative — asking a running server — was rejected because the benchmark
must be able to reject a task's declared safety tier before it starts a server,
and because a tier checked against the same process it is judging is not a
check.

Column position is not assumed: the header row names the columns, and `effect`
is looked up by name. A generator that reorders its columns must not silently
turn every tool into a read.
"""

from __future__ import annotations

import functools
from pathlib import Path

MATRIX = Path(__file__).resolve().parent.parent / "docs" / "capability-matrix.md"

READ = "read"
WRITE = "write"


def _cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def _unquote(cell: str) -> str:
    return cell.strip().strip("`").strip()


@functools.lru_cache(maxsize=None)
def effects(path: str | None = None) -> dict[str, str]:
    """`{tool: "read" | "write"}` for every tool in the matrix."""
    target = Path(path) if path else MATRIX
    text = target.read_text(encoding="utf-8")

    out: dict[str, str] = {}
    tool_col: int | None = None
    effect_col: int | None = None

    for line in text.splitlines():
        if not line.startswith("|"):
            tool_col = effect_col = None
            continue
        cells = _cells(line)
        header = [_unquote(c).lower() for c in cells]
        if "tool" in header and "effect" in header:
            tool_col, effect_col = header.index("tool"), header.index("effect")
            continue
        if tool_col is None or effect_col is None:
            continue
        if all(set(c) <= set("-: ") for c in cells):  # the |---|---| rule
            continue
        if max(tool_col, effect_col) >= len(cells):
            continue
        tool = _unquote(cells[tool_col])
        effect = _unquote(cells[effect_col]).lower()
        if effect in (READ, WRITE):
            out[tool] = effect

    if not out:
        raise SystemExit(
            f"no tool/effect table found in {target} — regenerate it with "
            "KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix"
        )
    return out


META_SECTION = "meta-tools"


@functools.lru_cache(maxsize=None)
def meta_tools(path: str | None = None) -> frozenset[str]:
    """The gateway/discovery tools, read from the matrix's own `## Meta-tools`.

    Same argument as `effects()`: the registry lives in Rust
    (`capability::META_TOOL_EFFECTS`) and the matrix is its rendered form, so
    reading the document is reading the registry. A hand-kept second list here
    is exactly the drift D58 forbids — and it had already drifted: the copy
    that used to live in `bench/runner.py` named six of the thirteen, missing
    `kicad_agent_verify` among others.

    The section is found by heading, not by position, and an empty result is an
    error rather than a silent "no meta-tools" — which would put every
    discovery call back under `allowed_tools`.
    """
    target = Path(path) if path else MATRIX
    out: set[str] = set()
    in_section = False
    tool_col: int | None = None

    for line in target.read_text(encoding="utf-8").splitlines():
        if line.startswith("#"):
            in_section = line.lstrip("#").strip().lower() == META_SECTION
            tool_col = None
            continue
        if not in_section:
            continue
        if not line.startswith("|"):
            tool_col = None
            continue
        cells = _cells(line)
        header = [_unquote(c).lower() for c in cells]
        if "tool" in header:
            tool_col = header.index("tool")
            continue
        if tool_col is None or all(set(c) <= set("-: ") for c in cells):
            continue
        if tool_col < len(cells):
            out.add(_unquote(cells[tool_col]))

    if not out:
        raise SystemExit(
            f"no `## {META_SECTION}` table found in {target} — regenerate it with "
            "KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix"
        )
    return frozenset(out)


def is_write(tool: str) -> bool:
    """Fail-safe, exactly like the Rust side: an unknown tool is a write.

    A tool the matrix has never heard of is far more likely to be new than to be
    harmless, and the whole point of the `read_only` tier is that it refuses
    what it cannot vouch for.
    """
    return effects().get(tool, WRITE) == WRITE


def unknown(tools: list[str]) -> list[str]:
    """Which of `tools` the registry does not list, in the given order."""
    known = effects()
    return [t for t in tools if t not in known]


if __name__ == "__main__":  # a developer aid, not part of any measurement
    table = effects()
    writes = sum(1 for v in table.values() if v == WRITE)
    print(f"{len(table)} tools: {writes} write, {len(table) - writes} read")
    print(f"{len(meta_tools())} meta-tools: {', '.join(sorted(meta_tools()))}")
