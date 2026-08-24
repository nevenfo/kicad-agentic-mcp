# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2, K.1.4 et K.1.17 sont closes. K.1 dépend encore
de K.1.1 ; la phase M dépend de K.1.1. Les phases D, F et L sont closes. La
phase I reste conditionnée au matériel : cette machine a KiCad 10.0, pas
KiCad 11 / `kicad-cli api-server`.

## Tâche actuelle

**K.1.1 — campagne multi-harness.** Codex : 14/14, aucun void. Claude
(`claude-sonnet-5`) : 13/14 scorés, seul `sch_hierarchy` reste void dans la
campagne fusionnée.

## Dernière tâche validée

**Diagnostic du re-run `sch_hierarchy` du 2026-08-24.** Le runner et Konnect
ont démarré, mais Claude a répondu en 31 ms `Not logged in · Please run /login`.
`claude auth status` confirme `loggedIn: false`, `authMethod: none`. Le run a
coûté USD 0.00 et n'a pas été fusionné.

## Décisions actives

- D97 : un re-run remplace le void correspondant avec `--merge`, sans modifier
  le dénominateur ; l'ancre Opus reste une campagne distincte.
- Chaque run Claude consomme la fenêtre Pro partagée et demande l'accord de
  l'utilisateur. L'accord du 2026-08-21 porte sur ce re-run Sonnet ; la
  tentative d'authentification échouée du 2026-08-24 n'a rien dépensé.
- Le prochain essai conserve `claude-sonnet-5`, `--repeat 1`, le cap USD 2.00,
  le binaire absolu et de nouveaux chemins `--out` / `--log-dir`.

## Blocage actif

Claude Code n'est plus authentifié. Une personne doit exécuter `claude /login`
et terminer l'authentification interactive avant le re-run. Ne pas fusionner
`k11-claude-sonnet5-hierarchy-20260824.json` : il contient seulement l'échec
d'authentification, que le runner classe actuellement comme non-void.

## Fichiers / zones utiles

- `bench/results/k11-claude-sonnet5.json` — campagne Claude à fusionner.
- `bench/results/k11-claude-sonnet5-hierarchy.json` et
  `bench/results/k11-logs/sch_hierarchy-0.jsonl` — tentative quota rejetée.
- `bench/results/k11-claude-sonnet5-hierarchy-20260824.json` et
  `bench/results/k11-logs-hierarchy-20260824/sch_hierarchy-0.jsonl` — échec
  d'authentification, non fusionnable.
- `bench/harness_runner.py` — run, `--merge`, puis `--rescore --enforce`.

## NEXT ACTION

Après `claude /login`, vérifier **K.1.1** avec `claude auth status`, puis relancer
`sch_hierarchy` ×1 avec les paramètres décidés et de nouveaux chemins ; si le
run est exploitable, le fusionner dans `bench/results/k11-claude-sonnet5.json`
et lancer `--rescore --enforce`.
