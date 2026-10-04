# PROGRESS

## Phase actuelle

**Z — Benchmark PCB live V2 contre Konnect v0.13.0**, branche
`ai/pcb-live-bench-v2`. Y4 (reprise Hi-Fi, autre dépôt) reste ouverte, en
attente : la revue du 2026-10-04 a priorité.

## Tâche actuelle

Z5 — backports justifiés par la matrice : B (pads live) et E (parité
réellement exécutée) délégués à un `code-worker` (non commités tant que non
validés) ; Z5.3 (message flip D) fait dans `pcb_components.rs`, non commité.

## Dernière tâche validée

**Z4 — matrice de base** `bench/results/pcb_live_v2-baseline-20261004.json`.

Validation :
- 40 runs, 0 erreur de harness, contrôles valides dans les 5 scénarios,
  chaque cellule identique sur ses 3 runs.
- Fork : B et E faux succès 3/3 ; A et C capacité absente ; D fonctionnel 3/3
  mais redémarrage éditeur requis. Upstream v0.13.0 : A, B, D, E 3/3 ; C refus
  à tort 3/3 (#779, cause : toute zone ⇒ tous nets « routés »,
  `pcb_sync.rs` ~l.1600).
- Rapport : `docs/benchmark-pcb-live-v2.md`.

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

Aucun.

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

Z5.1/Z5.2 — vérifier le retour du worker (diff, fmt, clippy, tests), puis
`cargo build --release -p konnect`, rejouer B, D, E ×3 et committer si le fork
y est fonctionnel sans faux succès.
