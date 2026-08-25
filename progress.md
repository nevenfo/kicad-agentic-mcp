# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.5 closes le 2026-08-24. P.6
(backlog de correctness upstream) est ouverte : P.6.1 à P.6.6 closes, P.6.7 est
ouverte (les huit items d'origine, P.6.7.9 et P.6.7.10 clos, P.6.7.11
ouverte), P.6.10,
P.6.9 (triage) et P.6.9.1 à P.6.9.23 closes — **tout le triage d'origine et
toutes ses découvertes**. Restent P.6.7.11, P.6.8 et P.6.11.
Branche de travail : `ai/P-schematic-fidelity`, PR #10 vers `agentic/main`.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

P.6.7.10 — `export_bom` n'exposait ni `--fields`, ni `--labels`, ni
`--group-by`. Décision prise avant d'implémenter : **exposer les trois**. Sans
`--fields`, le BOM sort avec le défaut de KiCad
`Reference,Value,Footprint,QUANTITY,DNP` — ni colonne MPN ni colonne LCSC, donc
un BOM qu'on ne peut pas commander, sur un serveur qui porte tout un toolset de
sourcing. Forme reprise de `--layers` (P.6.7.7) : tableaux de chaînes côté
schéma, joints en une valeur séparée par virgules ; un champ omis ne pousse
aucun flag, donc la CLI applique **ses** défauts.

Trois comportements mesurés sur 10.0.3 avant tout code, chacun décidant si une
garde était due — et la réponse est asymétrique :
- `--fields` plus long que `--labels` **ne décale pas** les colonnes (l'en-tête
  reprend le nom du champ) et l'inverse ignore le surplus → **aucune garde** ;
- `--labels` sans `--fields` s'applique aux premiers champs par défaut →
  **aucune garde** ;
- `--group-by` nommant un champ absent des champs exportés est accepté, sort 0
  et **ne groupe rien** en silence → **garde**, refusée avant tout spawn de
  kicad-cli, en `invalid_argument` sur `group_by`, avec normalisation des
  délimiteurs `${}`.

Garder les deux premiers aurait été poser une garde qui ne garde rien (D126).

Validation :
- rouge d'abord : moitié unitaire par `BomOptions` refusant de compiler contre
  le nouveau vecteur d'arguments, moitié garde en la désactivant
- oracle honnête (`#[ignore]`, comme P.6.7.6) : copie temporaire de la fixture
  avec un champ `MPN` sur R1, exportée avec une étiquette personnalisée, la
  colonne vérifiée dans le CSV réellement écrit par KiCad. **Lancé ici contre
  10.0.3 : PASS**
- `cargo test --workspace --locked --lib --tests --no-fail-fast` : PASS,
  **56 suites, 1345 tests, 0 échec**
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `the_committed_matrix_is_up_to_date` : PASS, matrice intacte

`BomOptions` perd `Copy` (il porte des `Vec<String>`), sans appelant concerné ;
`manufacturing.rs` garde son comportement par `default()`.

## Décisions actives

- **D136** — une garde dupliquée diverge, et le fait en silence. `pcb_routing.rs`
  et `pcb_components.rs` portaient chacun un `ipc!` ; l'un vérifiait que KiCAD
  tient bien le board nommé, l'autre non, et rien ne pouvait le signaler
  puisque les deux compilaient et portaient le même nom au site d'appel. La
  règle : une garde de sécurité a **une** définition, et un test lexical
  interdit le chemin qui la contourne (`no_ipc_call_bypasses_the_guarded_macro`).
  Le doc de `ipc_boundary.rs` énonçait déjà ce principe pour `with_ipc` — la
  garde de board avait simplement été écrite ailleurs.

- **D135** — un scan statique prouve qu'une clé est **lue**, jamais qu'elle est
  **honorée**. `route_pad_to_pad` lit `board` par `get_path` pour trouver ses
  pads dans le fichier, donc `required_schema_static_honesty` le blanchit — et
  il routait ensuite par IPC sans vérifier que KiCAD tient ce board : lire les
  pads de A, graver sur B. Corollaire de méthode : pour une clé qui désigne une
  **cible**, la lecture ne prouve rien ; seul un test qui interdit le chemin
  non gardé ferme la classe.

