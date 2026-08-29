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

Le serveur privé `kicad-agentic-mcp` n'est pas rattaché à cette session : le
contrôleur KiCad a inspecté 314 outils et n'a trouvé aucun outil KiCad,
`kicad_describe` ou `kicad_invoke`. Aucun projet n'a été ouvert ni modifié. La
build de développement a été retirée et l'installation active restaurée sur
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

## NEXT ACTION

V.3.1 — Rattacher `kicad-agentic-mcp`, réinstaller la build de développement
du commit `1bfd3ad`, puis valider depuis Eeschema les pipes, lecture,
`save_project`, réouverture et persistance sur un projet temporaire hors Hi-Fi.
