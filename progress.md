# PROGRESS

## Phase actuelle

**V — Routage DocumentType Eeschema.** V.3 terminée.

## Tâche actuelle

**V.4.1 — Préparer version, lockfiles et notes de `v1.1.3`.**

## Dernière tâche validée

**V.3.2 et V.3.3 — Smoke-tests réels et consignation du défaut.**

Validation : `scripts/live-pcb-e2e.ps1` sort 0, 3 tests live PASS — la voie PCB
n'a pas régressé. Smoke-test Hi-Fi en lecture seule : Eeschema ouvre
`HifiAmp_TPA3255.kicad_sch`, le contexte est résolu `schematic`, et
`.kicad_sch`, `.kicad_pcb`, `.kicad_pro`, `.kicad_sym` sont bit-identiques avant
et après ; B1.3 non reprise, aucun fichier KiCad édité. Défaut consigné dans
`…\Chaine Hifi\reports\MCP_BUG-documenttype-routing-eeschema.md`.

Avant : **V.3.1** validée par `scripts/live-schematic-e2e.ps1`, 7 contrôles PASS
sur KiCad `10.0.6` réel avec `konnect.exe` SHA256 `8D847386…C796E434`.

## Décisions actives

- Branche `ai/documenttype-routing-v1.1.3` depuis `9a051146`.
- Le contexte indéterminé échoue explicitement, jamais PCB.
- `api.enable_server` est à `true` dans le profil réel ; sauvegarde
  `kicad_common.json.backup-v3.1-crash-20260831`.
- Les copies de rollback du plugin vivent hors de `3rdparty`, dans
  `C:\Users\FlowUP\Documents\KiCad\10.0\konnect-plugin-backups`.
- `10.0.6` reste la seule version KiCad installée ; aucune installation n'a été
  modifiée. Une extraction `10.0.3` a servi de témoin jetable et n'est plus
  nécessaire.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le profil
  réel de l'utilisateur.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `scripts/live-schematic-e2e.ps1`, `scripts/live-pcb-e2e.ps1`
- `crates/konnect/src/main.rs` (`--document-type`), `crates/konnect-ipc/src/client.rs`
- `crates/konnect-core/src/tools/project.rs` (`open_project`, `save_project`)
- `plugin/{plugin.json,__init__.py}`, `packaging/`
- Installation plugin :
  `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Build validé : `target/release/konnect.exe`, SHA256
  `8D84738B603755F7C3B728822778785A3BD22C7CF3CBFE3DCBA11210C796E434`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Projet Hi-Fi : `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi`

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

V.4.1 — Préparer la release `v1.1.3` selon la politique existante : version,
lockfiles et notes, sans committer le tag ni publier.
