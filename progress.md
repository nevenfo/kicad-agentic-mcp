# PROGRESS

## Phase actuelle

**V — Routage DocumentType Eeschema.**

## Tâche actuelle

**V.3.4 — Rétablir un runtime KiCad dont le serveur API IPC fonctionne**
(préalable bloquant à V.3.1).

## Dernière tâche validée

**V.2.3 — Correctif et gate local complet.**

Validation : les actions bornées par scope passent explicitement
`--document-type pcb|schematic`; `Auto` refuse zéro ou deux handlers et tout
contexte vide, sans fallback PCB. Gate : check workspace PASS ; 1 422 tests
PASS/38 ignorés ; 5 doctests PASS/3 ignorés ; clippy strict PASS ; viewer check
et 20 tests PASS ; format/diff PASS.

## Décisions actives

- Branche `ai/documenttype-routing-v1.1.3` depuis `9a051146`.
- Le projet Hi-Fi reste intact jusqu'à V.3.2 ; aucune poursuite de B1.3.
- Le contexte indéterminé échoue explicitement, jamais PCB.
- `api.enable_server` est repassé à `false` dans le profil réel : laissé à
  `true`, il rendait Eeschema et Pcbnew inutilisables. Sauvegarde
  `kicad_common.json.backup-v3.1-crash-20260831`.
- Anciennes versions et backups servent uniquement au rollback explicite.

## Blocage actif

KiCad `10.0.6` (installé le 28/08/2026, contre `10.0.3` lors de J.3.1) n'expose
plus du tout le serveur API IPC. Avec `api.enable_server: true` :
`eeschema.exe` et `pcbnew.exe` autonomes plantent au démarrage,
`0xC0000005` dans `wxbase332u_vc_x64_custom.dll` (wxWidgets 3.3.2, offset
`0xc9950`) ; `kicad.exe` démarre mais aucun `\\.\pipe\*\kicad\api.sock`
n'apparaît après 60 s. Reproduit avec un profil `KICAD_CONFIG_HOME` isolé ne
contenant que ce drapeau. Exclus : fixture, projet Hi-Fi, plugin Konnect,
`eeschema.json`, `sym-lib-table`, `working_dir` obsolète, `interpreter_path`,
absence de `%LOCALAPPDATA%\Temp\kicad`, `KICAD_API_SOCKET` forcé. Aucun ticket
amont ni contournement documenté (GitLab KiCad, forum, notes 10.0.4/5/6).
Prochaine tentative : installer une 10.0.x antérieure depuis
`https://downloads.kicad.org/kicad/windows/explore/stable` — décision
utilisateur requise (remplacement ou installation côte à côte).

## Fichiers / zones utiles

- `plugin/{plugin.json,__init__.py}`
- `crates/konnect/src/{main.rs,config.rs}`
- `crates/konnect-ipc/src/{client,lib}.rs`, `crates/konnect-ipc/tests/`
- `crates/konnect-core/src/{mcp/handler.rs,tools/{mod,project}.rs}`
- `scripts/live-pcb-e2e.ps1` (détection du pipe, préconditions API)
- Installation : `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Rollback : même racine, suffixe `.backup-v1.1.2-pre-v3.1-20260829`
- Build prêt : `target/release/konnect.exe`, SHA256
  `8D84738B603755F7C3B728822778785A3BD22C7CF3CBFE3DCBA11210C796E434`
- Fixture :
  `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31\konnect_v31_eeschema_pipe_fixture.kicad_sch`
- Configuration KiCad : `C:\Users\FlowUP\AppData\Roaming\kicad\10.0\kicad_common.json`
- Profils de bisect jetables :
  `C:\Users\FlowUP\AppData\Local\Temp\claude\kicad-cfg-{bisect,min,pristine-cfg}`
- Config MCP : `C:\Users\FlowUP\.codex\config.toml`; sauvegarde exacte suffixée
  `.backup-v3.1-mcp-20260829`

## NEXT ACTION

V.3.4 — Obtenir la décision utilisateur sur le runtime KiCad, puis installer une
10.0.x dont le serveur API IPC répond, vérifier l'apparition de
`\\.\pipe\*\kicad\api.sock` avec `api.enable_server: true`, et seulement ensuite
reprendre V.3.1 sur la fixture isolée.
