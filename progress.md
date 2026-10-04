# PROGRESS

## Phase actuelle

**Z — Benchmark PCB live V2 contre Konnect v0.13.0 : terminée**, mergée dans
`agentic/main` (`bc3a815`, PR #22). Y4 (reprise Hi-Fi, autre dépôt) reste la
seule unité ouverte.

## Tâche actuelle

Aucune dans ce dépôt.

## Dernière tâche validée

**Z6 — rejeu, docs, PR.**

Validation :
- `bench/results/pcb_live_v2-after-z7-20261004.json` : fork A–E fonctionnels
  3/3, 0 faux succès ; upstream v0.13.0 : A, B, D, E 3/3, C refus à tort 3/3.
- CI PR #22 verte (fmt, clippy, tests 3 OS, PCM) ; `gate.ps1` PASSED sur Z5.
- Rapport : `docs/benchmark-pcb-live-v2.md` ; README et `docs/benchmark.md` ne
  présentent plus v0.2.2 comme comparaison avec l'upstream actuel.

## Décisions actives

- La comparaison fork/upstream se fonde sur `docs/benchmark-pcb-live-v2.md`
  (KiCad 10.0.6, upstream tag `v0.13.0`), jamais sur le benchmark v0.2.2.
- Arbitres du bench : `kipy` 0.8.0 (venv `../_bench-venv`) et `kicad-cli` ; un
  succès MCP n'est jamais une preuve ; tout oracle a deux contrôles.
- Upstream = réservoir de primitives, import sélectif seulement sur cellule
  mesurée perdante. Restent volontairement non importés : `FlipItems` live (D :
  état final déjà correct, coût = un redémarrage éditeur).
- `update_pcb_from_schematic` est `UNPROVEN` dans la matrice : la règle exige un
  test live Rust (`kicad_reads_back`) ; le bench Python ne compte pas.
- Le flake `kam-llm openai_compat::absent_backend_is_unreachable_not_a_panic`
  (port éphémère) peut arrêter `gate.ps1` ; il passe isolé.
- `scripts/*.ps1` et `gate.ps1` se lancent avec `pwsh`.
- Convention : une branche `ai/<phase>`, une PR par phase, merge commit.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- Bench : `bench/pcb_live_v2/{run.py,kicad_session.py,report.py,fixtures/}` ;
  lancer `../_bench-venv/Scripts/python.exe bench/pcb_live_v2/run.py
  --scenario A B C D E --runs 3` (upstream : worktree `../_upstream-v0.13.0`,
  binaire `../_upstream-target/release/konnect.exe`).
- Sync : `crates/konnect-core/src/tools/pcb_sync.rs` (`record_zone_nets` = #779).
- Projet Hi-Fi : `~/Documents/Etabli/Projets/Chaine Hifi`.

## Préconditions de tout test live

1. Aucune autre instance KiCad ouverte (le harness refuse sinon).
2. Aucun dialogue modal (profil dédié, `pcb upgrade` des copies).
3. Toolsets opt-in : `load_toolset` avant l'appel.

## NEXT ACTION

Y4.1 — sur le projet Hi-Fi (dépôt et continuité distincts), reprendre sa
`NEXT ACTION` F1.2-b1 avec un binaire contenant Z (`route_pad_to_pad` live,
parité réelle, sync) ; une release v1.3.0 préalable relève d'une décision de
l'utilisateur.
