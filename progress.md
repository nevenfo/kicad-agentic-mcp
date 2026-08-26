# PROGRESS

## Phase actuelle

**R — Launch & adoption : terminée.** R.1 à R.10.8 et tous les critères de
sortie sont cochés. La release publique **v1.1.1** est validée et la PR #12
porte la phase complète vers `agentic/main`.

## Tâche actuelle

Aucune.

## Dernière tâche validée

**R.10.8 — portabilité des tests de découverte Windows.** Les deux tests qui
construisent `%LOCALAPPDATA%\Programs\KiCad` sont bornés à Windows.

Validation :

- local : 8 tests `kicad_locate::resolve_binary_tests`, `fmt` et
  `git diff --check` PASS ;
- CI PR #12, run `33014965860` : sept checks PASS — format, clippy, packaging
  PCM, viewer, tests et doctests Windows/macOS/Ubuntu.

R.10.7 reste prouvée par le tag `v1.1.1` sur `7d565ce`, le workflow release
`33012207008` PASS, sept assets publics, et le parcours du PCM Windows publié
sans configuration : découverte IPC/CLI/GUI, 4 composants live et DRC
`1 warning / 0 error`. La configuration utilisateur a été restaurée et les
temporaires supprimés.

## Décisions actives

- Périmètre release : R.7, R.8, R.9.1, R.9.2 et F-03 uniquement.
- L'identifier PCM `com.github.mixelpixx.konnect` reste inchangé.
- INV-R1 à INV-R4 restent les invariants des futures releases et mesures.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- Release : `RELEASE_NOTES.md`, `README.md`, `packaging/metadata.json`.
- Lancement : `docs/launch/launch-kit.md`, `docs/launch/announce-*.md`,
  `docs/launch/decision-gate.md`, `docs/adoption.md`.
- PR : `https://github.com/nevenfo/kicad-agentic-mcp/pull/12`.

## NEXT ACTION

Décider si les brouillons R.4 doivent maintenant être publiés sur les canaux
externes nommés, puis rouvrir la porte R.6 lorsque `docs/adoption.md` contient
des données extérieures.
