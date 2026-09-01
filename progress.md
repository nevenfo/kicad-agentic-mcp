# PROGRESS

## Phase actuelle

**W — v1.1.4, les trois limitations Pareto.** W.1 à W.4 terminées et validées
en réel. Reste W.5, la publication.

## Tâche actuelle

W.5.1 — Version, lockfiles, notes de version. **Le périmètre et le numéro
attendent l'utilisateur avant toute modification.**

## Dernière tâche validée

**W.4 — Régression globale.** `gate.ps1` complet vert (fmt, clippy, tests,
doctests, build release). Les trois suites live vertes : routage
`DocumentType` au schématique, IPC PCB, refus du verrou éditeur de W.1. ERC
Hi-Fi inchangé, 0 erreur et 15 avertissements, mesuré de part et d'autre de
l'édition B2.8. `live-pcb-e2e.ps1` a cessé d'écrire dans le profil KiCad de
l'utilisateur : il en copie un pour lui-même sous son répertoire de travail,
comme son jumeau schématique — c'est ce qui a levé le `AS_NOT_READY` causé par
l'assistant de premier démarrage resté sans réponse.

**W.3 — Attributs natifs `on_board`, `in_bom`, `dnp`, et suppression d'une
propriété.**

Lecture : `get_schematic_component` et `list_schematic_components` rendent les
trois attributs, toujours — un tag absent est le défaut de KiCad, pas un champ
indéterminé. Écriture : `edit_schematic_component` et
`batch_edit_schematic_components` les acceptent en booléens, par
`set_symbol_attribute` et non par le chemin des propriétés ; un tag manquant
est inséré là où eeschema l'écrit (après `on_board`, avant `uuid`), à
l'indentation du fichier ; un appel adressé par référence atteint toutes les
unités d'un symbole multi-unités ; une valeur non booléenne est refusée.

W.3.5, découverte en préparant W.3.4 : `fields: {"clé": null}` supprime la
propriété. Le bloc `(property …)` entier disparaît, ses propres lignes avec —
une propriété écrite par eeschema en fait huit — de sorte que la suppression
rend le document tel qu'il était avant que la propriété existe. `Reference` et
`Value` sont refusées, KiCad les exige ; supprimer une propriété absente est
signalé, pas maquillé en changement.

W.3.4 : B2.8 du benchmark Hi-Fi levé par le MCP seul, en un appel, `RV1`
adressé par `uuid`. Le projet Hi-Fi est modifié et commité (`a55870a` sur
`main`, dépôt propre avant et après) : `(on_board yes)` → `(on_board no)` et
les dix lignes de `exclude_from_board` supprimées, rien d'autre dans le
fichier. ERC identique avant/après, 0 erreur et 15 avertissements, comme à la
porte C2.

