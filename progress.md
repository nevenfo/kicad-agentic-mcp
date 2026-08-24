# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2, K.1.4, K.1.17 et K.1.18 sont closes. K.1 dépend
encore de l'ancre Opus de K.1.1 ; la phase M dépend de K.1.1. Les phases D, F
et L sont closes. La phase I reste conditionnée au matériel : cette machine a
KiCad 10.0, pas KiCad 11 / `kicad-cli api-server`.

## Tâche actuelle

**K.1.1 — ancre `claude-opus-5`.** Les campagnes principales sont complètes :
Codex 14/14, Claude Sonnet 14/14, aucun void. Deux essais Opus ont été coupés
avant tout tour modèle par l'incident upstream HTTP 529.

## Dernière tâche validée

**K.1.18 — erreurs harness classées void.** `harness_runner.py` classe désormais
`terminal_reason: api_error` comme interruption avec statut et cause compacte.
Validation : `py_compile` PASS ; les deux transcripts 529 et l'ancien échec
d'authentification deviennent void ; le transcript Sonnet complet reste
non-void.

## Décisions actives

- D98 : le projet peut consommer la capacité Claude nécessaire, quel que soit
  le modèle, sans nouvel accord par run. Les caps opérationnels peuvent être
  augmentés et les runs relancés automatiquement.
- D97 : un re-run remplace un void dans sa campagne ; l'ancre Opus reste une
  campagne distincte.
- Retirer `CLAUDE_CONFIG_DIR` seulement dans le processus du run pour utiliser
  la connexion Claude Pro normale.

## Blocage actif

Incident officiel Anthropic actif depuis 2026-08-24 07:27 CEST : panne partielle
API / Claude Code avec erreurs élevées sur Opus 5 et d'autres modèles. Deux
essais ont chacun épuisé 10 retries internes sur HTTP 529. Ne pas relancer
agressivement avant résolution officielle.

## Fichiers / zones utiles

- `bench/results/k11-claude-opus5-anchor{,-r2}.json` et dossiers de logs
  correspondants — essais 529 à conserver.
- `bench/harness_runner.py` — classification K.1.18.
- `bench/results/k11-claude-sonnet5.json` — campagne Sonnet complète.

## NEXT ACTION

Après résolution de l'incident officiel, relancer **K.1.1 `sch_inspection` ×1**
avec `--model claude-opus-5`, `--repeat 1`, un nouveau `--out` / `--log-dir` et
`CLAUDE_CONFIG_DIR` retiré ; rescorrer l'ancre, documenter la comparaison puis
clore K.1.1.
