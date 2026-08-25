# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7 est
ouverte (les huit items d'origine clos, P.6.7.9 à P.6.7.11 ouvertes), P.6.10,
P.6.9 (triage) et P.6.9.1 à P.6.9.12 closes (tous les items du triage
d'origine). P.6.9.15 à P.6.9.18, découverts en route, restent, dans
l'ordre du triage ; P.6.8 et P.6.11 aussi. Branche de travail :
`ai/P-schematic-fidelity`, PR #10 vers `agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.9.14 — la voie batch traite `fields` comme la voie mono-composant. La
difficulté n'était pas le diagnostic mais la conciliation de deux modèles
d'écriture : ce handler accumule des `SexpEdit` dont les plages indexent le
contenu **d'origine** et ne sont justes qu'appliquées d'un coup, tandis que
`set_symbol_property` rend un document déjà splicé — il le doit, puisque la
position et l'indentation d'une insertion se lisent sur le symbole tel qu'il
est.

Résolu en deux phases. Phase 1 inchangée : les champs standard restent des
édits d'offsets appliqués en un seul `apply_edits`. Phase 2 déroule les paires
`(champ, texte)` validées — parquées par composant dans un
`PendingProperties` — sur la chaîne résultante, en relocalisant le symbole par
`find_symbol_instance_block` avant chaque écriture, exactement comme
`set_field` sur la voie mono-composant et pour la même raison : une insertion
précédente du même batch a décalé tout ce qui la suit. Tous les offsets de
phase 1 restent valides, rien n'est jamais resérialisé, et une édition d'un
champ reste un diff d'une ligne (P.6.9.4).

`property_text` est passé `pub(crate)` — comme `place_one_component`, seul
autre voisin exporté du fichier — au lieu d'être dupliqué, si bien que les deux
voies refusent les mêmes valeurs. `RESERVED_PROPERTY_KEYS` sert de `reject`,
donc `Reference` est écarté avant toute écriture.

Validation :
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests` : PASS, **54 suites, 1332
  tests, 0 échec** (1326 + 6 nouveaux, aucun test existant modifié)
- rouge d'abord : quatre des six, dont
  `a_batch_edit_writes_a_field_given_as_a_number`
  (`{"errors":[],"updated":[],"updated_count":0}` — succès, rien d'écrit) et
  `a_batch_edit_refuses_to_rewrite_the_reference_property`
  (`changes: ["Reference → R9"]`). Les deux autres étaient verts avant comme
  après et bornent la non-régression, dont le diff d'exactement une ligne

## Décisions actives

- **D131** — la validation `required` du dispatch (P.6.9.10) ne couvre pas les
  entrées de `kicad_invoke` : la passerelle est vérifiée en enveloppe seule et
  appelle `(def.handler)(…)` directement. Les helpers `require_*` et
  `get_path`, désormais alignés sur une même forme d'erreur (P.6.9.11), sont
  donc la **seule** validation d'argument que voit une entrée de batch. Toute
  future garde d'argument posée uniquement au dispatch laissera la passerelle
  derrière elle ; c'est la question ouverte de P.6.9.17.

- **D130** — l'ordre des refus au dispatch est : mode gate d'abord, validation
  des arguments ensuite. Mesuré : placer la validation avant le gate domaine
  fait passer `the_mode_gate_still_answers_first` au rouge et ne change rien
  d'autre. Un appelant `ReadOnly` ne reçoit pas de coaching d'arguments sur un
  appel qui serait refusé de toute façon. Corollaire pour les tools de
  domaine : la vérification vit après `get_tool`, donc après l'auto-load, ce
  qui la rend atteignable pour un toolset pas encore chargé.

- **D129** — l'heuristique de routage `net_count > 3 && track_count == 0` est
  conservée, mais **seulement** quand la connectivité n'a pas été mesurée
  (`!gate.connectivity_measured`). Mesure : dès que `unconnected_items` est
  `Some`, l'heuristique est strictement subsumée — une carte avec des nets et
  zéro cuivre ne peut pas avoir de liste `unconnected_items` vide — donc la
  lancer quand même ne pourrait qu'ajouter un faux positif contredisant une
  mesure. Règle générale : une heuristique s'efface devant la mesure qu'elle
  approximait, elle ne s'y ajoute pas.

- **D127** — la liste `required` du schéma d'un tool est l'autorité sur ce
  qu'un handler doit exiger, pas l'intuition du risque. P.6.9.7 a durci
  exactement ces clés-là : `count_y` porte `"default": 1` et reste optionnel
  bien que voisin de `count_x`, qui est durci. Corollaire, découvert en
  passant : quand schéma et handler divergent sur un défaut, c'est un défaut à
  part entière — voir P.6.9.15 pour `spacing_y`.
- **D128** — `docs/capability-matrix.md` est généré, et son scanner conserve la
  source de preuve **lexicographiquement la plus petite**
  (`capability/coverage.rs:93`). Ajouter un test unitaire dans `src/tools/…`
  pour un tool jusque-là prouvé par un test d'intégration déplace donc sa ligne
  d'evidence, et rend `the_committed_matrix_is_up_to_date` rouge. Régénérer
  avec `KAM_UPDATE_MATRIX=1` ; ce n'est pas une régression de couverture tant
  que le statut ne bouge pas.

- **D126** — une garde « ne rien changer est un échec » qui exige *aussi* une
  erreur ne garde rien : elle ne se déclenche que là où un autre code a déjà
  signalé le problème. Les deux silences qu'elle laissait passer —
  `edit_schematic_component` ignorant `fields`, et un appel sans aucun argument
  éditable — étaient précisément les cas sans erreur. La condition correcte est
  « rien n'a changé », l'`errors` ne servant qu'à rédiger le motif.

- **D124** — `Reference` est la **seule** propriété réservée d'un symbole :
  c'est la seule stockée deux fois, dans la propriété **et** dans
  `(instances …)`, donc la seule qu'une voie générique d'écriture de
  propriété puisse désynchroniser. `Value`/`Footprint`/`Datasheet` ont un
  argument dédié sur `edit_schematic_component` mais aucune seconde copie —
  les écrire par `add_component_annotation` est légitime, et l'audit BOM le
  fait. Une liste à quatre clés casse
  `the_bom_audit_finds_missing_footprints_and_lets_go_when_they_are_assigned` :
  le test avait raison, la liste avait tort.
- **D125** — précision des coordonnées de schéma, mesurée sur 126 933 valeurs
  `(at …)` du corpus de démos : toutes portent au plus **4 décimales**, sauf
  `59.209102362204725`, qui est une conversion depuis les pouces et non du
  bruit. Le bruit d'addition binaire apparaît vers la **13e** décimale. Tout
  code qui calcule une coordonnée par addition — et non par `snap_point`, qui
  arrondit — doit arrondir à **6 décimales** : cela sépare les deux avec de
  la marge des deux côtés et déplace au pire de 0,4 nm, sous la résolution
  interne de 1 nm de KiCAD.
- **D123** — les feuilles de démo KiCad 10 livrées par l'installeur Windows
  sont toutes en **CRLF**. Un writer qui émet du LF y reproduit exactement le
  symptôme que P.6.9.4 corrige — tout le document dans le diff — par un autre
  axe. La fin de ligne est donc un champ propre de `WriteStyle`, pas un détail
  de l'indentation. Corollaire de méthode : toute mesure de fidélité octet
  comparant avec `str::lines()` est **aveugle** à cet axe, puisque `lines()`
  retire le `\r` final ; l'axe EOL exige ses propres tests.
- **D122** — mesuré sur `complex_hierarchy` : avec l'ancienne dérivation
  d'instances (feuille clé sur elle-même), `kicad-cli sch erc` rapporte
  **zéro** violation — il n'exécute pas la vérification d'annotation — et le
  netlist exporté **contient quand même** le symbole, KiCad retombant sur la
  propriété `Reference` faute de chemin d'instance correspondant. La
  conséquence réelle est l'annotation par instance dans eeschema, qu'aucune
  CLI disponible ici n'observe. Toute sonde live sur ce sujet doit donc se
  limiter à « KiCad accepte le fichier » ; la preuve de forme reste dans les
  tests unitaires. L'affirmation contraire venait du message de commit amont.
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
- `crates/konnect-schematic-editor/src/sexp/writer.rs` — `WriteStyle`,
  `IndentStyle`, `write` (fragment, défaut) et `write_styled` (document) ;
  `schematic/mod.rs::sniff_write_style` est le seul point de reniflage.
- `crates/konnect-core/tests/conformance_test.rs` —
  `typed_writer_edit_stays_localized_against_kicad_demo_sheets`, la mesure de
  fidélité ; aveugle aux fins de ligne par construction (D123), que couvrent
  les quatre tests de format en fin de
  `crates/konnect-schematic-editor/tests/integration.rs`.
- `crates/konnect-sexp/src/net.rs` — lecture **et** écriture des nets par forme
  (`NetRef`, `net_ref_for_write`, `zone_tokens`) ; `tools/mod.rs::zone_net_ref`
  est le point d'entrée des deux handlers de zone.
- `crates/konnect-ipc/src/builders.rs` — `try_layer_from_name` / `layer_from_name` ;
  `crates/konnect-ipc/src/client.rs` — `check_layer`, `build_footprint_item`,
  `build_graphic_child`.
- `crates/konnect-core/src/tools/mod.rs` — `sheet_instance_context` et
  `instance_targets` (P.6.9.3), `zone_net_ref` (P.6.9.2),
  `require_str`/`require_f64`/`get_path` l.414-447 (cibles de P.6.9.7).
- `crates/konnect-core/src/tools/cli.rs` — `DrcReport`, base de P.6.9.8.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Implémenter P.6.9.15 — `place_component_array` : son schéma publie
`"spacing_y": { "default": 0 }` (`pcb_components.rs:1030`) alors que le handler
lit `args["spacing_y"].as_f64().unwrap_or(spacing_x)` (`:1511`). Qui croit le
schéma demande une ligne et obtient une grille carrée : toutes les pièces d'un
tableau N×M sont placées au mauvais y. Décider lequel des deux a raison — en
mesurant ce qu'un tableau en ligne demande normalement — puis aligner l'autre,
et couvrir par un test qui place un 3×2 sans `spacing_y` et assère les
coordonnées y. Rouge d'abord.
