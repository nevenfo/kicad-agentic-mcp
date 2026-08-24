# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7
est ouverte (P.6.7.1 à P.6.7.6 closes, P.6.7.7 à P.6.7.10 restent), P.6.8 à P.6.11
restent. Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers
`agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.7.6 — `export_bom` honore `exclude_dnp`, et `format` cesse de mentir.
`BomOptions` + `bom_args` dans `cli.rs` (le flag s'assert sans KiCad), le
handler de `sch_export.rs` lit `exclude_dnp` (défaut `true` du schéma) et
passe `--exclude-dnp`. `format` est un ensemble fermé de `"csv"`, déclaré en
`enum` et refusé sinon — voir D118. `manufacturing.rs` garde son comportement
par `BomOptions::default()`.

Changement de contrat assumé : `export_bom` sans `exclude_dnp` applique
désormais le `true` que son schéma annonçait déjà, donc les composants DNP
cessent d'apparaître par défaut.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 50 suites, 0 échec
- sondes live `cli_tools` avec `KICAD_CLI` : PASS, 9/9
- rouge-avant vérifié sur le seul oracle honnête, le CSV écrit par KiCad : avec
  `exclude_dnp: true`, le composant DNP **R2 restait dans la BOM**

## Décisions actives

- **D118** — `kicad-cli sch export bom` (10.0.3) n'a **aucune** option
  `--format` ; il expose `--fields`, `--labels`, `--group-by`, `--sort-field`,
  `--filter`, `--exclude-dnp` et des délimiteurs. Toute option annoncée par un
  schéma de tool doit être vérifiée contre le `--help` de la CLI installée
  avant d'être implémentée : un argument que KiCad refuse fait échouer l'export
  entier, et un diff upstream n'est pas une autorité sur la CLI locale.
- **D117** — les boards KiCad récents (≥ `20241229`, mesuré sur `CM5_MINIMA_3`
  et `video`) numérotent le cuivre en **pairs** : `F.Cu`=0, `B.Cu`=2,
  `In1.Cu`=4, `In2.Cu`=6. L'ancien schéma (`B.Cu`=31, internes 1..30) ne vaut
  que pour les fichiers plus vieux, dont notre fixture `unrouted.kicad_pcb`.
  Tout code qui alloue un id de layer doit le dériver du nom canonique sous la
  numérotation du board, jamais d'un intervalle fixe — c'est P.6.11.
- **D116** — un board livré par KiCad 10 peut être réellement malformé :
  `demos/royalblue54L_feather/RoyalBlue54L-Feather.kicad_pcb` ferme sa racine
  à l'octet 14735 sur 3,6 Mo et finit 349 parenthèses fermantes en avance.
  Vérifié : un scan de balance rend 0 sur `interf_u` et `pic_programmer`, donc
  la mesure est bien celle du fichier. Toute conformance de board doit traiter
  ce cas comme un échec attendu, pas comme une régression du parser.
- **D115** — oracle de forme des nets, mesuré sur les 18 boards de démo :
  la version **20260206** est la bascule. Elle supprime la table de nets et
  écrit `(net "<nom>")` sur chaque item ; tout ce qui va jusqu'à 20250907 garde
  la table et `(net <id> "<nom>")`. Se discrimine par `SexpNode::Str` contre
  `SexpNode::Atom` en position 1, jamais par un numéro de version.
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
  `5cd6454`, merge-base avec `upstream/main`. Le code d'un item backporté se
  lit directement ici : pour un merge, `git diff <parent1> <parent2> -- <path>`.
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

- `crates/konnect-sexp/src/layers.rs` — stackup lu par forme, noms canoniques.
- `crates/konnect-sexp/src/net.rs` — accesseurs de nets des deux formes.
- `crates/konnect-core/src/tools/pcb_board.rs` — `close_of_block`,
  `entry_indent`, `handle_add_layer`, `handle_get_board_info`, module
  `layers_block_tests`.
- `crates/konnect-core/src/tools/cli.rs` — `ReportItem`, `parse_report_items`,
  `ErcViolation`, `DrcViolation`, `DrcCategory`, `DrcReport`, `parse_erc_json`,
  `parse_drc_json`, modules `erc_parse_tests` et `drc_parse_tests`.
- `crates/konnect-core/src/evidence/validators.rs` — gate d'evidence, `location`
  encore dérivé de `pos`.
- `crates/konnect-sexp/src/schematic.rs` — `extract_power_symbol_labels`,
  `extract_all_net_labels`, `LibPin::electrical_type`.
- `crates/konnect-sexp/src/parser.rs` — `parse_sexp` l.89-111, la retombée
  « implicit List » que P.6.10 doit traiter.
- `crates/konnect-core/tests/cli_tools.rs` + `fixtures/unrouted.kicad_pcb`.
- `docs/upstream-audit.md` — source des items P.6.7 à P.6.9.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.7.7 — `#266` : l'export PCB `pdf` et `svg` répète `--layers`
une fois par couche, alors que KiCad 10 attend une seule valeur séparée par des
virgules et rejette le doublon ; `--mode-single` n'est jamais passé, donc un
tracé en fichier unique n'est pas demandé. Upstream factorise
`single_file_pcb_export_args` et ajoute `cli_failure_diagnostics`, parce que
`kicad-cli` écrit son message « Duplicate argument » sur **stdout**, que le
chemin d'erreur jetait. Vérifier d'abord le `--help` réel de
`kicad-cli pcb export pdf` et `svg` (voir D118), puis relire l'entrée `#266` de
`docs/upstream-audit.md`. Sonde live attendue : un export multi-couches qui
produit réellement son fichier. Valider par `cargo fmt` / `clippy -D warnings` /
`cargo test --workspace --lib --tests` plus `cli_tools -- --ignored`.
