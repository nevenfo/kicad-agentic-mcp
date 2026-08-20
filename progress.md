# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2, K.1.4 et K.1.17 sont closes. K.1 dépend encore
de K.1.1 ; la phase M dépend de K.1.1. Les phases D, F et L sont closes. La
phase I reste conditionnée au matériel : cette machine a KiCad 10.0, pas
KiCad 11 / `kicad-cli api-server`.

## Tâche actuelle

**K.1.1 — campagne multi-harness.** Codex : 14/14, aucun void. Claude
(`claude-sonnet-5`) : 13/14 scorés, seul `sch_hierarchy` reste void. Le run
précédent non suivi a construit presque tout le design mais Claude l’a interrompu
sur le quota hebdomadaire ; il reste non fusionnable.

## Dernière tâche validée

**Préparation du dernier re-run `sch_hierarchy`.** `cargo build --release -p
konnect` : PASS ; le binaire absolu
`C:\Users\FlowUP\kicad-agentic-mcp\konnect-agentic\target\release\konnect.exe`
est disponible. L’état Git initial ne contient aucune modification suivie ; les
deux fichiers non suivis ci-dessous préexistaient à cette reprise et restent
intacts.

## Décisions actives

- D97 : un re-run remplace le void correspondant avec `--merge`, sans modifier
  le dénominateur ; l’ancre Opus reste une campagne distincte.
- Chaque run Claude consomme la fenêtre Pro partagée et demande l’accord de
  l’utilisateur. L’accord du 2026-08-21 portait sur ce re-run Sonnet seulement.
- Le re-run utilise `claude-sonnet-5`, `--repeat 1`, cap estimé USD 2.00 et un
  `--log-dir`. `--rescore` / `--merge` ne dépensent rien.

## Blocage actif

Claude a rejeté le run précédent sur la fenêtre `seven_day`, avec réouverture
annoncée le **2026-08-24 à 03:00**. Avant cette échéance, un nouvel essai ne
ferait que produire un autre void. Aucun autre travail de phase M n’est
indépendant de K.1.1.

## Fichiers / zones utiles

- `bench/results/k11-claude-sonnet5.json` — campagne Claude à fusionner.
- `bench/results/k11-claude-sonnet5-hierarchy.json` et
  `bench/results/k11-logs/sch_hierarchy-0.jsonl` — tentative quota rejetée,
  non suivie et à préserver.
- `bench/harness_runner.py` — run, `--merge`, puis `--rescore --enforce`.
- `bench/runner.py` — audit et seuils ; `decisions.md` — D35–D97.

## NEXT ACTION

Après le **2026-08-24 03:00**, exécuter **K.1.1 `sch_hierarchy` ×1** avec le
binaire absolu, `--model claude-sonnet-5`, `--repeat 1`,
`--max-budget-usd 2.00`, un nouveau `--out` et un nouveau `--log-dir` ; si le
run est non void, le fusionner dans `bench/results/k11-claude-sonnet5.json`,
puis lancer `--rescore --enforce`. Demander ensuite séparément l’accord pour
l’ancre `claude-opus-5`.
