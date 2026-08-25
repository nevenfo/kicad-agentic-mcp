# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7 est
ouverte (les huit items d'origine clos, P.6.7.9 à P.6.7.11 ouvertes), P.6.10,
P.6.9 (triage) et P.6.9.1 closes. P.6.9.2 à P.6.9.12 restent, dans l'ordre du
triage ; P.6.8 et P.6.11 aussi. Branche de travail : `ai/P-schematic-fidelity`,
PR #10 vers `agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.9.1 — un nom de layer que l'API ne sait pas représenter ne part plus vers
KiCAD. `try_layer_from_name` **dérive** la correspondance au lieu de la lister :
les noms de l'enum proto *sont* les noms KiCad, `.` remplacé par `_` derrière un
préfixe `BL_`, donc `from_str_name` répond pour tout ce que le schéma connaît —
cuivre interne jusqu'à `In30.Cu`, `User.1` à `User.45` par-dessus le trou
`BL_Rescue = 62` — sans arithmétique à se tromper. Les trois sentinelles sont
refusées par nom, et `build_footprint_item` contrôle le layer du footprint,
ceux de chaque pad et de chaque graphic **avant** de construire le moindre
enfant : en aval il n'y a pas d'erreur à rattraper, seulement un éditeur mort.

Mesure sur le corpus installé, pas une estimation : **915 des 15 433 footprints
officiels (5,9 %)** nomment un layer que l'ancienne table de quinze entrées
ignorait — `Dwgs.User`, `Cmts.User`, `F.Adhes`, `Margin`, `User.2` et tout le
cuivre interne au-delà de `In2.Cu`. Cette mesure est devenue
`crates/konnect-ipc/tests/layer_corpus_test.rs`, dans le job E2E gatant.

Le corpus complet a trouvé ce que l'échantillon avait manqué : `*.SilkS`, un
quatrième wildcard de pad que personne n'avait développé (pads NPTH de
`Connector_RJ` et `Heatsink`). Il était silencieusement retiré du pad ; avec la
validation, il aurait fait échouer le placement entier. Il est développé sur les
deux faces sérigraphie.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, **51 suites, 1231
  tests, 0 échec**
- corpus de layers : **15 433 footprints, 51 noms distincts**, 0 non
  représentable, en 1,8 s
- rouge d'abord, chaque moitié neutralisée tour à tour : table dérivée (4 tests
  rouges), validation (1), expansion `*.SilkS` (1)
- borne énoncée : **aucune sonde live**. La mesure amont est KiCAD qui faute à
  `0xc0000005`, ce qui exige une session GUI avec l'API activée ; les assertions
  portent donc sur ce qui sort du processus — le layer réellement émis et le
  refus qui arrête un layer sans représentation.

## Décisions actives

- **D120** — l'enum `BoardLayer` de kiapi se nomme comme KiCad : `BL_` + le nom
  du layer avec `.` → `_` (`Dwgs.User` → `BL_Dwgs_User`, `User.10` →
  `BL_User_10`). Toute correspondance nom↔layer passe par `from_str_name`, pas
  par une table écrite à la main ni par de l'arithmétique sur les ids : c'est ce
  qui rend la couverture automatique quand KiCad ajoute un layer. Corollaire :
  `BL_UNKNOWN`/`BL_UNDEFINED`/`BL_UNSELECTED` ne doivent jamais être renvoyés
  par nom — KiCAD n'a pas de validation de layer scalaire côté réception.
- **D119** — le triage P.6.9 vaut aussi comme mesure d'écart : ce fork n'a ni
  `pcb_sync.rs`, ni `apply_footprint_fields`, ni `update_pcb_from_schematic`, et
  discrimine déjà les enfants d'un footprint par `type_url`
  (`konnect-ipc/src/transform.rs:282-295`). Un item upstream touchant la sync
  n'a donc pas de site ici ; si une telle voie est ajoutée un jour, reprendre
  `2904841` puis `59d0ead`, dans cet ordre.
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
  numérotation du board, jamais d'un intervalle fixe — c'est P.6.11. Voisin
  laissé en place par P.6.9.1 : `konnect-ipc/src/client.rs:1314` développe
  `*.Cu` en `3..=34`, même hypothèse d'intervalle fixe, dans la fonction que
  P.6.9.1 vient de toucher.
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
  `-R nevenfo/kicad-agentic-mcp`, sans quoi on lit les PR d'upstream.
