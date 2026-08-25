# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7 est
ouverte (les huit items d'origine clos, P.6.7.9 à P.6.7.11 ouvertes), P.6.10 et
P.6.9 (triage) closes. P.6.9.1 à P.6.9.12 sont les tâches issues du triage ;
P.6.8 et P.6.11 restent. Branche de travail : `ai/P-schematic-fidelity`,
PR #10 vers `agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.9 — triage des 16 correctifs upstream partis directement sur `main`
(annexe A de `docs/upstream-audit.md`), par la méthode de P.4 : diff amont, puis
localisation du mécanisme dans ce fork par `rg`, verdict avec citation
`file:line`. Résultat : **8 BACKPORT NOW, 4 LATER, 4 NOT APPLICABLE**, plus un
ordre d'implémentation par conséquence. Les tâches correspondantes sont ouvertes
en `P.6.9.1` … `P.6.9.12` dans `plan.md`.

Mesures qui portent le classement :
- `ff518c8` est **pire ici qu'en amont** : la table de `layer_from_name`
  (`konnect-ipc/src/builders.rs:42-61`) est plus courte que celle qu'upstream a
  corrigée, et les chemins graphic/text/footprint-instance envoient
  `BL_UNDEFINED` là où le chemin pad le retire. `pcb_components.rs:353` lit les
  graphics du vrai `.kicad_mod`, donc un footprint de librairie officielle avec
  un enfant `Dwgs.User` fait planter l'éditeur ouvert.
- Trois items sont **moins chers ici** parce que P.6 a déjà posé leur moitié
  dure : `f2372ca` sur `konnect_sexp::net` (P.6.5), `977f0c5` sur `DrcReport`
  (P.6.1), `de70351`/`8591707` sur `update_field`/`insert_property`.
- Quatre sont **sans objet parce que le mécanisme est absent**, pas parce qu'il
  serait mineur : aucun chemin de sync (`2904841`, `59d0ead`), aucun
  `.ancestors()` dans l'arbre (`ec705c3`), aucun bloc de couverture de board
  (`d5774b3`, dont le piège `find_all` a été balayé et ne se retrouve nulle
  part ailleurs en production).

Validation : documentaire, aucun code touché. Chaque verdict est ancré par une
citation vérifiée dans ce fork ; les numéros de ligne ont été relus après
rédaction (`writer.rs:104`, `sch_components.rs:795`/`:825`/`:860`,
`pcb_components.rs:1494`).

## Décisions actives

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
  mesuré pendant P.6.9 : `konnect-ipc/src/client.rs:1294` développe `*.Cu` en
  `3..=34`, même hypothèse d'intervalle fixe, dans la fonction que P.6.9.1
  touche.
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
  peut pas les voir. C'est ce qui a motivé P.6.9, désormais close.
- **D109** — aucun contenu portant des backslashes ne doit transiter par une
  heredoc : celle de ce shell les mange, même en `<<'EOF'`, et une heredoc
  Python les relit comme échappements, en silence. Tout contenu de ce genre
  passe par Write/Edit.
- Les décisions V1 antérieures (INV6, D97…D101) restent actives. D103, D105 et
  D106 sont résolues et retirées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `docs/upstream-audit.md` — annexe A : le triage P.6.9, avec pour chaque item
  mécanisme amont, état dans ce fork (`file:line`), impact et coût.
- `crates/konnect-ipc/src/builders.rs` — `layer_from_name` l.42-61, cible de
  P.6.9.1 ; `crates/konnect-ipc/src/client.rs` — `build_graphic_child` l.1561,
  chemin footprint l.1230-1400.
- `crates/konnect-core/src/tools/pcb_routing.rs` + `pcb_board.rs` — les deux
  `find_net_id` et le template de zone, cibles de P.6.9.2.
- `crates/konnect-sexp/src/net.rs` — lecture des nets par forme (P.6.5), base
  du write-side de P.6.9.2.
- `crates/konnect-core/src/tools/mod.rs` — `project_name_for` l.452,
  `ensure_root_uuid` l.497, `require_str`/`require_f64`/`get_path` l.414-447.
- `crates/konnect-schematic-editor/src/sexp/writer.rs` — les trois causes du
  reformatage (P.6.9.4).
- `crates/konnect-core/src/tools/cli.rs` — `DrcReport`, base de P.6.9.8.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.9.1 — `ff518c8` : `layer_from_name`
(`crates/konnect-ipc/src/builders.rs:42-61`) doit couvrir tout layer sur lequel
un footprint KiCad 10 peut légalement dessiner, par calcul et non par liste
(attention : `BL_Rescue = 62` s'intercale entre `BL_User_9 = 61` et
`BL_User_10 = 63`), et un `try_layer_from_name` doit refuser un nom sans
représentation **avant** qu'un seul enfant ne soit construit — root, pads,
graphics et textes. Rouge d'abord : un test qui montre qu'un enfant `Dwgs.User`
part aujourd'hui en `BL_UNDEFINED` par `build_graphic_child`
(`client.rs:1561`). Le chemin de mesure amont (KiCAD qui faute) n'est pas
reproductible sans session GUI ; l'assertion porte donc sur ce qui est envoyé,
et la limite doit être écrite dans la tâche.
