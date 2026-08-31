# PROGRESS

## Phase actuelle

**V — Routage DocumentType Eeschema.**

## Tâche actuelle

**V.3.2 — Smoke-test Pcbnew et smoke-test Hi-Fi non destructif.**

## Dernière tâche validée

**V.3.1 — Validation réelle Eeschema** (après **V.3.4**, précondition runtime).

Validation, KiCad `10.0.6` réel, build `konnect.exe` SHA256 `8D847386…C796E434`,
pilotée par `scripts/live-schematic-e2e.ps1` : 7 contrôles PASS. Contexte
`schematic` → `open_documents` de type `schematic`, `save_project` répond
« Schematic changes are already persisted to disk. » ; contexte `pcb` sans
carte ouverte → refus explicite (`GetOpenDocuments` non géré pour ce type,
aucun repli PCB) ; `Auto` → résout l'unique handler vif. Après arrêt d'Eeschema,
`.kicad_sch` et `.kicad_pcb` sont bit-identiques — la régression `v1.1.2`
écrivait la carte. Réouverture : routage schematic conservé.

## Décisions actives

- Branche `ai/documenttype-routing-v1.1.3` depuis `9a051146`.
- Le projet Hi-Fi reste intact jusqu'à V.3.2 ; aucune poursuite de B1.3.
- Le contexte indéterminé échoue explicitement, jamais PCB.
- `api.enable_server` est à `true` dans le profil réel ; sauvegardes
  `kicad_common.json.backup-v3.1-crash-20260831` et antérieures.
- Les copies de rollback du plugin vivent hors de `3rdparty`, dans
  `C:\Users\FlowUP\Documents\KiCad\10.0\konnect-plugin-backups`.
- Aucune installation KiCad n'a été modifiée : `10.0.6` reste la seule version
  installée. Une extraction `10.0.3` sert uniquement de témoin jetable.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `scripts/live-schematic-e2e.ps1` (V.3.1 exécutable), `scripts/live-pcb-e2e.ps1`
- `plugin/{plugin.json,__init__.py}`
- `crates/konnect/src/{main.rs,config.rs}`
- `crates/konnect-ipc/src/{client,lib}.rs`, `crates/konnect-ipc/tests/`
- `crates/konnect-core/src/tools/project.rs` (`open_project`, `save_project`)
- Installation plugin :
  `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Build validé : `target/release/konnect.exe`, SHA256
  `8D84738B603755F7C3B728822778785A3BD22C7CF3CBFE3DCBA11210C796E434`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Configuration KiCad réelle :
  `C:\Users\FlowUP\AppData\Roaming\kicad\10.0\kicad_common.json`
- Témoin jetable 10.0.3 :
  `C:\Users\FlowUP\AppData\Local\Temp\claude\kicad-downgrade\kicad-10.0.3`

## Deux préconditions de tout test live (V.3.4)

1. Un seul répertoire sous `3rdparty` par identifiant de plugin. Trois copies
   de `com_github_mixelpixx_konnect` tuent l'éditeur 3 s après démarrage
   (`0xC0000005`, `wxbase332u_vc_x64_custom.dll`), pipe publié puis perdu, ce
   qui se lit à tort comme « connection refused ». Identique en `10.0.3` et
   `10.0.6` : ce n'est pas une régression de version.
2. Aucun dialogue modal : l'assistant `Configuration de KiCad` fait répondre
   `AS_NOT_READY` sur un pipe présent. Le script utilise un `KICAD_CONFIG_HOME`
   dédié, copie du profil réel avec `do_not_show_again` répondu.

## NEXT ACTION

V.3.2 — Exécuter `scripts/live-pcb-e2e.ps1` comme smoke-test Pcbnew, puis un
smoke-test Hi-Fi strictement non destructif (lecture seule, aucune écriture,
aucune poursuite de B1.3), et consigner les deux résultats.