- **D113** — *résolue par P.6.10 et conservée pour la leçon* : le lookup de
  `conformance_test` ignorait l'install `%LOCALAPPDATA%`, si bien que les tests
  se sautaient **en silence**. Le lookup connaît désormais ce chemin, un
  `KICAD_DEMOS` explicite mais introuvable échoue, et les comptes sont assertés.
  La leçon reste : un test qui peut se sauter doit rendre son silence visible.
  `layer_corpus_test` suit la même règle : `KICAD_FOOTPRINTS` explicite mais
  introuvable échoue, et les comptes sont assertés et affichés.
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
  `e2e-kicad.yml` et inchangé. Les librairies officielles de la même install
  (`share\kicad\footprints`, 15 433 `.kicad_mod`) sont l'oracle des noms de
  layers.
- **D107** — P.4 s'est arrêtée à la classification ; les 15 items
  `BACKPORT NOW` sont P.6, seul `#174` a été backporté dans P.4.
- **D108** — deux des correctifs les plus graves ont atterri directement sur
  `upstream/main` (`e7eeeac`, `9a56233`) : une énumération par `--merges` ne
  peut pas les voir. C'est ce qui a motivé P.6.9, désormais close.
- **D109** — aucun contenu portant des backslashes ne doit transiter par une
  heredoc : celle de ce shell les mange, même en `<<'EOF'`, et une heredoc
  Python les relit comme échappements, en silence. Tout contenu de ce genre
  passe par Write/Edit. Reconfirmé pendant P.6.9.1 : `share\kicad\footprints`
  écrit par une heredoc Python est devenu `share\kicad` + form feed +
  `ootprints` dans le workflow, sans aucun message.
- Les décisions V1 antérieures (INV6, D97…D101) restent actives. D103, D105 et
  D106 sont résolues et retirées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `docs/upstream-audit.md` — annexe A : le triage P.6.9, avec pour chaque item
  mécanisme amont, état dans ce fork (`file:line`), impact et coût.
- `crates/konnect-core/src/tools/pcb_routing.rs` + `pcb_board.rs` — les deux
  `find_net_id` (l.52/l.113) et le template de zone (l.45), cibles de P.6.9.2.
- `crates/konnect-sexp/src/net.rs` — lecture des nets par forme (P.6.5), base
  du write-side de P.6.9.2.
- `crates/konnect-ipc/src/builders.rs` — `try_layer_from_name` / `layer_from_name` ;
  `crates/konnect-ipc/src/client.rs` — `check_layer`, `build_footprint_item`,
  `build_graphic_child`.
- `crates/konnect-core/src/tools/mod.rs` — `project_name_for` l.452,
  `ensure_root_uuid` l.497 (P.6.9.3), `require_str`/`require_f64`/`get_path`
  l.414-447 (P.6.9.7).
- `crates/konnect-schematic-editor/src/sexp/writer.rs` — les trois causes du
  reformatage (P.6.9.4).
- `crates/konnect-core/src/tools/cli.rs` — `DrcReport`, base de P.6.9.8.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.9.2 — `f2372ca` : les deux copies privées de `find_net_id`
(`pcb_board.rs:113` et `pcb_routing.rs:52`) résolvent un nom de net par offset
de chaîne, et un board KiCad 10 n'a pas d'ids à trouver, donc chaque zone part
en `(net 0) (net_name "GND")` sur le pseudo-net non connecté, rapportée comme un
succès. Ajouter le pendant write-side dans `konnect_sexp::net` (la détection par
forme y est déjà, D115) : `(net "<nom>")` sans `net_name` et `(layers …)`
pluriel sur un board KiCad 10, la paire id + `net_name` et `(layer …)` singulier
sur un board legacy avec l'id résolu depuis la table, et un **refus** nommant
`add_net` quand un board legacy ne déclare pas le net, au lieu de le mettre à 0.
Supprimer les deux copies. Rouge d'abord sur un board de chaque forme. La
seconde moitié amont — refuser l'édition quand KiCAD tient ce board ouvert —
n'est pas dans cette tâche.
