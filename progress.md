# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.4 closes, P.6.5 à
P.6.9 restent. Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers
`agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.4 — une violation ERC/DRC conserve tous ses items. `ReportItem
{ description, pos, uuid }` et un décodeur unique `parse_report_items`
alimentent `parse_erc_json` et `parse_drc_json` ; `items: Vec<ReportItem>` est
porté par `ErcViolation` et `DrcViolation` et ressort dans les trois sorties
d'outil. `pos` reste le raccourci dérivé de `items[0]`, donc aucun consommateur
ne casse, et `rule` reste `Option<String>`.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 50 suites ok, 0 échec
- sondes live `cli_tools` avec `KICAD_CLI` : PASS, 7/7

## Décisions actives

- **D114** — `gh` résout par défaut vers le remote **upstream**
  `mixelpixx/Konnect`. Toute commande `gh` visant notre travail doit porter
  `-R nevenfo/kicad-agentic-mcp`, sans quoi on lit les PR d'upstream (leur #10
  est mergée et sans rapport avec la nôtre).
- **D113** — `conformance_test` se saute **en silence** (« SKIP: no KiCAD demos
  found ») quand il ne localise pas les démos : son lookup ne connaît pas
  l'install `%LOCALAPPDATA%`. Trois tests « passed » en 0.00 s signifient donc
  zéro schéma vérifié. Toujours lancer avec
  `KICAD_DEMOS=%LOCALAPPDATA%\Programs\KiCad\10.0\share\kicad\demos`, et
  vérifier la ligne « parsed 115/115 ».
- **D112** — mesures d'oracle de P.6.2 : le bloc `(netclass …)` inséré dans le
  board fait sortir `kicad-cli` en **code 3** ("Échec du chargement de la
  carte") sans écrire de rapport. Et un vrai `.kicad_pro` KiCad 10 porte
  `net_settings.{classes, meta.version 4, netclass_patterns}` ; le champ de
  largeur s'y nomme `track_width`, pas `trace_width` comme l'argument MCP.
- **D110** — mesure d'oracle de P.6.1 : sur une carte non routée,
  `kicad-cli pcb drc --format json` écrit ses deux erreurs sous
  `unconnected_items` et **aucun** `pos` au niveau violation. La position vit
  sur chaque item. `schematic_parity` est présent et vide, d'où la distinction
  absent/vide.
- **D111** — `tests/fixtures/test.kicad_pcb` est un fichier KiCad 8
  (`version 20240108`) que KiCad 10 refuse de charger. Toute preuve passant par
  `kicad-cli` utilise `unrouted.kicad_pcb` ou `harness::BLANK_BOARD`.
- **D102** — ancres upstream vérifiées dans ce dépôt : `#144` = merge `8dd54e8`
  (corrige l'issue `#143`), `#209` = merge `1d31ad4`. Baseline du fork :
  `5cd6454`, merge-base avec `upstream/main`.
- **D104** — oracle KiCad : `kicad-cli` **10.0.3** en local
  (`%LOCALAPPDATA%\Programs\KiCad\10.0\bin`), **10.0.5** épinglé dans
  `e2e-kicad.yml` et inchangé.
- **D107** — P.4 s'est arrêtée à la classification ; les 15 items
  `BACKPORT NOW` sont P.6, seul `#174` a été backporté dans P.4.
- **D108** — deux des correctifs les plus graves ont atterri directement sur
  `upstream/main` (`e7eeeac`, `9a56233`) : une énumération par `--merges` ne
  peut pas les voir. Vaut pour le triage P.6.9.
- **D109** — aucun contenu portant des backslashes ne doit transiter par une
  heredoc : celle de ce shell les mange, même en `<<'EOF'`, et une heredoc
  Python les relit comme échappements, en silence. Tout contenu de ce genre
  passe par Write/Edit.
- Les décisions V1 antérieures (INV6, D97…D101) restent actives. D103, D105 et
  D106 sont résolues et retirées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-core/src/tools/cli.rs` — `ReportItem`, `parse_report_items`,
  `ErcViolation`, `DrcViolation`, `DrcCategory`, `DrcReport`, `parse_erc_json`,
  `parse_drc_json`, modules `erc_parse_tests` et `drc_parse_tests`.
- `crates/konnect-core/src/tools/{verification,pcb_export,sch_export}.rs` —
  sorties JSON de `run_drc`, `get_drc_violations` et `run_erc`.
- `crates/konnect-core/src/evidence/validators.rs` — gate d'evidence, `location`
  encore dérivé de `pos`.
- `crates/konnect-sexp/src/schematic.rs` — `extract_power_symbol_labels`,
  `extract_all_net_labels`, `LibPin::electrical_type`.
- `crates/konnect-core/src/tools/pcb_board.rs` — cible de P.6.5 (`#142`, lecture
  des nets de pad par index fixe) et de P.6.6 (`#153`, `add_layer`).
- `crates/konnect-core/tests/cli_tools.rs` + `fixtures/unrouted.kicad_pcb`.
- `docs/upstream-audit.md` — source des items P.6.5 à P.6.9.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.5 — `#142` : sur une carte KiCad 10, le net d'un pad est lu à un
index fixe, donc tout pad rapporte un net vide ; les comptages de nets et les
ids rendus par `add_net` sont dérivés par sous-chaîne. Relire l'entrée `#142` de
`docs/upstream-audit.md` pour la mécanique exacte avant de coder, puis valider
par `cargo fmt` / `clippy -D warnings` / `cargo test --workspace --lib --tests`
plus la sonde live `cli_tools` avec `KICAD_CLI`.
