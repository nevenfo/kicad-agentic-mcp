# PROGRESS

## Phase actuelle

**R — Launch & adoption : terminée.** R.1 à R.10 et tous les critères de sortie
sont cochés. La release publique **v1.1.1** porte uniquement R.7, R.8, R.9.1,
R.9.2 et F-03, conformément au périmètre validé le 2026-08-26.

## Tâche actuelle

Aucune. **R.10 — release v1.1.1** est close.

## Dernière tâche validée

**R.10.7 — parcours de l'artefact publié sans configuration.** Preuves :

- tag `v1.1.1` sur `7d565ce`, workflow release `33012207008` : PASS ;
- release publique non-draft/non-prerelease, sept assets présents et non vides ;
- zip PCM Windows publié : 12 267 698 octets, SHA-256
  `3E379B78E87B20CEECD0BEFC8AC130DBBA47900163CD67197E64DB41B90D14F7` ;
- installation via le Plugin and Content Manager : v1.1.1 stable, auteur
  `nevenfo`, homepage du fork ; binaire installé répond `konnect 1.1.1` ;
- pendant le parcours : aucun `konnect-settings.json`, aucun `--config`, et
  `KICAD_API_SOCKET`, `KICAD_CLI`, `KICAD_BINARY` absents de l'environnement ;
- découverte observée : IPC par défaut Windows, `kicad-cli` et `kicad` dans le
  préfixe KiCad 10 standard ;
- `get_component_list` : 4 composants live ; `run_drc` : réponse KiCad CLI,
  1 avertissement et 0 erreur.

La gate pré-tag (`fmt`, clippy, tests, doctests, build release) et l'E2E manuel
KiCad (2 tests IPC + 1 test MCP) étaient PASS avant création du tag. Le fichier
de configuration préexistant a été restauré après le parcours ; KiCad a été
fermé sans sauvegarde ; les fichiers temporaires ont été supprimés.

## Décisions actives

- L'identifier PCM `com.github.mixelpixx.konnect` reste inchangé pour préserver
  le dossier d'installation et les configurations existantes.
- INV-R1 à INV-R4 restent les invariants des futures releases et mesures.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- Release : `RELEASE_NOTES.md`, `README.md`, `packaging/metadata.json`.
- Lancement : `docs/launch/launch-kit.md`, brouillons `docs/launch/announce-*.md`,
  porte `docs/launch/decision-gate.md`, tally `docs/adoption.md`.

## NEXT ACTION

Décider si les brouillons R.4 doivent maintenant être publiés sur les canaux
externes nommés, puis rouvrir la porte R.6 lorsque `docs/adoption.md` contient
des données extérieures.
