# PROGRESS

## Phase actuelle

**P — Schematic round-trip fidelity.** P.1 à P.6 sont closes et leurs preuves
inchangées. P.7 a rouvert la phase : la CI sur le commit exact de la clôture
(`8aeaff7`) était **rouge sur les trois OS** alors que la suite locale était
verte. Branche `ai/P-schematic-fidelity`, PR #10 vers `agentic/main`,
non fusionnée.

## Tâche actuelle

**P.7** — implémentée et verte localement dans les deux environnements ; en
attente de la seule preuve qui manque, un run CI vert sur le commit poussé.

## Dernière tâche validée

**P.6.8.9** — le snap de grille ne déplace plus un point qui **est** une pin
(D139). Trois items P.7 sont écrits et prouvés localement, pas encore cochés :

- **P.7.1** — `a_component_placed_on_a_child_sheet_is_written_with_the_roots_path`
  plaçait `Device:R` dans un enfant issu de `blank_schematic_template()`, dont
  le `lib_symbols` est vide : `ensure_lib_symbol` devait donc résoudre l'id
  dans les librairies **installées**, présentes ici et sur aucun runner CI.
  L'outil refusait, n'écrivait rien, et l'assertion lisait ensuite un fichier
  jamais touché. L'enfant est désormais `harness::TWO_RESISTORS`, qui embarque
  `Device:R` — la réponse que le dépôt donnait déjà à cette question.
- **P.7.2** — le harness laissait ce silence passer : `Harness::json`
  documentait « Panics if the tool errored » et ne testait que `Result::Err`,
  alors qu'un handler qui refuse rend ici `Ok(CallToolResult { is_error: true })`.
  Il assertit maintenant `!is_error` et affiche le corps du refus.
- **P.7.3** — `ci.yml` lançait `cargo test` sans `--no-fail-fast` : le run
  s'arrêtait au premier binaire rouge et ne disait rien des onze suivants.

Validation :
- portée **mesurée**, pas supposée : suite complète avec `ProgramFiles`,
  `ProgramFiles(x86)`, `LOCALAPPDATA`, `APPDATA` et les trois
  `KICAD<major>_SYMBOL_DIR` pointés sur un dossier vide — toutes les racines
  que `kicad_paths::share_roots` connaît sous Windows. **Un seul** test
  tombait ; la classe est ce test, pas une famille
- `cargo test --workspace --locked --lib --tests --no-fail-fast`, machine sans
  KiCad simulée : PASS, **57 suites, 1385 tests, 0 échec**
- même commande en environnement normal (KiCad 10.0.3) : PASS, mêmes comptes
- `cargo fmt --all -- --check` / `cargo clippy --workspace --locked
  --all-targets -- -D warnings` : PASS, 0

## Décisions actives

- **D140** — la suite locale et la suite CI ne mesurent pas la même machine, et
  l'écart est une **install KiCad**. Un test qui place un `lib_id` sans
  l'embarquer dans son fixture affirme une propriété de la machine en croyant
  affirmer une propriété du code : il passe ici et ne peut pas passer là-bas.
  Règle : tout test non `#[ignore]` qui place un symbole part d'un fixture qui
  porte son `lib_symbols`. Corollaire de méthode, celui qui a fermé la classe :
  la mesure se fait en pointant **toutes** les racines de
  `kicad_paths::share_roots` vers un dossier vide, pas seulement les
  `KICAD<major>_SYMBOL_DIR` — `library_dirs` ajoute les racines bundlées après
  les variables d'environnement, donc les variables seules ne simulent rien.
  Même famille que D113 et P.6.9.20, à l'échelle de la CI.

- **D139** — le snap de grille est une correction pour un appelant approximatif,
  pas une vérité sur le fichier : une pin réelle n'a pas besoin d'être corrigée,
  et 7,6 % des pins du corpus (3 670 sur 48 068) ne survivent pas à la
  correction. Règle : un outil qui écrit un **point de connexion** (fil,
  jonction, no-connect, stub) laisse le point tel quel quand une pin placée s'y
  trouve, et ne snappe que ce qui n'en est pas une ; un outil qui positionne un
  **symbole** continue de snapper, son ancre devant rester sur la grille (E6).

