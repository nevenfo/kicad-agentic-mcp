# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7 est
ouverte (les huit items d'origine clos, P.6.7.9 à P.6.7.11 ouvertes), P.6.10,
P.6.9 (triage), P.6.9.1 et P.6.9.2 closes. P.6.9.3 à P.6.9.12 restent, dans l'ordre du
triage ; P.6.8 et P.6.11 aussi. Branche de travail : `ai/P-schematic-fidelity`,
PR #10 vers `agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.9.2 — une zone écrit sa référence de net dans la forme du board qu'elle
modifie. `konnect_sexp::net` gagne `NetRef`, `net_ref_for_write` et
`NetRef::zone_tokens`, qui réutilisent le discriminant par forme de P.6.5 :
lecture et écriture ne peuvent plus diverger. Les deux copies privées de
`find_net_id` (résolution par offset de chaîne) sont supprimées.

Correction d'une hypothèse du triage, mesurée avant d'écrire : `(layer …)` vs
`(layers …)` n'est **pas** une différence de forme de fichier mais de
cardinalité (voir D121). Ce qui diffère, c'est le nœud net, et de plus que
l'id.

Le refus est cadré : un board legacy qui ne déclare pas le net est refusé en
nommant `add_net`, fichier vérifié byte-identique ; un board sans table ne
déclare rien, donc un nom inconnu s'écrit tel quel et KiCad crée le net au
chargement. Les deux sens sont testés. `add_zone` ne rapporte `net_id` que
lorsqu'il en existe un.

Nouvelle fixture `kicad10_no_net_table.kicad_pcb` (20260206, nets nommés sur
les pads), vérifiée chargeable par kicad-cli 10.0.3 — 0 erreur, 0 non connecté
— avant de servir d'oracle.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, **51 suites, 1240
  tests, 0 échec**
- sondes live `cli_tools` avec `KICAD_CLI` : PASS, **11/11**, dont la nouvelle
  (`a_pour_on_a_kicad_10_board_still_loads_in_kicad_cli`), ajoutée au job E2E
- rouge d'abord : `net_ref_for_write` neutralisé au comportement zéro-ou-id
  fait échouer trois des quatre tests d'intégration
- borne énoncée : la sonde live prouve la validité du fichier, pas
  l'attachement électrique — le DRC ne peut pas le montrer sur un net à une
  seule pastille ; la preuve octet par octet vit dans les tests unitaires

## Décisions actives

- **D121** — dans un `(zone …)`, `(layer "X")` et `(layers "A" "B")` ne
  distinguent pas les versions de fichier mais la **cardinalité** : mesuré sur
  les démos, `vme-wren` (20241229) écrit les deux formes, et `CM5_MINIMA_3`
  (20250513) aussi. Ce qui distingue les formes, c'est le nœud net :
  `pic_programmer` (20260206) écrit `(zone (net "GND") …)` **sans**
  `net_name`, tandis que 20250907 et antérieurs écrivent `(net <id>)` avec le
  nom dans un `(net_name …)` frère — et non `(net <id> "<nom>")`, qui est la
  forme des pads. Le message de commit amont affirmait le contraire sur les
  layers ; la mesure locale prime.
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
- `crates/konnect-sexp/src/net.rs` — lecture **et** écriture des nets par forme
  (`NetRef`, `net_ref_for_write`, `zone_tokens`) ; `tools/mod.rs::zone_net_ref`
  est le point d'entrée des deux handlers de zone.
- `crates/konnect-core/tests/fixtures/kicad10_no_net_table.kicad_pcb` — board
  20260206 sans table de nets, chargeable par kicad-cli.
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

Implémenter P.6.9.3 — `e7b0c54` : sur une feuille fille, le bloc
`(instances (project "NOM" (path "/…")))` d'un symbole est aujourd'hui
construit à partir du fichier où l'on écrit — `project_name_for`
(`tools/mod.rs:452`) rend le stem du fichier et `ensure_root_uuid` (`:497`)
son propre uuid, utilisé comme chemin entier. Juste sur une racine, faux sur
une fille : KiCad ne fait correspondre ni le projet ni le chemin, et tout
symbole placé là se lit comme non annoté pendant que l'outil rapporte un
succès. Sites d'écriture : `sch_components.rs:492`, `sch_batch.rs:468`,
`sch_wiring.rs:1754`. Résoudre la vraie place de la feuille : `.kicad_pro` le
plus proche pour le nom du projet, sa racine `.kicad_sch` sœur pour l'uuid de
tête, puis une descente bornée en profondeur depuis cette racine qui enregistre
l'uuid de chaque `(sheet …)` traversé, d'où `"/<root>/<sheet>[/<sheet>…]"`.
`owning_project_root` (`sch_export.rs:582`, P.6.7.8) fait déjà la moitié
« trouver le projet », mais ne regarde que le répertoire du fichier : élargir
cette borne seulement si la mesure l'exige, et le dire. Tout ce qui ne se
résout pas retombe sur le comportement autonome actuel, qui doit rester testé.
Oracle disponible en local : la démo `complex_hierarchy` de KiCad 10, dont
`ampli_ht.kicad_sch` est une fille non nommée d'après le projet.
