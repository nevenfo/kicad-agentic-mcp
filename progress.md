# PROGRESS

## Phase actuelle

**N — Documentation consolidation.** Ouverte le 2026-08-24 après M.1. Les
phases du chemin critique (D, F, K, L, M) sont closes ; ne restent en dehors de
N que des tâches conditionnelles : I.1 attend KiCad 11 (aucun KiCad n'est
installé sur cette machine, donc pas de `kicad-cli api-server` à tester) et
D.5.3 attend un cas réel qui sature les 64 entrées du magasin de preuves.

## Tâche actuelle

Aucune en cours. N.3 est ouverte mais demande d'abord un arbitrage de portée
(voir NEXT ACTION).

## Dernière tâche validée

**N.2 — l'arborescence de `DEV.md` couvre enfin tout le workspace.** Les huit
crates ajoutés par les phases E, G et H y figuraient absents (`kam-context`,
`kam-evidence`, `kam-graph`, `kam-llm`, `kam-runtime`, `kam-plan`, `kam-state`,
`konnect-schematic-editor`), ainsi que quatre toolsets (`sch_buses`, `plan`,
`task`, `graph`).

Validation :
- 12/12 `members` de `Cargo.toml` présents ; les 102 entrées de l'arbre existent
  toutes sur disque
- 22/22 annotations `# N tools` conformes à `registry.rs` ; `meta_tools.rs`
  corrigé 6 → 13, `sch_export.rs` 6 → 7
- aucun fichier Rust modifié depuis le gate vert de N.1, donc `.\gate.ps1`
  reste valide pour cet état

**N.1 — les chiffres publics étaient tous faux dans le même sens**, commit
`c8136ef`, poussé. Vérité mesurée, tracée à `bench/results/m1-surface.json` et
à `router/registry.rs` : 202 tools / 22 toolsets (+13 méta = 215 au catalogue),
`tools/list` au démarrage 21 tools / 2 831 tokens, catalogue complet 33 183.
Corrigé dans README, DEV.md, `tool-directory.md`, le skill livré aux
utilisateurs et `packaging/metadata.json`.

Le cinquième endroit n'était pas de la documentation : `find_capabilities`
annonçait « Search all 196 KiCAD tools » à l'intérieur de `tools/list`, donc lu
et payé à chaque session. Son corpus est `all_tools_with_toolset()` — 202.

Validation :
- `.\gate.ps1` : PASS (fmt, clippy, tests, doctests, build release), 0 échec
- `router::tests::find_capabilities_description_quotes_the_real_corpus_size` :
  vert à 202, rouge à 203 (les deux sens vérifiés)
- `tool-directory.md` : 215/215 tools présents, 22 sections, chaque table au
  `tool_count` déclaré

## Décisions actives

- D98 : le projet peut consommer la capacité Claude nécessaire, quel que soit
  le modèle, sans nouvel accord par run.
- D97 : un re-run remplace un void dans sa campagne.
- Un cap de budget ne doit jamais pouvoir voider sa propre mesure.
- Une colonne de comparaison se mesure le même jour que les autres.
- Une dérive qui s'est déjà produite se ferme par un test, pas par une ligne de
  checklist : `CONTRIBUTING.md` avertissait déjà, et la dérive a recommencé sur
  cinq emplacements. Le compteur en dur de `find_capabilities` est désormais
  verrouillé sur le registry, comme `registry_tool_counts_match_reality`.
- Les 187 tools cités par `decisions.md` D44 et `docs/capability-matrix.md`
  désignent la surface de la **baseline** à `5cd6454` : dénominateur gelé, il ne
  bouge pas (INV6).

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-core/src/router/registry.rs::ALL_TOOLSETS` — les `tool_count`
  faisant autorité ; `STARTER_KIT` (`project`) et `STARTER_TOOLS` (2 tools
  `config`) expliquent les 21 du démarrage.
- `crates/konnect-core/src/router/mod.rs` — les tests d'invariants du registry,
  dont le nouveau verrou sur `find_capabilities`.
- `bench/results/m1-surface.json` — 215 noms de tools et leur coût, sans rien
  exécuter.
- `bench/m1_table.py` — toutes les tables M.1 depuis les seuls artefacts
  committés.
- `Cargo.toml` `members` — la liste que l'arbre de DEV.md doit couvrir.

## NEXT ACTION

Deux décisions utilisateur, aucune action autonome sûre :

1. **N.3.1 — profondeur** de la documentation de la couche agentique dans
   DEV.md : simple section « où vit la couche agent » pointant vers les crates,
   ou section d'architecture complète par crate. `plan.md`, `decisions.md` et
   `docs/benchmark.md` couvrent déjà le *pourquoi* et les mesures ; les
   dupliquer serait pire que le manque.
2. **N.1.8 — binaire de 21,8 MB** : ajouter ou non un `[profile.release]`
   strip/LTO dans `Cargo.toml`. C'est un changement de build, pas éditorial.