Validation : 13 tests d'intégration neufs (`crates/konnect-core/tests/
symbol_attributes.rs`), `scripts/live-b28-on-board.ps1` vert sur le projet réel
et sur copie, `fmt`, `clippy -D warnings`, suite complète workspace verte,
`docs/capability-matrix.md` régénéré.

## Décisions actives

- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Sonde
  réelle : `~<nom>.kicad_sch.lck` et `~<projet>.kicad_pro.lck` apparaissent à
  l'ouverture d'Eeschema, contenu `{"hostname":…,"username":…}`, 50 octets,
  **sans PID ni horodatage** ; une fermeture propre les retire. La fraîcheur
  n'étant pas décidable, elle n'est pas décidée : présence vaut refus.
- Le garde ne vise que `.kicad_sch`. Le `.kicad_pcb` passe par l'IPC, et le
  lock `.kicad_pro` n'est pas celui du document muté.
- `v1.1.3` reste la version installée. Rollback unique et explicite :
  `C:\Users\FlowUP\Documents\KiCad\10.0\konnect-plugin-backups\com_github_mixelpixx_konnect.rollback-v1.1.2-20260831`.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le
  profil réel de l'utilisateur.
- `set_footprint_graphics` est une API typée par primitive, pas un éditeur de
  texte `.kicad_mod` : une couche par appel, tout le reste reporté tel quel.
- Le repère de broche 1 ne se devine pas : c'est une déclaration du client,
  `true` par défaut, l'oubli sur une pièce polarisée étant l'erreur coûteuse.
- L'alignement du courtyard sur la grille KLC se fait vers l'extérieur.
- Les trois attributs natifs sont des tags du bloc symbole, jamais des
  propriétés : `(property "dnp" "yes")` s'affiche dans la liste des champs et
  ne change ni le netlist, ni la BOM, ni « Update PCB from schematic ».
- `null` dans `fields` signifie désormais suppression, non plus « valeur sans
  forme textuelle ». Le test unitaire `numeric_and_boolean_field_values_are_
  stored_as_text` nomme maintenant un objet comme valeur irreprésentable.
- Les empreintes Hi-Fi défectueuses sont corrigées et prouvées **sur copie**
  (`scripts/live-footprint-fix.ps1`). L'application in-place dans
  `HifiAmp_TPA3255_Local.pretty\` n'a pas été faite : elle relève de D1.8 du
  plan Hi-Fi et attend l'utilisateur.

## Blocage actif

Aucun.

## Observation hors périmètre, non corrigée

`ToolErrorKind::from_anyhow` ne reconnaît pas `konnect_sexp::SexpError::Conflict`
nu. Les outils qui appellent `write_atomic_if_unchanged` directement dégradent
donc une course GUI en `handler_error` au lieu de `conflict`. Défaut
préexistant, orthogonal à W.1, laissé tel quel volontairement.

## Fichiers / zones utiles

- `crates/konnect-core/src/tools/mod.rs` (`set_symbol_attribute`,
  `set_symbol_attribute_on_all_units`, `remove_symbol_property`,
  `remove_symbol_property_on_all_units`, `SYMBOL_CHILD_ORDER`)
- `crates/konnect-core/src/tools/sch_components.rs`
  (`handle_edit_schematic_component`, `set_attribute`, `remove_field`),
  `crates/konnect-core/src/tools/sch_batch.rs` (`handle_batch_edit`)
- `crates/konnect-core/tests/symbol_attributes.rs`
- `crates/konnect-core/src/tools/footprint_graphics.rs`,
  `crates/konnect-core/src/tools/library.rs`
- `gate.ps1` (racine), `scripts/live-editor-lock.ps1`,
  `scripts/live-schematic-e2e.ps1`, `scripts/live-pcb-e2e.ps1`,
  `scripts/live-footprint-fix.ps1`, `scripts/live-b28-on-board.ps1`
- `kicad-cli` : `%LOCALAPPDATA%\Programs\KiCad\10.0\bin\kicad-cli.exe`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Projet Hi-Fi : `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi`
  (dépôt git propre, dernier commit `a55870a`)

## Préconditions de tout test live

1. Un seul répertoire sous `3rdparty` par identifiant de plugin. Trois copies
   de `com_github_mixelpixx_konnect` tuent l'éditeur 3 s après démarrage
   (`0xC0000005`, `wxbase332u_vc_x64_custom.dll`), pipe publié puis perdu.
   Identique en `10.0.3` et `10.0.6` : ce n'est pas une régression de version.
2. Aucune autre instance KiCad ne détient le socket d'API — sinon les requêtes
   partent au mauvais éditeur et reviennent en « does not handle … for this
   document type ».
3. Aucun dialogue modal : l'assistant `Configuration de KiCad` et l'avis de
   format de fichier ancien font répondre `AS_NOT_READY` sur un pipe présent.
4. Les toolsets sont opt-in : un client charge `load_toolset` avant d'appeler
   (`sch_components`, `library`, …), sinon chaque outil répond
   `toolset_not_loaded` et une assertion de refus passe pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir : la fermeture propre se
   retente.

## NEXT ACTION

W.5.1 — Faire valider par l'utilisateur le périmètre de la publication et le
numéro `v1.1.4`, puis seulement ensuite : versions des manifestes et lockfiles,
`RELEASE_NOTES.md`, CI verte sur le commit candidat, tag et artefacts (W.5.2).