- **D138** — le corpus de démos n'est pas un oracle direct du placement d'un
  champ : sur une feuille finie, un champ a souvent été déplacé à la main, donc
  aucune règle ne peut en reproduire plus d'une fraction. Mesuré : la meilleure
  règle en reproduit 41,4 %, et c'est **normal**, pas un échec. Ce qui décide
  entre deux règles est la comparaison **relative**, et sur les sous-ensembles
  où elles divergent. Corollaire : les champs reproduits exactement sont ceux
  que personne n'a bougés, et eux seuls forment un oracle propre pour les
  propriétés voisines.

- **D137** — `kicad-cli` n'est **pas** un oracle de la table `(layers …)`.
  Mesuré sur 10.0.3 : id non canonique, id dupliqué et même nom déclaré deux
  fois se chargent tous, `pcb drc` réussit et les gerbers sont identiques ; le
  loader clé par **nom**. Conséquence d'un id faux : elle vit dans ce serveur
  (`layers::copper()` compte par nom). Les deux numérotations sont dérivées :
  la **legacy** vaut l'enum `BoardLayer` du proto **moins 3**, la **moderne**
  est mesurée sur les 18 boards de démo — `In<n>.Cu` = `2n+2`, `User.<n>` =
  `37+2n`. `Rescue` n'a pas d'ordinal moderne mesuré et est refusé.

- **D136** — une garde dupliquée diverge, et le fait en silence. La règle : une
  garde de sécurité a **une** définition, et un test lexical interdit le chemin
  qui la contourne (`no_ipc_call_bypasses_the_guarded_macro`). Étendu par
  P.6.8.5 au-delà des gardes : `placed_pins` et `all_pin_endpoints` étaient la
  même boucle dans deux fichiers, et une seule des deux savait ce que l'autre
  avait appris.

- **D135** — un scan statique prouve qu'une clé est **lue**, jamais qu'elle est
  **honorée**. Pour une clé qui désigne une **cible**, la lecture ne prouve
  rien ; seul un test qui interdit le chemin non gardé ferme la classe.

- **D134** — `required` ne peut pas dire « l'un ou l'autre » : la forme retenue
  est `"required": []` plus `"anyOf"`. Conséquence : `anyOf` n'est **pas**
  appliqué par `first_missing_required`, donc un tool disjonctif garde sa garde
  dans le handler ; c'est la seule qui protège une entrée de batch (D131).

- **D133** — le scanner de couverture produit aussi des **faux positifs** : il
  reconnaît `"<tool>"` n'importe où dans une source de test, liste d'exclusion
  comprise. Règle : un nom de tool cité dans un test qui ne l'appelle pas doit
  être cassé lexicalement, avec le commentaire qui dit pourquoi.

- **D132** — *corrigée par la mesure de P.6.9.19.* Le premier critère du
  scanner (`"<tool>"`) suffit, et la convention de test du dépôt le déclenche
  toujours — `tests/harness/mod.rs` passe par `ToolRouter` par nom. Couverture
  cachée mesurée : **zéro**. Tant que cette convention tient, un `NOT_TESTED`
  se lit littéralement.

- **D131** — la validation `required` du dispatch ne couvre pas les entrées de
  `kicad_invoke` : la passerelle appelle `(def.handler)(…)` directement. Les
  helpers `require_*` et `get_path` sont donc la **seule** validation
  d'argument que voit une entrée de batch.

- **D130** — l'ordre des refus au dispatch est : mode gate d'abord, validation
  des arguments ensuite. Un appelant `ReadOnly` ne reçoit pas de coaching
  d'arguments sur un appel qui serait refusé de toute façon.

- **D129** — une heuristique s'efface devant la mesure qu'elle approximait :
  `net_count > 3 && track_count == 0` n'est conservée que quand la connectivité
  n'a pas été mesurée (`!gate.connectivity_measured`).

