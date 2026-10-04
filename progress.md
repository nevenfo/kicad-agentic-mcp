# PROGRESS

## Phase actuelle

**Z — Benchmark PCB live V2 contre Konnect v0.13.0**, branche
`ai/pcb-live-bench-v2`. Y4 (reprise Hi-Fi, autre dépôt) reste ouverte, en
attente : la revue du 2026-10-04 a priorité.

## Tâche actuelle

Z6.2 — gate local sur l'intégration Z7. Lancé, **arrêté par Claude Code faute
de mémoire système** après fmt et clippy verts, pendant la compilation des
tests. Ne pas le relancer sans accord de l'utilisateur.

## Dernière tâche validée

**Z7 — sync schéma → PCB portée, #779 corrigé** ; **Z6.1/Z6.3** rejeu et docs.

Validation :
- `bench/results/pcb_live_v2-after-z7-20261004.json` : fork A–E fonctionnels
  3/3, 0 faux succès, contrôles valides ; upstream v0.13.0 C refus 3/3.
- Suite complète verte dans le worktree du worker (74 binaires de test).
- Gate `0cae8bf` (Z5) : `GATE PASSED`. Gate Z7 : incomplet (mémoire).
- Rapport et README/benchmark.md alignés ; compteurs publics 205 outils.

## Décisions actives

- Arbitres : `kipy` 0.8.0 (venv `../_bench-venv`) pour le live, `kicad-cli`
  10.0.6 pour le saved. Le JSON des serveurs n'est jamais une preuve.
- Les mouvements « utilisateur GUI » non sauvegardés sont faits par `kipy`,
  côté oracle, pas par le serveur testé.
- Baseline upstream = tag `v0.13.0` (`6bbe3e4f`), worktree
  `../_upstream-v0.13.0`, binaire `../_upstream-target/release/konnect.exe`.
  `main` upstream (`3ad01f8`, 2026-10-03) n'est pas la baseline.
- Le correctif B (Z5.1) attend la matrice complète sauf si rien ne s'y oppose ;
  il est déjà justifié par mesure.
- `kicad-cli` ne valide pas par code de sortie : l'oracle est l'effet DRC. Le
  champ `type` d'une violation est stable, sa `description` traduite.
- Convention : une branche `ai/<phase>`, une PR par phase, merge commit.

## Blocage actif

Gate Z7 non terminé : machine à court de mémoire (arrêt par le harness, pas
un échec). Faits exclus : fmt et clippy verts sur le checkout intégré.
Prochaine tentative : `pwsh gate.ps1` quand la mémoire le permet, ou la CI de
la PR.

## Fichiers / zones utiles

- Harness : `bench/pcb_live_v2/{kicad_session.py,run.py,fixtures/}`.
- Lancer : `../_bench-venv/Scripts/python.exe bench/pcb_live_v2/run.py
  --scenario B --runs 3` (sortie `bench/results/pcb_live_v2-*.json`).
- Fork B : `crates/konnect-core/src/tools/pcb_routing.rs`
  (`handle_route_pad_to_pad`) ; upstream équivalent : `get_live_pad`.
- Analyse upstream déjà faite de `update_pcb_from_schematic` : `plan.md` X9.

## Préconditions de tout test live

1. Aucune autre instance KiCad ouverte (le harness refuse sinon).
2. Aucun dialogue modal (profil dédié copié, prompts désactivés, `pcb upgrade`
   des copies).
3. Les toolsets sont opt-in : `load_toolset` avant l'appel.

## NEXT ACTION

Z6.2 — obtenir un gate vert sur `ai/pcb-live-bench-v2` (CI de la PR, ou
`pwsh gate.ps1` relancé avec l'accord de l'utilisateur), puis Z6.4 : merger la
PR `ai/pcb-live-bench-v2` → `agentic/main`.
