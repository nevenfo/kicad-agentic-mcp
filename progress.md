# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. Branche
de travail : `ai/P-schematic-fidelity`, partant de `cdc7273`.

## Tâche actuelle

Aucune en cours. P.6 (backlog de correctness upstream différé) est ouverte et
non commencée.

## Dernière tâche validée

P.5 — release gate. `e2e-kicad.yml` devient un workflow `workflow_call` appelé
par `release.yml` avec `gating: true` ; le job `release` a `needs: [build,
pcm-package, e2e-kicad]`. Le trigger `push: tags` a été retiré d'`e2e-kicad.yml`
(il provoquait un double run au tag) ; `live-ipc`, piloté par une GUI pcbnew,
est exclu du gate par `if: ${{ !inputs.gating }}` et reste sur le run
hebdomadaire.

Validation de l'ensemble P.1–P.5 :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 50 binaires ok, 0 échec
- `cargo test --workspace --locked --doc` : PASS
- `cargo build --release -p konnect` : PASS
- oracle `kicad-cli` 10.0.3, `schematic_fidelity_live -- --ignored` : 2/2 PASS
- YAML des trois workflows relu et parsé ; graphe de jobs vérifié

## Décisions actives

- **D102** — ancres upstream vérifiées dans ce dépôt : `#144` = merge `8dd54e8`
  (corrige l'issue `#143`), `#209` = merge `1d31ad4`. Baseline du fork :
  `5cd6454`, merge-base avec `upstream/main`.
- **D104** — oracle KiCad : `kicad-cli` **10.0.3** en local
  (`%LOCALAPPDATA%\Programs\KiCad\10.0\bin`), **10.0.5** épinglé dans
  `e2e-kicad.yml` et inchangé.
- **D106** — la fixture `derived_lib_name.kicad_sch` discrimine au niveau de
  l'oracle, pas seulement des assertions internes : mesuré avec `kicad-cli`,
  elle produit 6 nets nommés avec `lib_name` préservé et 2 sans (les 4 nets
  dérivés disparaissent, remplacés par un `Net-(C1-Pad??)` auto-généré), et
  KiCad n'émet aucun avertissement dans les deux cas.
- **D107** — P.4 s'arrête à la classification. L'audit a produit 15 items
  `BACKPORT NOW` (~1600 lignes) ; les implémenter aurait été la synchronisation
  générale avec upstream que le brief de phase interdit. Seul **#174** a été
  backporté dans P.4, parce qu'il tient en une fonction et se prouve par un
  test trivial. Le reste est P.6.
- **D108** — deux des correctifs les plus graves de l'audit ne figuraient dans
  aucune liste de candidats : `e7eeeac` et `9a56233` ont atterri directement sur
  `upstream/main`, donc une énumération par `--merges` ne pouvait pas les voir.
  Toute reprise de l'audit doit énumérer aussi les commits directs.
- **D109** — la heredoc de cet environnement shell ne préserve pas les
  backslashes, même en `<<'EOF'`. Tout contenu qui en comporte passe par
  Write/Edit, jamais par heredoc.
- Les décisions V1 antérieures (INV6, D97…D101) restent actives. D103 et D105
  sont résolues et retirées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-schematic-editor/src/schematic/{mod,symbol,sheet}.rs` —
  `paper_args`, `Symbol::lib_name`, `exclude_from_sim`, `unmodelled_children`.
- `crates/konnect-sexp/src/schematic.rs` — `find_lib_symbol`,
  `SymbolInstance::lib_symbol_name`. `parser.rs` — `unescape` en un passage.
- `crates/konnect-core/tests/schematic_fidelity_live.rs` — oracle KiCad,
  `#[ignore]`, câblé dans le job gatant.
- `crates/konnect-core/tests/fixtures/derived_lib_name.kicad_sch`.
- `docs/upstream-audit.md` — classification bornée, 553 lignes, sources de P.6.
- `.github/workflows/{release,e2e-kicad}.yml`.

## NEXT ACTION

Implémenter P.6.1 — `e7eeeac` : faire lire à `run_drc` les trois tableaux du
JSON de `kicad-cli pcb drc` (`violations`, `unconnected_items`,
`schematic_parity`) et corriger `pos`, lu au niveau de la violation alors que
KiCad l'écrit sur chaque item impliqué. Écrire d'abord le test rouge sur une
fixture de board au cuivre non routé, vérifier qu'il passe aujourd'hui le gate
d'evidence, puis corriger `crates/konnect-core/src/tools/cli.rs` et relancer
`cargo test -p konnect-core` plus l'oracle `kicad-cli`.