- **D134** — `required` ne peut pas dire « l'un ou l'autre », et deux tools en
  ont besoin : `get_datasheet_url` (`mpn` ou `lcsc_id`) et `run_design_review`
  (`schematic` ou `board`). La forme retenue est `"required": []` plus
  `"anyOf": [{"required":[a]}, {"required":[b]}]` — le contrat est publié, et
  le dispatch cesse de refuser un appel légitime. Conséquence : `anyOf` n'est
  **pas** appliqué par `first_missing_required`, donc un tool disjonctif garde
  sa garde dans le handler ; c'est la seule qui protège une entrée de batch
  (D131).

- **D133** — amende D132. Le scanner de couverture produit aussi des **faux
  positifs** : il reconnaît `"<tool>"` n'importe où dans une source de test, y
  compris dans une liste d'exclusion. Écrire `"save_project"` dans le tableau
  `EXCLUDED` d'un test qui refuse précisément d'appeler ce tool le fait passer
  `NOT_TESTED` → `SUPPORTED` dans la matrice (delta mesuré : +2 tools, fork
  73,7 % → 74,7 %). L'erreur ne va donc pas seulement vers le sous-comptage
  comme D132 l'affirmait. Contournement en place : les trois noms sont
  construits par `concat!` et la matrice n'est **pas** régénérée. Règle : un
  nom de tool cité dans un test qui ne l'appelle pas doit être cassé
  lexicalement, avec le commentaire qui dit pourquoi.

- **D132** — *corrigée par la mesure de P.6.9.19.* Le scanner reconnaît un tool
  par `"<tool>"` **ou** `handle_<tool>` (`capability/coverage.rs:210`), et 24
  des 198 tools ont un handler dont le nom ne correspond pas au leur. D132
  concluait qu'un `NOT_TESTED` ne prouvait donc rien pour ceux-là : c'est faux,
  mesuré. Le **premier** critère suffit, et la convention de test du dépôt le
  déclenche toujours — `tests/harness/mod.rs` passe par `ToolRouter` par nom
  plutôt que d'appeler un handler privé. Couverture cachée mesurée : **zéro**.
  Ce qui reste vrai, en plus étroit : un tool prouvé *uniquement* par un test
  unitaire appelant son handler directement serait invisible. Tant que la
  convention `ToolRouter` tient, un `NOT_TESTED` se lit littéralement.

- **D131** — la validation `required` du dispatch (P.6.9.10) ne couvre pas les
  entrées de `kicad_invoke` : la passerelle est vérifiée en enveloppe seule et
  appelle `(def.handler)(…)` directement. Les helpers `require_*` et
  `get_path`, désormais alignés sur une même forme d'erreur (P.6.9.11), sont
  donc la **seule** validation d'argument que voit une entrée de batch. Toute
  future garde d'argument posée uniquement au dispatch laissera la passerelle
  derrière elle ; c'est la question ouverte de P.6.9.17. Mesuré vivant par
  P.6.9.16 : c'est cette exemption qui rendait atteignable la revue de design
  vide annoncée en succès.

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
  part entière — voir P.6.9.15 pour `spacing_y`. P.6.9.16 renverse la charge :
  quatre fois sur cinq, c'est le **schéma** qui avait tort.
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
  introuvable échoue, et les comptes sont assertés et affichés. P.6.9.20 est le
  même défaut sous un autre angle : un test qui affirme une propriété de la
  machine en croyant affirmer une propriété du harness.
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
- `crates/konnect-core/tests/required_schema_honesty.rs` — la passe P.6.9.16 ;
  ses doc-comments portent ce que la forme ne couvre pas, y compris P.6.9.21.
- `crates/konnect-core/src/mcp/handler.rs:344` — `missing_required_refusal`, le
  gate que la passe contourne ; `tools/mod.rs:425` — `first_missing_required`,
  qui n'applique **pas** `anyOf` (D134).
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

Implémenter P.6.7.11 — la mesure sur laquelle P.6.7.8 repose ne vit que dans un
commentaire. Relire la section dans `plan.md` (`rg -n "P.6.7.11" plan.md`) pour
ce qu'elle demande exactement et ses critères de validation. Contexte utile
déjà établi : D122 borne ce qu'une sonde live peut observer sur ce sujet, et
P.6.7.8 a mesuré 67 violations dont 46 `lib_symbol_issues` sur la sous-feuille
`ampli_ht` de `demos/complex_hierarchy`, contre 0 sur la feuille racine.
