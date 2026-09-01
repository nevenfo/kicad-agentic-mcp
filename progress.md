# PROGRESS

## Phase actuelle

**W — v1.1.4, les trois limitations Pareto.** W.1 terminée et validée en réel.

## Tâche actuelle

W.2 — Graphiques d'empreinte et courtyard.

## Dernière tâche validée

**W.1 — Garde de possession Eeschema.**

Toute mutation d'un `.kicad_sch` est refusée tant que le lock frère natif de
KiCad existe. Le garde vit dans le writer partagé
(`konnect-sexp::writer::ensure_kicad_schematic_is_closed`), donc tous les
chemins d'écriture schématique en bénéficient sans duplication : il est appelé
avant la création du scratch, à nouveau juste avant le `rename` final, dans
`write_new_atomic_unlocked`, et dans `commit_file_transaction` avant toute
écriture de journal ainsi que dans la récupération de journal.

Validation : 15 tests neufs (9 unitaires writer, 2 transaction, 4 intégration
MCP), suite complète `1437 passed / 0 failed`, doctests, `clippy` et `fmt`
verts. `scripts/live-editor-lock.ps1` passe ses 11 contrôles contre un
`eeschema.exe` 10.0 réel : refus `conflict`, SHA-256 du schéma identique,
lock intact, aucun scratch ni journal créé, lectures toujours disponibles,
puis après fermeture propre le même appel réussit et une relecture
indépendante voit la modification.

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
- `crates/konnect-core/src/tools/library.rs` (`build_footprint_graphics`,
  `courtyard_clearance`, `handle_create_footprint`) — cible de W.2
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

W.2.1 — Chemin vertical minimal d'édition des graphiques d'empreinte : relire
les graphiques d'un `.kicad_mod`, modifier une primitive, écrire, relire,
vérifier.
