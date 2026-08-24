# PROGRESS

## Phase actuelle

**Aucune. V1 est terminée.** Phase O (release et clôture) est close le
2026-08-24, après N et après toutes les phases du chemin critique (D, F, K, L,
M). Le projet n'a plus de tâche actionnable.

## Tâche actuelle

Aucune.

## Dernière tâche validée

**Phase O — V1 release & project closure.** Le dépôt a été audité (branche,
remotes, fichiers suivis, fuite de données, licences), validé, aligné sur ses
propres mesures, versionné, tagué et publié.

État de release :
- commits finaux sur `agentic/main`, poussés : `chore: prepare v1.0.0 release`,
  puis `fix: the schematic-viewer lock kept the old workspace versions` — la CI
  a attrapé ce que le gate local ne peut structurellement pas voir, le viewer
  étant exclu du workspace et portant son propre lock (O.7.3)
- tag : `v1.0.0`, annoté et poussé sur `58bc62f`, le commit que la CI
  32719802865 a validé (`git rev-list -n1 v1.0.0`). Les preuves de clôture
  écrites après la publication — dont cette section — sont postérieures au tag
  et ne le déplacent pas
- gate local : `.\gate.ps1` **PASSED** au bump de version (fmt, clippy
  `-D warnings`, 1 123 tests, doctests, build release). Le correctif de lock ne
  touche rien que le gate exécute ; il est vérifié comme la CI le vérifie,
  `cargo metadata --locked` contre le manifeste du viewer
- CI distante : **verte**, 7 jobs sur 7 (Format, Clippy, Check & Test sur
  windows/ubuntu/macos, Schematic viewer, PCM packaging validation)
- GitHub Release : **publiée** —
  https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.0.0, titre
  *KiCad Agentic MCP v1.0.0*, corps repris de `RELEASE_NOTES.md`. Workflow
  `Release` (run 32720207528) vert, 8 jobs sur 8
- artefacts : 7 — 4 binaires autonomes (linux-gnu, x86_64/aarch64 darwin,
  windows-msvc) et 3 paquets PCM validés contre le schéma `packages.v1` de
  KiCad avant upload. Le zip Windows a été rouvert après publication :
  `versions[]` à `1.0.0`/`stable`/`platforms ["windows"]`, aucun champ
  `download_*` inventé, 202 tools dans le manifeste du plugin, viewer inclus,
  et le binaire répond `konnect 1.0.0` à 21,8 MB
- `E2E (real KiCAD)` a tourné sur le tag (run 32720207516) sans le bloquer, et
  passe : *Full design loop* et *Live IPC against a running pcbnew*

Ce que la phase a corrigé, et rien d'autre : deux compteurs faux hérités de
N.1.6 (`packaging/metadata.json` et `plugin/plugin.json` disaient 185 tools au
lieu de 202), l'identité et les liens du README (il envoyait ses lecteurs vers
les releases d'upstream), l'URL affichée par les deux scripts PCM, et la
version `0.2.2 → 1.0.0` là où elle est réellement portée. Aucune feature,
aucune cible déplacée, aucun critère raté repeint en succès.

## Décisions actives

- Les critères V1 ratés restent ratés (INV6) : `WALL_CLOCK_P50` 86 ms contre
  77 ms, external tokens/task 2 249 contre ≤ 2 000, `tools/list` 2 831 contre
  ~1 000. `LLM_CALLS_PER_SUCCESSFUL_TASK` (15 → 5,5 dans le harnais model-fit)
  n'est pas revendiqué : aucune baseline n'a jamais été mesurée pour cette
  métrique.
- D99 : pas de `[profile.release]` — le binaire Windows reste à 21,8 MB non
  strippé, et c'est la taille que le README affiche.
- D98 : le projet peut consommer la capacité Claude nécessaire sans nouvel
  accord par run. D97 : un re-run remplace un void dans sa campagne.
- Les 187 tools cités par `decisions.md` D44 et `docs/capability-matrix.md`
  désignent la surface de la **baseline** à `5cd6454` : dénominateur gelé.
- `packaging/metadata.json`'s `versions[]` décrit encore les paquets v0.2.2
  d'upstream (URL et sha256 réels de ce dépôt-là). Laissé tel quel :
  `build-pcm.{ps1,sh}` n'en garde que la structure, stampe la version du tag et
  supprime les champs `download_*` avant d'écrire le zip. Ces entrées ne
  serviraient qu'à une soumission au dépôt kicad-addons, qui n'est pas dans V1.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `RELEASE_NOTES.md` — ce que V1 est, ce qu'elle mesure, ce qu'elle rate, ses
  limites connues. Source unique du corps de la GitHub Release.
- `docs/benchmark.md` — toutes les mesures ; `python bench/m1_table.py`
  régénère les tables M.1 depuis les artefacts committés sans rien exécuter.
- `.github/workflows/release.yml` — la méthode officielle de packaging : elle
  se déclenche sur un tag `v*`.
- `.\gate.ps1` — fmt, clippy, tests, doctests, build release.

## NEXT ACTION

**Aucune.** V1 est clôturée : commit final, tag `v1.0.0`, gate et CI verts,
GitHub Release publiée, aucune tâche actionnable restante.

Deux éléments restent ouverts par construction, et ni l'un ni l'autre n'est
actionnable sur cette machine :
- **D.5.3** reste conditionnelle — la capacité de 64 entrées du magasin de
  preuves ne sera reconsidérée que si une session réelle la sature.
- **I.1** reste conditionnée à **KiCad 11** — la réévaluation du chemin
  schematic IPC attend `kicad-cli api-server` et `kicad-python` 0.8.0 ; la
  position par défaut est toujours de ne pas forker KiCad (D3).

Reprise uniquement si (a) KiCad 11 est installé ici, ce qui débloque I.1, ou
(b) l'utilisateur ouvre explicitement une V2. Il n'y a pas de Phase P.
