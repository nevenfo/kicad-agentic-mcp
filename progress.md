# PROGRESS

## Phase actuelle

**R — Launch & adoption.** R.1 à R.9 sont clos. **R.10 — release v1.1.1** est
en cours, dans le périmètre fermé validé le 2026-08-26 : R.7, R.8, R.9.1,
R.9.2 et F-03 uniquement.

## Tâche actuelle

**R.10.6 — tag, push et contrôle des sept assets.** La release locale est prête
et sa gate pré-tag est verte.

## Dernière tâche validée

**R.10.5 — gate et E2E réel avant tag.** R.10.1 à R.10.5 sont cochées.

Validation :

- métadonnées PCM : schéma PASS ; exemples JSON : parsing PASS ;
- versions : les douze crates workspace et le viewer résolvent 1.1.1 ;
- `gate.ps1` : `fmt`, `clippy -D warnings`, tests, doctests et build release
  PASS ;
- `scripts/live-pcb-e2e.ps1` : PASS sur KiCad 10, 2 tests `konnect-ipc` et
  1 test `konnect`; découverte automatique observée pour `ipc_address`,
  `kicad-cli` et `kicad` ;
- aucun tag n'existait au moment de l'E2E, conformément à D144.

## Décisions actives

- Périmètre strict : R.7, R.8, R.9.1, R.9.2 et F-03 ; aucun correctif
  opportuniste.
- INV-R1 : R.10.7 doit installer et vérifier l'artefact publié, pas le build
  local.
- L'identifier PCM `com.github.mixelpixx.konnect` reste inchangé pour préserver
  le dossier d'installation et les configurations existantes.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- Release : `RELEASE_NOTES.md`, `README.md`, `docs/TROUBLESHOOTING.md`,
  `examples/*.json`, `packaging/metadata.json`, manifests et lockfiles Cargo.
- Workflow : `.github/workflows/release.yml`.

## NEXT ACTION

Créer et pousser le commit de release validé, créer/pousser le tag `v1.1.1`,
puis contrôler sur GitHub la présence et la taille des sept assets.
