# PROGRESS

## Phase actuelle

**R — Launch & adoption.** R.1 à R.10.7 sont clos et la release publique
**v1.1.1** est validée. **R.10.8** est ouvert uniquement pour rendre portables
deux tests Windows de R.7/R.9.1 avant fusion de la PR #12.

## Tâche actuelle

**R.10.8 — borner les tests `%LOCALAPPDATA%` à Windows.** La correction locale
est faite et attend la preuve de la CI multi-plateforme.

## Dernière tâche validée

**R.10.7 — parcours de l'artefact publié sans configuration.** Preuves :

- tag `v1.1.1` sur `7d565ce`, workflow release `33012207008` : PASS ;
- release publique, sept assets présents et non vides ;
- installation PCM Windows publiée : v1.1.1 stable, auteur `nevenfo`, homepage
  du fork ; aucun `--config`, fichier de configuration ou variable de découverte ;
- découverte de l'IPC par défaut, `kicad-cli` et `kicad` ;
  `get_component_list` : 4 composants ; `run_drc` : 1 avertissement, 0 erreur ;
- configuration utilisateur restaurée, KiCad fermé sans sauvegarde, temporaires
  supprimés.

## Décisions actives

- Périmètre release inchangé : R.7, R.8, R.9.1, R.9.2 et F-03 uniquement.
- L'identifier PCM `com.github.mixelpixx.konnect` reste inchangé.

## Blocage actif

PR #12 : le premier run CI `33014616858` échoue sur deux tests qui construisent
une arborescence `%LOCALAPPDATA%` Windows mais s'exécutent sur macOS/Linux. La
correction `#[cfg(target_os = "windows")]` passe localement : 8 tests ciblés,
`fmt` et `git diff --check` PASS. Il faut la preuve du prochain run CI.

## Fichiers / zones utiles

- `crates/konnect-core/src/kicad_locate.rs` — deux tests R.10.8.
- `plan.md` § R.10 ; PR `https://github.com/nevenfo/kicad-agentic-mcp/pull/12`.

## NEXT ACTION

Committer et pousser R.10.8, attendre la CI multi-plateforme de la PR #12, puis
cocher R.10.8 et fusionner seulement si tous les checks requis sont PASS.
