# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 et P.6.2 closes, P.6.3 à
P.6.9 restent. Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers
`agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.2 — `create_netclass` et `assign_net_to_class` écrivent dans le `.kicad_pro`
frère, plus jamais dans le `.kicad_pcb`. La classe va dans
`net_settings.classes`, l'appartenance dans `netclass_patterns`. Absence de
`.kicad_pro` : refus explicite plutôt qu'écriture là où rien ne lit. #220
appliqué par-dessus : une mise à jour ne déplace que les champs nommés par
l'appel.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 0 échec
- sondes live `cli_tools -- --ignored` (kicad-cli 10.0.3) : 7/7 PASS
- les deux commandes exactes des steps CI ajoutés : PASS

## Décisions actives

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
- **D109** — la heredoc de cet environnement shell ne préserve pas les
  backslashes, même en `<<'EOF'`. Tout contenu qui en comporte passe par
  Write/Edit.
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
- `crates/konnect-core/src/tools/pcb_routing.rs` — `load_project_settings`,
  `save_project_settings`, `NETCLASS_FIELDS`, module `netclass_tests`.
- `crates/konnect-core/tests/cli_tools.rs` + `fixtures/unrouted.kicad_pcb`,
  `crates/konnect-core/tests/config_and_rules.rs`.
- `docs/upstream-audit.md` — source des items P.6.3 à P.6.9.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.3 — `#262` : les symboles d'alimentation sont absents du graphe
de nets du schéma, donc chaque rail `power:` se lit comme non connecté.
`build_net_graph` n'est alimenté que par `extract_labels` ; il faut
`extract_power_symbol_labels` et `LibPin::electrical_type`, sachant que
`LabelKind::PowerSymbol` existe déjà ici comme variante morte. C'est le plus
gros des items restants : lire d'abord sa section dans `docs/upstream-audit.md`
plutôt que de re-dériver le mécanisme, puis écrire le test rouge sur une
fixture portant un `power:GND`, en prenant `kicad-cli sch erc` comme oracle.
