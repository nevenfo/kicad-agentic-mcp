# PROGRESS

## Phase actuelle

J.2 — raise capability coverage.

## Tâche actuelle

J.2.2 — fill the highest-value `MISSING` gaps (buses, standalone drill export,
IPC-D-356, the stackup write KiCad 10 declares and does not implement).

## Dernière tâche validée

J.2.1 — define the coverage comparison target the V1 criterion needs.

Validation :
- `cargo test --workspace`: 858 PASS, 11 ignored
- `cargo test -p konnect-core --test capability_matrix`: 13 PASS, including
  `the_frozen_baseline_measurement_still_holds`, which re-derives both frozen
  lists from `git archive 5cd6454` in the default gate
- `cargo clippy -p konnect-core --lib -- -D warnings`: PASS
- matrix regenerated; new *V1 comparison target* section reads
  baseline 42/186 = 22.6 %, fork 55/186 = 29.6 %, 0 regressions

## Décisions actives

- D44 — `CAPABILITY_COVERAGE`'s comparison target is frozen: the 187 tools the
  baseline registers at `5cd6454` (this fork registers all 187, so no name
  mapping), minus what KiCAD gives no API for → denominator 186. Both numerators
  come from the same scanner pointed at each tree. The criterion is met only
  when strictly ahead *and* no tool the baseline proved is unproved here. The
  headline 28.6 % is the whole-surface number and is not the criterion.
- D43 — Direct/Agent is an explicit gateway entry-point choice. Direct remains
  `kicad_describe`/`kicad_invoke` and never starts an LLM. `ESCALATE` returns
  structured failure/evidence to the caller; it never silently contacts an
  external model.
- D42 — generic pin offsets do not carry E27's prompt effect. Agent retrieval
  must combine task-specific electrical and Plan IR constraints with geometry.
- D40 — router tiers are `NO_LLM | LOCAL | ESCALATE`; no measurable middle rung.
- D39 — uniquely resolvable installed-library names are canonicalized; ambiguous
  names remain failures with candidates.
- D38 — local model is `gpt-oss-20b`, `medium`, ctx 32 768.
- D35/D33 — one repair round buys no success; `strict_json` stays off.
- E.6.1 — a context with missing backend counts is `Unmeasured`; the initial
  local profile reserves the measured 5 120 tokens.
- E.6.2 — tokenisation is backend-specific: callers supply measured token costs.
  The durable task core is non-evictable; only old transcript is compacted.
- E.6.3 — retrieval order is caller-owned; bundles are atomic.
- H.7.1 — Agent is `kicad_agent`, separate from Direct. LOCAL provider injection
  is opt-in through a loopback URL; model replies remain assumptions until
  deterministic verification.
- H.7.2 — `kicad_agent_verify` returns `PASS | FAIL | COULD_NOT_RUN`; only a
  completed validator/cache verdict becomes a verified fact.
- H.7.3 — the local completion is constrained by the measured non-strict Plan IR
  JSON Schema at temperature 0.2; Direct Plan IR and the `apply_plan`
  `{"plan": ...}` wrapper normalize to the same deterministic path.
- J.1 — `find_single_pin_nets` is pin-aware and promises only pins without a
  wire/label or explicit `no_connect`; it stays advisory/`PARTIAL`.

## Blocage actif

Phase I remains gated: this machine has KiCad 10.0, not the required KiCad 11 /
`kicad-cli api-server`. This does not block J.2.

## Fichiers / zones utiles

- `crates/konnect-core/src/capability/baseline.rs` — frozen V1 target (J.2.1)
- `crates/konnect-core/src/capability/coverage.rs` — the scanner both sides use
- `crates/konnect-core/src/capability/render.rs` — matrix + comparison section
- `crates/konnect-core/tests/capability_matrix.rs` — matrix equality and the
  baseline re-derivation
- `crates/konnect-core/src/capability/mod.rs` — `MANIFEST`, `MISSING` (J.2.2)
- `crates/kam-runtime/src/lib.rs` — routing vocabulary and supervisor turn
- `crates/konnect-core/src/router/meta_tools.rs` — explicit Agent gateway
- `crates/konnect-core/src/agent_loop.rs` — proposal/preview/apply/verify loop
- `bench/agent_e2e.py` — reproducible H.7.3 harness
- `docs/local-agents.md` — measured profile and gateway contract
- Pre-existing H.6.1–H.6.5 changes and task-state files remain uncommitted; note
  that `cargo clippy --tests` fails inside them (`await_holding_lock` in
  `sch_components.rs` / `sch_wiring.rs` test helpers), which is why the project's
  clippy gate is `--lib`

## NEXT ACTION

Execute J.2.2 — implement the highest-value `MISSING` entries starting with the
`buses` domain, then regenerate the matrix and confirm the V1 comparison table
still reports 0 regressions.
