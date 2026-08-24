# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2, K.1.4 et K.1.17 sont closes. K.1 dépend encore
de l'ancre Opus de K.1.1 ; la phase M dépend de K.1.1. Les phases D, F et L
sont closes. La phase I reste conditionnée au matériel : cette machine a
KiCad 10.0, pas KiCad 11 / `kicad-cli api-server`.

## Tâche actuelle

**K.1.1 — ancre `claude-opus-5`.** Les campagnes principales sont complètes :
Codex 14/14, Claude Sonnet 14/14, aucun void. L'ancre rejoue une tâche pour
rattacher la campagne Sonnet au smoke Opus de K.1.6.

## Dernière tâche validée

**Re-run Sonnet `sch_hierarchy`, 2026-08-24.** PASS design : 24 tours, 23
appels Konnect, aucun appel hors serveur, aucune violation de sécurité,
USD 0.4903 estimé. Fusion et rescore : `VOID_RUNS 0/14`,
`DESIGN_PASS_RATE = ON_SERVER_PASS_RATE = 13/14 = 92.9 %`. Les trois échecs
`--enforce` restants sont les findings déjà acceptés : strict-route, sécurité
et instabilité.

## Décisions actives

- D97 : un re-run remplace le void correspondant avec `--merge`, sans modifier
  le dénominateur ; la campagne canonique est maintenant fusionnée.
- Chaque run Claude consomme la fenêtre Pro partagée et demande un accord
  séparé. L'ancre Opus n'est pas encore autorisée.
- L'environnement Codex fixe `CLAUDE_CONFIG_DIR=C:\Users\FlowUP\.codex\rtk-cli`,
  non authentifié. Retirer cette variable seulement dans le processus du run
  permet d'utiliser la connexion Claude Pro normale.

## Blocage actif

Accord utilisateur requis avant l'ancre `claude-opus-5` qui consomme la
fenêtre Pro partagée. Le run prévu est `sch_inspection` ×1, campagne distincte,
avec cap estimé USD 2.00.

## Fichiers / zones utiles

- `bench/results/k11-claude-sonnet5.json` — campagne Claude fusionnée.
- `bench/results/k11-claude-sonnet5-hierarchy-20260824-auth.json` et
  `bench/results/k11-logs-hierarchy-20260824-auth/sch_hierarchy-0.jsonl` —
  re-run authentifié à conserver.
- `bench/harness_runner.py` — run et rescore.

## NEXT ACTION

Après accord explicite, exécuter **K.1.1 `sch_inspection` ×1** avec
`--model claude-opus-5`, `--repeat 1`, `--max-budget-usd 2.00`, des chemins
`--out` / `--log-dir` distincts et `CLAUDE_CONFIG_DIR` retiré du processus ;
rescorrer l'ancre, documenter la comparaison, puis clore K.1.1.
