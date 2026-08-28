# PROGRESS

## Phase actuelle

**T — Reprise du benchmark Hi-Fi.**

## Tâche actuelle

**T.2.1 — Définition de la prochaine étape du benchmark.**

## Dernière tâche validée

**U.1.6 — Publication v1.1.2 et retour au benchmark.**

Validation : candidat/tag `c7b66d8f511cfc4f7358dc279196800f88fc271d` ;
CI `33148447984` PASS ; Release `v1.1.2` `33148755072` PASS ; sept artefacts ;
PCM Windows publié validé avec serveur/viewer, métadonnées `1.1.2` et binaire
embarqué `konnect 1.1.2`.

## Décisions actives

- Résolution : table projet, table globale, bibliothèques installées ;
  `${KIPRJMOD}` est ancré au dossier du schéma.
- `save_project` conserve son API et devient document-aware ; un schéma déjà
  persisté est un succès explicite, sans commande PCB.
- Un appel IPC sans chemin reste conservateur et requiert `documents` explicites
  pour la protection transactionnelle.
- Le serveur MCP installé correspond au build de HEAD `9bcd9fb` ; l'ancien
  exécutable reste disponible comme rollback `konnect.exe.pre-9bcd9fb.bak`.
- La release reste strictement une patch `v1.1.2`; aucun développement Hi-Fi
  ni changement fonctionnel supplémentaire n'entre dans son périmètre.
- `v1.1.2` est publiée ; le tag reste sur le commit candidat validé et la phase
  T reprend sans modifier le projet Hi-Fi.

## Blocage actif

La suite du benchmark Hi-Fi après le placement U1 n'est décrite ni dans le
dépôt ni dans la mémoire de projet ; une cible fonctionnelle est indispensable
avant toute nouvelle modification du schéma.

## Fichiers / zones utiles

- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_pro`
- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_sch`
- `Cargo.toml`, `Cargo.lock`, `crates/schematic-viewer/Cargo.toml`
- `crates/schematic-viewer/Cargo.lock`, `RELEASE_NOTES.md`
- `.github/workflows/{ci,e2e-kicad,release}.yml`

## NEXT ACTION

T.2.1 — Obtenir le brief de la prochaine étape du benchmark Hi-Fi et fixer son
critère de validation avant toute nouvelle écriture dans le projet KiCad.
