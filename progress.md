# PROGRESS

## Phase actuelle

**V — Routage DocumentType Eeschema.** Phase terminée, `v1.1.3` publiée et
installée.

## Tâche actuelle

Aucune. Prochaine phase à décider : `T.2.1`, définir l'étape suivante du
benchmark Hi-Fi et ses critères de validation à partir du brief.

## Dernière tâche validée

**V.4.2 à V.4.4 — Publication et mise en service de `v1.1.3`.**

Validation : PR #15 sur `agentic/main`, 7 checks CI PASS ; merge `7c89182`, CI
PASS à nouveau sur ce commit ; tag annoté `v1.1.3` posé dessus ; workflow
Release `success` et 7 artefacts publiés (3 paquets PCM, 4 binaires). Paquet PCM
Windows inspecté : `metadata.json` en `1.1.3`, binaire `konnect 1.1.3`, et
`plugin.json` porte bien les deux actions scopées avec
`--document-type pcb|schematic`. Installé comme seule version en vigueur, SHA256
`C4696CE6…00B8B0C0` identique à l'artefact ; le runtime installé repasse les
7 contrôles de `scripts/live-schematic-e2e.ps1`.

## Décisions actives

- `v1.1.3` est la version en vigueur. Rollback unique et explicite :
  `C:\Users\FlowUP\Documents\KiCad\10.0\konnect-plugin-backups\com_github_mixelpixx_konnect.rollback-v1.1.2-20260831`,
  hors de `3rdparty`. Les deux sauvegardes antérieures ont été supprimées ;
  elles restent reconstructibles depuis les releases GitHub.
- Le contexte de document indéterminé échoue explicitement, jamais PCB.
- `api.enable_server` est à `true` dans le profil réel ; sauvegarde
  `kicad_common.json.backup-v3.1-crash-20260831`.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le profil
  réel de l'utilisateur.
- Le projet Hi-Fi est intact ; B1.3 n'a pas été reprise.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `scripts/live-schematic-e2e.ps1`, `scripts/live-pcb-e2e.ps1`
- `crates/konnect/src/main.rs` (`--document-type`), `crates/konnect-ipc/src/client.rs`
- `crates/konnect-core/src/tools/project.rs` (`open_project`, `save_project`)
- `plugin/{plugin.json,__init__.py}`, `packaging/`
- Installation active :
  `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Projet Hi-Fi : `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi`,
  défaut consigné dans `reports\MCP_BUG-documenttype-routing-eeschema.md`

## Préconditions de tout test live

1. Un seul répertoire sous `3rdparty` par identifiant de plugin. Trois copies de
   `com_github_mixelpixx_konnect` tuent l'éditeur 3 s après démarrage
   (`0xC0000005`, `wxbase332u_vc_x64_custom.dll`), pipe publié puis perdu.
   Identique en `10.0.3` et `10.0.6` : ce n'est pas une régression de version.
2. Aucune autre instance KiCad ne détient le socket d'API — sinon les requêtes
   partent au mauvais éditeur et reviennent en « does not handle … for this
   document type ». `live-schematic-e2e.ps1` refuse de démarrer dans ce cas.
3. Aucun dialogue modal : l'assistant `Configuration de KiCad` et l'avis de
   format de fichier ancien font répondre `AS_NOT_READY` sur un pipe présent.
   Les scripts répondent aux invites et mettent la carte jetable à niveau.

## NEXT ACTION

T.2.1 — Définir la prochaine étape fonctionnelle du benchmark Hi-Fi et ses
critères de validation à partir du brief, avant toute modification du projet.
