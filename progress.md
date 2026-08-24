# PROGRESS

## Phase actuelle

**M — comparaison des trois modes.** K.1 est close : les deux campagnes
(codex 14/14, claude sonnet 14/14, aucun void) et l'ancre `claude-opus-5` sont
mesurées. Les phases D, F, K et L sont closes. La phase I reste conditionnée au
matériel : cette machine a KiCad 10.0, pas KiCad 11 / `kicad-cli api-server`.

## Tâche actuelle

**M.1.1 — table de comparaison des trois modes** sur la même suite golden
(baseline / direct / agent), puis M.1.2 : chaque critère V1 re-mesuré, les
manqués enregistrés comme manqués (INV6). Sortie attendue :
`docs/benchmark.md`, reproductible depuis les artefacts committés.

## Dernière tâche validée

**K.1.1 — ancre `claude-opus-5`.** `sch_inspection` ×1, cap $5.00, incident 529
résolu (sonde : `terminal_reason: completed`). Run complet à **$0.3861**,
11 tours, **8 aller-retours**, `DESIGN_PASS_RATE 1/1`, `SAFETY_VIOLATIONS 0`,
`OFF_SERVER_CALLS 0`, `VOID_RUNS 0/1` ; seule violation, la route stricte
(`missing_expected`). `--rescore --enforce` reproduit le score hors ligne.

Le résultat porteur n'est pas le prix mais la route : Opus est **le premier
agent, tous harnais confondus, à passer par la gateway** — 3 `kicad_invoke`
portant 15 appels audités en 8 aller-retours (sonnet : 0 `kicad_invoke` sur
toute la campagne ; codex : 0). La branche *unwrap* que K.1.4 déclarait non
exercée a donc tourné sur sortie réelle. Et le batching a payé le modèle plus
cher : $0.3861 se place **entre** les deux runs sonnet de la même tâche
($0.4455 et $0.2448).

## Décisions actives

- D98 : le projet peut consommer la capacité Claude nécessaire, quel que soit
  le modèle, sans nouvel accord par run. Les caps opérationnels peuvent être
  augmentés et les runs relancés automatiquement.
- D97 : un re-run remplace un void dans sa campagne ; l'ancre Opus reste une
  campagne distincte.
- Un cap de budget ne doit jamais pouvoir voider sa propre mesure : $2.00 avait
  voidé `sch_ldo`, l'ancre est partie à $5.00.

## Blocage actif

Aucun. L'incident Anthropic du 2026-08-24 07:27 CEST est résolu ; les deux
essais 529 restent conservés comme preuves de runs void.

## Fichiers / zones utiles

- `bench/results/k11-claude-opus5-anchor-r3.{json,log}` et
  `k11-logs-opus5-anchor-r3/` — l'ancre.
- `bench/results/k11-claude-sonnet5.json`, `k11-codex.json` — les campagnes.
- `bench/harness_runner.py` — `--rescore`, `--merge`, classification void.
- `bench/runner.py` — le chemin oracle, l'autre moitié de la table M.1.

## NEXT ACTION

Ouvrir **M.1.1** : inventorier ce que `bench/runner.py` (baseline / direct) et
`bench/harness_runner.py` (agent) produisent déjà sur la suite golden, décider
ce qui manque pour une table à trois colonnes comparable, puis rédiger
`docs/benchmark.md` à partir des artefacts committés uniquement.
