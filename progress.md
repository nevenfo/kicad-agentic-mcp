# PROGRESS

## Phase actuelle

**V — Routage DocumentType Eeschema.**

## Tâche actuelle

**V.3.1 — Validation réelle Eeschema.**

## Dernière tâche validée

**V.2.3 — Correctif et gate local complet.**

Validation : la régression de `9bcd9fb` envoyait toujours d'abord
`DOCTYPE_PCB`. Les sources KiCad 10 confirment que `actions[].args`, bien
qu'omis du schéma JSON, est transmis à `argv`. Les actions bornées par scope
passent donc explicitement `--document-type pcb|schematic`; `Auto` refuse zéro
ou deux handlers et tout contexte vide, sans fallback PCB. Tests ciblés : IPC
32 PASS/1 ignoré, CLI/manifeste 2 PASS, core projet 29 PASS. Gate : check
workspace PASS ; 1 422 tests PASS/38 ignorés ; 5 doctests PASS/3 ignorés ;
clippy strict PASS ; viewer check et 20 tests PASS ; format/diff PASS.

## Décisions actives

- Branche `ai/documenttype-routing-v1.1.3` depuis `9a051146`.
- Le projet Hi-Fi reste intact jusqu'à V.3.2 ; aucune poursuite de B1.3.
- Le contexte indéterminé échoue explicitement, jamais PCB.
- L'API globale KiCad 10 est activée dans `kicad_common.json`; aucune fixture
  n'est modifiée tant que le serveur IPC n'est pas réellement à l'écoute.
- Anciennes versions et backups servent uniquement au rollback explicite.

## Blocage actif

La nouvelle session charge bien les outils MCP KiCad et la build corrigée est
active. Eeschema a été lancé sur la fixture isolée, puis relancé après passage
de `api.enable_server:false` à `true` dans
`C:\Users\FlowUP\AppData\Roaming\kicad\10.0\kicad_common.json`. Malgré cela,
`ipc_ready:false` persiste et la connexion à
`ipc://C:\Users\FlowUP\AppData\Local\Temp\kicad\api.sock` est refusée. La
fixture est intacte, le projet Hi-Fi n'a pas été ouvert et le rollback est
préservé. V.3.1 ne peut pas être validée avant que le serveur IPC KiCad écoute
réellement.

## Fichiers / zones utiles

- `plugin/{plugin.json,__init__.py}`
- `crates/konnect/src/main.rs`
- `crates/konnect-ipc/src/{client,lib}.rs`, `crates/konnect-ipc/tests/`
- `crates/konnect-core/src/{mcp/handler.rs,tools/{mod,project}.rs}`
- `packaging/`
- Installation : `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Rollback : même racine, suffixe `.backup-v1.1.2-pre-v3.1-20260829`
- Build prêt : `target/release/konnect.exe`, SHA256
  `8D84738B603755F7C3B728822778785A3BD22C7CF3CBFE3DCBA11210C796E434`
- Fixture :
  `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31\konnect_v31_eeschema_pipe_fixture.kicad_sch`
- Configuration KiCad :
  `C:\Users\FlowUP\AppData\Roaming\kicad\10.0\kicad_common.json`
- Config MCP : `C:\Users\FlowUP\.codex\config.toml`; sauvegarde exacte suffixée
  `.backup-v3.1-mcp-20260829`

## NEXT ACTION

V.3.1 — Rendre le serveur API IPC de KiCad 10 réellement à l'écoute sur
`api.sock`, confirmer `ipc_ready:true`, puis reprendre sur la fixture isolée :
lecture, `save_project`, fermeture/réouverture et persistance.
