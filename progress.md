# PROGRESS

## Phase actuelle

**Z — Benchmark PCB live V2 contre Konnect v0.13.0**, branche
`ai/pcb-live-bench-v2`. Y4 (reprise Hi-Fi, autre dépôt) reste ouverte, en
attente : la revue du 2026-10-04 a priorité.

## Tâche actuelle

Z3.2 — scénario A (sync schéma → PCB).

## Dernière tâche validée

**Z1 (scénario B) et Z3.1 (scénario E).**

Validation :
- Contrôles injectés par `kipy` : bon = vert, mauvais = rouge
  (`track_dangling` 1, non-connectés 2). Oracle valide.
- Fork : `route_pad_to_pad` répond `routed: true` mais trace vers la position
  **sauvegardée** de R2 → faux succès. Upstream `v0.13.0` : trace vers la
  position live (`source: ipc`), DRC propre.
- E : contrôles valides ; fork **faux vert** (`run_drc` sans
  `--schematic-parity` → 0 quand l'arbitre trouve 1) ; upstream 1 puis 0.
- Un run par implémentation ; trois runs restent à faire (Z4).
- Z2 fait côté recherche (à consigner dans le rapport) : #700 = défaut B, fermé
  upstream par PR #719 avant v0.13.0 ; #779 (zone ⇒ tous nets « routés »)
  **ouverte** ; #791 fermée par #805 (après v0.13.0) ; KiCad stable 10.0.6,
  10.0.7 en RC2.

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

Z3.2 — scénario A : sur `fixtures/divider/`, modifier le schéma (ajout d'un
symbole R3 + changement de valeur), demander à chaque serveur la sync schéma →
PCB, juger par `kipy` (R1/R2 et H1/H2 aux mêmes positions, piste /VOUT
intacte, R3 présent) puis parité `kicad-cli` = 0 ; contrôles bon/mauvais.