- **D128** — `docs/capability-matrix.md` est généré, et son scanner conserve la
  source de preuve **lexicographiquement la plus petite**. Ajouter un test
  unitaire pour un tool prouvé par un test d'intégration déplace sa ligne
  d'evidence et rend `the_committed_matrix_is_up_to_date` rouge. Régénérer avec
  `KAM_UPDATE_MATRIX=1`.

- **D127** — la liste `required` du schéma d'un tool est l'autorité sur ce qu'un
  handler doit exiger, pas l'intuition du risque. P.6.9.16 renverse la charge :
  quatre fois sur cinq, c'est le **schéma** qui avait tort.

- **D126** — une garde « ne rien changer est un échec » qui exige *aussi* une
  erreur ne garde rien. La condition correcte est « rien n'a changé »,
  l'`errors` ne servant qu'à rédiger le motif.

- **D124** — `Reference` est la **seule** propriété réservée d'un symbole : la
  seule stockée deux fois, dans la propriété **et** dans `(instances …)`.
  Écrire `Value`/`Footprint`/`Datasheet` par `add_component_annotation` est
  légitime, et l'audit BOM le fait.

- **D125** — précision des coordonnées de schéma, mesurée sur 126 933 valeurs
  `(at …)` : au plus **4 décimales**, le bruit d'addition binaire apparaissant
  vers la **13e**. Tout code qui calcule une coordonnée par addition arrondit à
  **6 décimales**. Corollaire : une attente calculée par le test se compare à
  0,01 près, jamais par `==`.

- **D123** — les feuilles de démo KiCad 10 sont toutes en **CRLF** ; la fin de
  ligne est un champ propre de `WriteStyle`. Corollaire : toute mesure de
  fidélité octet comparant avec `str::lines()` est **aveugle** à cet axe.

- **D122** — sur `complex_hierarchy`, `kicad-cli sch erc` ne vérifie pas
  l'annotation et le netlist retombe sur la propriété `Reference` : la
  conséquence réelle vit dans eeschema, qu'aucune CLI n'observe. Toute sonde
  live sur ce sujet se limite à « KiCad accepte le fichier ».

- **D121** — dans un `(zone …)`, `(layer …)`/`(layers …)` distinguent la
  **cardinalité**, pas la version. Ce qui distingue les versions, c'est le nœud
  net : 20260206 écrit `(net "<nom>")` sans `net_name`.

- **D120** — l'enum `BoardLayer` de kiapi se nomme comme KiCad : `BL_` + le nom
  avec `.` → `_`. Toute correspondance nom↔layer passe par `from_str_name`.
  `BL_UNKNOWN`/`UNDEFINED`/`UNSELECTED` ne doivent jamais être renvoyés par nom.

- **D119** — ce fork n'a ni `pcb_sync.rs`, ni `apply_footprint_fields`, ni
  `update_pcb_from_schematic` : un item upstream touchant la sync n'a pas de
  site ici. Si une telle voie est ajoutée, reprendre `2904841` puis `59d0ead`.

- **D118** — `kicad-cli sch export bom` (10.0.3) n'a **aucune** option
  `--format`. Toute option annoncée par un schéma de tool se vérifie contre le
  `--help` de la CLI installée ; un diff upstream n'est pas une autorité.

- **D117** — les boards KiCad ≥ `20241229` numérotent le cuivre en **pairs**.
  Tout code qui alloue un id de layer le dérive du nom canonique sous la
  numérotation du board (fait par P.6.11, tables en D137).

