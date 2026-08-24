# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.3 closes, P.6.4 à
P.6.9 restent. Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers
`agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.3 — les symboles d'alimentation nomment enfin les nets. `LibPin` lit le type
électrique (premier atome de `(pin power_in line …)`) et seules les pins
`power_in` nomment un net, donc un `PWR_FLAG` (`power_out`) ne renomme pas le
rail qu'il signale. Tous les consommateurs du graphe passent par
`extract_all_net_labels`, sauf `find_orphan_items`, laissé sur `extract_labels`
comme upstream. `sch_bridge.rs` n'est pas touché.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 0 échec
- `conformance_test` avec `KICAD_DEMOS` : PASS, 115/115 schémas parsés
- sondes live `cli_tools` 7/7 et `schematic_fidelity_live` 2/2 : PASS
- sur `power_symbol_divider.kicad_sch` : 3 nets vus au lieu de 1

## Décisions actives

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
- **D110** — mesure d'oracle de P.6.1, réutilisable pour P.6.4 : sur une carte
  non routée, `kicad-cli pcb drc --format json` écrit ses deux erreurs sous
  `unconnected_items` et **aucun** `pos` au niveau violation. L'ancien parsing
  voyait donc 0 erreur sur 2 et toutes les positions nulles. `schematic_parity`
  est présent et vide, d'où la distinction absent/vide.
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
  Python les relit comme échappements (`` devient un backspace, en
  silence). Tout contenu de ce genre passe par Write/Edit.
- Les décisions V1 antérieures (INV6, D97…D101) restent actives. D103, D105 et
  D106 sont résolues et retirées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-core/src/tools/cli.rs` — `DrcCategory`, `DrcReport`,
  `parse_drc_json`, `parse_erc_json`, module `drc_parse_tests`.
- `crates/konnect-core/src/evidence/validators.rs` — gate d'evidence.
- `crates/konnect-core/src/tools/{verification,pcb_export}.rs` — `handle_run_drc`
  et `handle_get_drc_violations`, ventilation `by_category`.
- `crates/konnect-sexp/src/schematic.rs` — `extract_power_symbol_labels`,
  `extract_all_net_labels`, `LibPin::electrical_type`.
- `crates/konnect-core/src/tools/sch_analysis.rs` — `build_net_graph` et ses
  consommateurs, `with_power_symbol_labels`.
- `crates/konnect-core/src/tools/pcb_routing.rs` — `load_project_settings`,
  `save_project_settings`, `NETCLASS_FIELDS`, module `netclass_tests`.
- `crates/konnect-core/tests/cli_tools.rs` + `fixtures/unrouted.kicad_pcb`,
  `crates/konnect-core/tests/config_and_rules.rs`.
- `docs/upstream-audit.md` — source des items P.6.4 à P.6.9.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.4 — `#297` + `#298` : seul `items[0]` d'une violation ERC/DRC
survit, alors qu'une violation en nomme régulièrement deux ; un conflit
`pin_to_pin` perd la pin qui l'explique, et deux `unconnected_items` partageant
règle, description et première position deviennent indiscernables. Promouvoir
`items` en `Vec` sur `ErcViolation` et `DrcViolation` avec un seul décodeur
d'item partagé, dans `crates/konnect-core/src/tools/cli.rs` (`parse_erc_json`
l.~140, `parse_drc_json` ajouté en P.6.1). Préserver la divergence connue de ce
fork : `rule` y est `Option<String>` là où upstream a `String`. Le JSON réel
d'une violation à deux items est déjà en fixture dans `drc_parse_tests`.
