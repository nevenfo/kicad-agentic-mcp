# PROGRESS

## Phase actuelle

**U — Publication v1.1.2.**

## Tâche actuelle

**U.1.4 — Commit, push et CI du candidat.**

## Dernière tâche validée

**U.1.3 — Gate local du candidat v1.1.2.**

Validation : `gate.ps1` PASS ; check workspace `--locked` PASS ; régression
`project_symbol_library_e2e` 1/1 ; viewer hors workspace 20/20 ; builds release
serveur/viewer `--locked` PASS ; métadonnées et PCM Windows PASS ; binaire local
`konnect 1.1.2`.

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

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_pro`
- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_sch`
- `Cargo.toml`, `Cargo.lock`, `crates/schematic-viewer/Cargo.toml`
- `crates/schematic-viewer/Cargo.lock`, `RELEASE_NOTES.md`
- `.github/workflows/{ci,e2e-kicad,release}.yml`

## NEXT ACTION

U.1.4 — Examiner et indexer uniquement les fichiers de release, créer le commit
candidat, pousser `agentic/main`, puis vérifier la CI de son SHA exact.
