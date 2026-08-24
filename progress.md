# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 close, P.6.2 à P.6.9
restent. Branche de travail : `ai/P-schematic-fidelity`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.1 — `run_drc` lit les trois tableaux du rapport DRC. `DrcReport` remplace
`Vec<DrcViolation>` et porte `violations`, `unconnected_items` et
`schematic_parity` en `Option<Vec<_>>` : clé absente = passe non exécutée, clé
vide = mesure propre. `pos` et la description enrichie viennent de `items[0]`,
comme le parsing ERC. `validators.rs` refuse un rapport dont une catégorie
manque au lieu de le compter zéro finding.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 0 échec
- sondes live `cli_tools -- --ignored` (kicad-cli 10.0.3) : 6/6 PASS
- la commande exacte du nouveau step CI : PASS

## Décisions actives

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
- `crates/konnect-core/tests/cli_tools.rs` + `fixtures/unrouted.kicad_pcb`.
- `docs/upstream-audit.md` — source des items P.6.2 à P.6.9.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.2 — `9a56233` + `#220` : `create_netclass` écrit un nœud
`(netclass …)` dans le `.kicad_pcb` (`crates/konnect-core/src/tools/pcb_routing.rs:643`,
insertion à `content.rfind(')')` l.657-678), ce qui produit une carte que KiCad
refuse d'ouvrir, alors que KiCad 10 ne lit les netclasses que dans le
`.kicad_pro` (`net_settings`). Écrire d'abord le test rouge qui prouve avec
`kicad-cli` que la carte écrite ne se charge plus, puis déplacer
`create_netclass` et `assign_net_to_class` vers le `.kicad_pro` frère avec refus
explicite s'il est absent, et appliquer par-dessus le correctif `#220` (une mise
à jour ne doit pas réinjecter les défauts de création).
