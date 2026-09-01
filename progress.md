# PROGRESS

## Phase actuelle

**W — v1.1.4, les trois limitations Pareto.** W.1 terminée et validée en réel.
W.2.1 à W.2.4 terminées et vertes au gate.

## Tâche actuelle

W.2.5 — Rejouer les deux empreintes Hi-Fi défectueuses, sans édition externe.

## Dernière tâche validée

**W.2.1 à W.2.4 — Graphiques d'empreinte, courtyard et repère de broche 1.**

Nouvel outil `set_footprint_graphics` (`crates/konnect-core/src/tools/
footprint_graphics.rs`, toolset `library`) : édition structurée, atomique et
bornée à une couche des primitives `fp_line`, `fp_arc`, `fp_rect`, `fp_circle`
et `fp_poly` d'un `.kicad_mod`, en modes `append`, `replace` et `delete`. Tout
le reste du fichier — pastilles, modèle, propriétés, graphiques des autres
couches — est reporté octet pour octet, et le résultat est reparsé avant
écriture. `get_footprint_info` en est la moitié lecture : il rend désormais les
graphiques dans la forme exacte que `set_footprint_graphics` reprend, filtrable
par `graphics_layer`. Un graphique que le lecteur ne sait pas interpréter est
une erreur, jamais une omission silencieuse.

`create_footprint` corrigé sur deux défauts : le courtyard est maintenant
l'enveloppe **combinée** du corps et des pastilles, élargie de la garde puis
alignée vers l'extérieur sur la grille KLC de 0,01 mm (KLC F5.3, comme
`getFootprintBounds()` de `kicad-library-utils`) — la silkscreen suit la même
enveloppe ; et le repère de broche 1 devient un choix du client
(`pin1_marker`, défaut `true`), son point de silk étant borné pour ne jamais
sortir du courtyard.

Validation : 8 tests d'intégration neufs (`crates/konnect-core/tests/
footprint_graphics.rs`) dont les deux cas Hi-Fi réduits, plus 6 tests unitaires
dans `library.rs`. `fmt`, `clippy -D warnings`, suite complète workspace et
doctests verts ; `docs/capability-matrix.md` régénéré (203 outils).

## Décisions actives

- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Sonde
  réelle : `~<nom>.kicad_sch.lck` et `~<projet>.kicad_pro.lck` apparaissent à
  l'ouverture d'Eeschema, contenu `{"hostname":…,"username":…}`, 50 octets,
  **sans PID ni horodatage** ; une fermeture propre les retire. La fraîcheur
  n'étant pas décidable, elle n'est pas décidée : présence vaut refus, pour un
  lock valide, étranger, vide ou illisible.
- Le garde ne vise que `.kicad_sch`. Le `.kicad_pcb` passe par l'IPC, où
  l'éditeur arbitre son propre document, et le lock `.kicad_pro` n'est pas
  celui du document muté.
- Le refus réutilise le kind `conflict` existant plutôt qu'un nouveau : un
  client qui sait déjà s'arrêter et relire fait exactement ce qu'il faut.
- `v1.1.3` reste la version installée. Rollback unique et explicite :
  `C:\Users\FlowUP\Documents\KiCad\10.0\konnect-plugin-backups\com_github_mixelpixx_konnect.rollback-v1.1.2-20260831`.
- Le contexte de document indéterminé échoue explicitement, jamais PCB.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le
  profil réel de l'utilisateur.
- Le projet Hi-Fi est intact ; B1.3 n'a pas été reprise.
- `set_footprint_graphics` est une API typée par primitive, pas un éditeur de
  texte `.kicad_mod` : une couche par appel, tout le reste reporté tel quel.
  Un éditeur générique aurait un rayon de dégât illimité et ne dirait rien de
  son intention.
- Le repère de broche 1 ne se devine pas : les pastilles d'un fusible sont
  numérotées « 1 » et « 2 » comme celles d'une diode. C'est une déclaration du
  client, avec `true` par défaut, parce que l'oubli du repère sur une pièce
  polarisée est l'erreur coûteuse.
- L'alignement du courtyard sur la grille KLC se fait vers l'extérieur, alors
  que le vérificateur de KiCad aligne au plus proche : rendre jusqu'à un demi
  pas de la garde qu'on vient d'ajouter serait exactement le défaut corrigé.

## Blocage actif

Aucun.

## Observation hors périmètre, non corrigée

`ToolErrorKind::from_anyhow` ne reconnaît pas `konnect_sexp::SexpError::Conflict`
nu. Les outils qui appellent `write_atomic_if_unchanged` directement, sans
passer par `konnect-schematic-editor`, dégradent donc une course GUI en
`handler_error` au lieu de `conflict`. Défaut préexistant, orthogonal à W.1,
laissé tel quel volontairement.

## Fichiers / zones utiles

- `crates/konnect-sexp/src/writer.rs` (`ensure_kicad_schematic_is_closed`),
  `crates/konnect-sexp/src/transaction.rs`, `crates/konnect-sexp/src/error.rs`
- `crates/konnect-core/src/mcp/error.rs` (`from_anyhow`),
  `crates/konnect-core/tests/kicad_editor_lock.rs`
- `crates/konnect-core/src/tools/footprint_graphics.rs`,
  `crates/konnect-core/tests/footprint_graphics.rs`
- `crates/konnect-core/src/tools/library.rs` (`courtyard_bbox`, `body_bbox`,
  `snap_courtyard_outward`, `pin1_dot_center`, `build_footprint_graphics`,
  `handle_get_footprint_info`)
- `crates/konnect-core/src/tools/sch_components.rs`
  (`handle_edit_schematic_component`) — cible de W.3
- `scripts/live-editor-lock.ps1`, `scripts/live-schematic-e2e.ps1`,
  `scripts/live-pcb-e2e.ps1`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Projet Hi-Fi : `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi`
  - empreintes défectueuses dans `HifiAmp_TPA3255_Local.pretty\`
  - `plan.md` B2.8 (`on_board`), D1.8 (graphiques d'empreinte)

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
4. Les toolsets sont opt-in : un client charge `load_toolset` avant d'appeler,
   sinon chaque outil répond `toolset_not_loaded` et une assertion de refus
   passe pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir : une fenêtre qui vient
   d'apparaître peut l'ignorer. La fermeture propre se retente.

## NEXT ACTION

W.2.5 — Rejouer les deux empreintes Hi-Fi défectueuses (`CF_Film_Box_P5.00mm_
7.2x3.5mm` et `Fuse_Schurter_UMT-H_5.3x16mm` dans `HifiAmp_TPA3255_Local.
pretty\`) par le MCP seul : recréation avec le courtyard et le repère corrigés,
ou correction en place par `set_footprint_graphics`, sans édition externe.
