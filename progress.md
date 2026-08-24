# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7
est ouverte : les huit items d'origine (P.6.7.1 à P.6.7.8) sont clos,
P.6.7.9 à P.6.7.11 sont des découvertes ouvertes, P.6.8 à P.6.11
restent. Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers
`agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.7.8 — `run_erc` refuse une sous-feuille au lieu de rapporter des artefacts
d'invocation. `owning_project_root` dans `sch_export.rs` reconnaît une feuille
appartenant à un projet enraciné ailleurs, et le refus est un
`invalid_argument` structuré sur `schematic` qui **nomme la racine** à
réessayer. Borne assumée : la détection regarde le répertoire propre du fichier,
pas ses ancêtres — une feuille déplacée hors du dossier de son projet n'est pas
attrapée.

Défaut reproduit contre `kicad-cli` 10.0.3 avant de coder, sur une copie de
`demos/complex_hierarchy` : `sch erc` sur la sous-feuille `ampli_ht` rend **67
violations, dont 46 `lib_symbol_issues`**, contre **0** sur la feuille racine.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, 50 suites, 0 échec
- sondes live `cli_tools` avec `KICAD_CLI` : PASS, 10/10
- rouge-avant vérifié en neutralisant la détection : la sous-feuille résout à
  `None` et aucun refus n'est émis. La borne inverse est testée aussi
  explicitement que le défaut — racine, schéma sans projet, voisin non
  référencé et répertoire multi-projets restent intacts

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

Choisir la prochaine tâche de P.6, le lot P.6.7 d'origine étant clos. Les
candidates, par valeur décroissante :
- **P.6.10** — `parse_sexp` répond `Ok` sur un document qu'il n'a pas consommé
  (`konnect-sexp/src/parser.rs:89-111`) : c'est la même forme de faux-propre
  que P.6.1 et P.5, et elle touche **tout** lecteur de fichier. Deux parties :
  refuser une entrée non consommée plutôt que fabriquer une racine implicite,
  et une conformance de corpus sur les boards de démo qui échoue bruyamment
  (voir D113 et D116, qui donne le cas d'échec attendu).
- **P.6.9** — triage des 16 correctifs upstream partis directement sur `main`
  (annexe A de `docs/upstream-audit.md`), à faire avant d'en implémenter un.
- **P.6.11**, **P.6.7.9**, **P.6.7.10**, **P.6.8** — plus petites, chacune
  portant son action précise dans `plan.md`.

Sauf indication contraire, prendre **P.6.10** : c'est la plus large et la seule
dont dépend la confiance dans tous les autres correctifs de lecture.
