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
- Anciennes versions et backups servent uniquement au rollback explicite.

## Blocage actif

`kicad-agentic-mcp` est maintenant enregistré globalement et activé par
`codex mcp list`, avec le build corrigé et `--document-type schematic`, mais
la session courante ne recharge pas les MCP à chaud. Deux contrôleurs KiCad,
dont un créé après l'enregistrement, voient zéro outil pertinent. La
documentation OpenAI ne décrit l'initialisation MCP qu'au démarrage/reprise :
une nouvelle session est la précondition manquante. Aucun KiCad ni projet n'a
été ouvert. L'installation active et le rollback restent identiques à
`v1.1.2`, SHA256
`C898F96D63B69ED44BB73E44433EE66F002E01D87262388F6A945D07D30D3B7D`.

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
- Config MCP : `C:\Users\FlowUP\.codex\config.toml`; sauvegarde exacte suffixée
  `.backup-v3.1-mcp-20260829`

## NEXT ACTION

V.3.1 — Reprendre dans une nouvelle session Codex afin de charger
`kicad-agentic-mcp`, confirmer ses outils, puis installer temporairement la
build prête et valider Eeschema/Pcbnew sur une fixture hors Hi-Fi avec rollback.