- **D116** — un board livré par KiCad 10 peut être réellement malformé
  (`royalblue54L_feather`, racine fermée à l'octet 14735 sur 3,6 Mo). Toute
  conformance doit traiter ce cas comme un échec attendu.

- **D115** — oracle de forme des nets : **20260206** est la bascule (table
  supprimée, `(net "<nom>")` sur chaque item). Se discrimine par `SexpNode::Str`
  contre `SexpNode::Atom` en position 1, jamais par un numéro de version.

- **D114** — `gh` résout par défaut vers l'upstream `mixelpixx/Konnect`. Toute
  commande `gh` visant notre travail porte `-R nevenfo/kicad-agentic-mcp`.

- **D113** — un test qui peut se sauter doit rendre son silence visible. Un
  `KICAD_DEMOS`/`KICAD_FOOTPRINTS` explicite mais introuvable échoue, et les
  comptes sont assertés. P.6.9.20 et D140 sont le même défaut sous d'autres
  angles.

- **D111** — `tests/fixtures/test.kicad_pcb` est un fichier KiCad 8 que KiCad 10
  refuse de charger. Toute preuve passant par `kicad-cli` utilise
  `unrouted.kicad_pcb` ou `harness::BLANK_BOARD`.

- **D112** / **D110** — mesures d'oracle : un bloc `(netclass …)` dans le board
  fait sortir `kicad-cli` en code 3 ; un vrai `.kicad_pro` nomme le champ
  `track_width`. Sur une carte non routée, `pcb drc --format json` écrit ses
  erreurs sous `unconnected_items`, sans `pos` au niveau violation.

- **D102 / D104 / D107 / D108 / D109** — ancres upstream vérifiées (`#144` =
  `8dd54e8`, `#209` = `1d31ad4`, baseline `5cd6454`) ; oracle KiCad `kicad-cli`
  10.0.3 local et 10.0.5 dans `e2e-kicad.yml` ; P.4 s'est arrêtée à la
  classification ; deux correctifs graves ont atterri directement sur
  `upstream/main`, invisibles à un `--merges` ; aucun contenu portant des
  backslashes ne transite par une heredoc.

- Les décisions V1 antérieures (INV6, D97…D101) restent actives.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-core/tests/harness/mod.rs` — `Harness::json`, qui assertit
  désormais `!is_error` (P.7.2) ; `TWO_RESISTORS` et les autres fixtures qui
  embarquent leur `lib_symbols`.
- `crates/konnect-core/tests/sheet_instances.rs` — `child_with_device_r`
  (P.7.1) ; `blank_child` reste pour les trois tests qui ne placent rien.
- `crates/konnect-schematic-editor/src/kicad_paths.rs` — `share_roots` /
  `library_dirs`, la liste exacte à neutraliser pour simuler une machine sans
  KiCad (D140).
- `.github/workflows/ci.yml` — le step `Test (unit + integration)`, désormais
  `--no-fail-fast` (P.7.3).
- `docs/upstream-audit.md` — annexe A : le triage P.6.9.
- `crates/konnect-core/src/tools/mod.rs` — `placed_pins`/`PlacedPin`,
  `all_pin_endpoints`, `pin_outward_at`, `sheet_instance_context`,
  `instance_targets`, `zone_net_ref`, `require_str`/`require_f64`/`get_path`,
  `first_missing_required` (qui n'applique **pas** `anyOf`, D134).
- `crates/konnect-core/src/tools/sch_wiring.rs` — les six sites de
  `snap_unless_pin` et `pin_endpoints_or_empty` (P.6.8.9).
- `crates/konnect-schematic-editor/src/library.rs` — `field_anchor`,
  `ensure_lib_symbol`, `find_symbol_dirs`.
- `crates/konnect-schematic-editor/src/sexp/writer.rs` — `WriteStyle`,
  `write` / `write_styled` ; `schematic/mod.rs::sniff_write_style`.
- `crates/konnect-sexp/src/net.rs` — lecture et écriture des nets par forme.
- `crates/konnect-ipc/src/builders.rs` / `client.rs` — `try_layer_from_name`,
  `check_layer`, `build_footprint_item`.
- `.github/workflows/e2e-kicad.yml` — job gatant, un step par sonde.

## NEXT ACTION

Attendre le run CI du commit P.7 poussé sur `ai/P-schematic-fidelity` et
vérifier que **Check & Test** passe au vert sur les trois OS. Si vert : cocher
P.7.1, P.7.2 et P.7.3 dans `plan.md`, puis demander à l'utilisateur la
décision restée ouverte — fusionner la PR #10 vers `agentic/main` ou ouvrir
une phase suivante. Si rouge : lire le nouveau log, qui nomme maintenant tous
les binaires en échec (P.7.3), et traiter le premier écart mesuré.
