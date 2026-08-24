# PROGRESS

## Phase actuelle

**Aucune.** N est close (N.1, N.2, N.3) le 2026-08-24. Toutes les phases du
chemin critique (D, F, K, L, M) le sont aussi. Ne restent que des tâches
conditionnelles, aucune actionnable sur cette machine : I.1 attend KiCad 11
(aucun KiCad n'est installé ici, donc pas de `kicad-cli api-server` à tester) et
D.5.3 attend un cas réel qui sature les 64 entrées du magasin de preuves.

## Tâche actuelle

Aucune.

## Dernière tâche validée

**N.3 — DEV.md a enfin une porte d'entrée vers la couche agentique.** Section
« The Agent Layer », 99 lignes, entre « Tool Routing » et « Build
Requirements ». Découpée par mécanisme et non par crate (N.3.1, arbitrage
utilisateur du 2026-08-24) : gateway, provider local + budget de contexte,
preuves et handles, world model, Plan IR, primitives d'état, puis le pont
(toolsets `plan`/`task`/`graph` et les méta-tools agent). Chaque mécanisme nomme
son crate, ses fichiers d'entrée et son adaptateur KiCAD dans `konnect-core` ;
le *pourquoi* reste dans `plan.md`/`decisions.md`, les mesures dans
`docs/benchmark.md`.

Validation :
- 12 chemins et 26 noms de fichiers `.rs` cités : tous existent sur disque
- 13 noms de tools et 13 symboles Rust cités : tous résolus dans `crates/`
- références corrigées contre les sources : les commentaires des crates citent
  un `D11` et une section « License impact » qui n'existent plus dans
  `plan.md` — la règle est INV2, et l'argument toolset-plutôt-que-verbe est
  E.4.4 (D20)
- aucun fichier Rust modifié : le gate vert de N.1 couvre toujours cet état

## Décisions actives

- D99 : pas de `[profile.release]` — le binaire reste à 21,8 MB non strippé, le
  README porte la taille mesurée (arbitrage N.1.8 du 2026-08-24).
- D98 : le projet peut consommer la capacité Claude nécessaire, quel que soit le
  modèle, sans nouvel accord par run.
- D97 : un re-run remplace un void dans sa campagne.
- Un cap de budget ne doit jamais pouvoir voider sa propre mesure.
- Une colonne de comparaison se mesure le même jour que les autres.
- Une dérive qui s'est déjà produite se ferme par un test, pas par une ligne de
  checklist (verrou `find_capabilities` ↔ registry, comme
  `registry_tool_counts_match_reality`).
- Les 187 tools cités par `decisions.md` D44 et `docs/capability-matrix.md`
  désignent la surface de la **baseline** à `5cd6454` : dénominateur gelé (INV6).

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `DEV.md` — « Architecture » (arbre du workspace), « Tool Routing », « The
  Agent Layer », « Current Stats » : les quatre endroits qui redisent la
  surface ou la structure et qui ont dérivé par le passé.
- `crates/konnect-core/src/router/registry.rs::ALL_TOOLSETS` — les `tool_count`
  faisant autorité ; `STARTER_KIT` + `STARTER_TOOLS` expliquent les 21 tools du
  démarrage.
- `bench/results/m1-surface.json` — 215 noms de tools et leur coût, sans rien
  exécuter ; `bench/m1_table.py` régénère les tables M.1 depuis les artefacts
  committés.
- `.\gate.ps1` — fmt, clippy, tests, doctests, build release.

## NEXT ACTION

Aucune action autonome : le plan n'a plus de tâche exécutable sur cette machine.
Reprise possible seulement si (a) KiCad 11 est installé, ce qui débloque I.1, ou
(b) l'utilisateur ouvre une phase O avec un objectif neuf. Attendre cette
entrée plutôt que d'élargir la portée seul.
