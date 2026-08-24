# PROGRESS

## Phase actuelle

**Aucune phase ouverte sur le chemin critique.** K.1 et M.1 sont closes ce
2026-08-24, comme D, F, K et L avant elles. Ne restent que des tâches
conditionnelles : I.1 attend KiCad 11 (cette machine a KiCad 10.0, pas de
`kicad-cli api-server`) et D.5.3 attend un cas réel qui sature les 64 entrées
du magasin de preuves.

## Tâche actuelle

Aucune. Les prochaines décisions sont éditoriales ou dépendent du matériel :
soit ouvrir une phase de consolidation (README/DEV à jour avec les chiffres
M.1), soit attendre KiCad 11 pour I.1.

## Dernière tâche validée

**M.1 — les trois modes mesurés côte à côte.** `docs/benchmark.md` porte la
section *M.1 — Baseline vs Direct vs Agent*, regénérable par
`bench/m1_table.py` depuis les seuls artefacts committés.

- Baseline et Direct re-mesurés dos à dos le 2026-08-24, `--repeat 5`,
  35 runs chacun : **14 337 → 2 249 tokens externes par tâche (−84,3 %)**,
  appels MCP 11 → 4, 35/35 des deux côtés.
- Agent (H.7, `gpt-oss-20b` en loopback) : **2 aller-retours MCP par
  tentative**, la boucle compile/apply/verify restant côté serveur ; 2 548
  tokens externes par tentative contre 2 414 pour Direct. `model_ldo` en 1
  tentative, `model_divider` en 4. Aucun taux de succès revendiqué à n = 1.
- Deux critères V1 bougent **contre** le projet et restent enregistrés tels
  quels (INV6) : `WALL_CLOCK_P50` **86 ms contre 77** (nouveau manqué ; le
  mécanisme est visible par tâche — `recovery` +109 ms, `sch_inspection`
  −8 ms), et tokens externes/tâche **2 204 → 2 249**.

## Décisions actives

- D98 : le projet peut consommer la capacité Claude nécessaire, quel que soit
  le modèle, sans nouvel accord par run.
- D97 : un re-run remplace un void dans sa campagne.
- Un cap de budget ne doit jamais pouvoir voider sa propre mesure.
- Une colonne de comparaison se mesure le même jour que les autres : un
  artefact vieux de deux semaines fait entrer l'état machine dans un tableau
  censé parler de serveurs.
- Donner à la baseline son propre toolset (`--extra-toolset pcb_export`) n'est
  pas déplacer la cible : E8 a déplacé `export_bom` dans ce fork, et le fichier
  de tâche liste la taxonomie du fork.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `bench/m1_table.py` — toutes les tables M.1, sans rien exécuter.
- `bench/results/m1-{baseline-r5,gateway-r5,baseline-noextra,surface}.json` et
  `agent-e2e-gpt-oss-20b-medium-m1-{divider,ldo}.json` — les colonnes.
- `bench/runner.py --extra-toolset` — la taxonomie de l'autre serveur.
- `bench/agent_e2e.py` — enregistre désormais `surface` (mêmes formules que
  `runner.py`) et le temps par tentative.
- `bench/mcp_client.py::_resolve_program` — un `--server` relatif ne meurt plus
  dans `CreateProcess`.

## NEXT ACTION

Rien n'est bloqué et rien n'est en cours. Décider avec l'utilisateur ce qui
vient : consolidation documentaire (README/DEV/ROADMAP alignés sur les chiffres
M.1), ou attente de KiCad 11 pour I.1.
