# PROGRESS

## Phase actuelle

**R — Launch & adoption : IN PROGRESS.** Périmètre : **adoption, pas capacité**.
Branche `ai/R-launch-adoption`. Aucune release avant la clôture de R.

Closes : **R.1**, **R.2**, **R.5**, **R.7** (sauf R.7.7), **R.8**, **R.9**
(triage complet des cinq trouvailles du run 1), et **R.3.1 → R.3.3**, **R.3.5**,
**R.3.6**, **R.3.8**, **R.3.9**.

Restent : **R.3.4** et **R.3.7** (les prises chronométrées, qui dépensent du
quota), **R.3.10**, **R.4** (dépend de R.3 close), **R.6**, **R.7.7**.

## Tâche actuelle

Aucune tâche technique en cours. **Trois décisions utilisateur** bloquent la
suite, et rien d'autre :

1. **R.3.10 — ce que mesure le budget de 40 s.** Run 2 réussit la tâche et met
   **377 s** (47 tours, ~8–10 s par tour, onze `route_trace` d'un segment
   chacun). Le produit n'est pas lent : deux placements en 176 ms. C'est la
   *conversation* qui l'est. Deux issues honnêtes, au choix de l'utilisateur :
   re-viser les 40 s sur l'intervalle que le spectateur regarde (premier au
   dernier write dans KiCad, sous la seconde ici), ou publier le vrai nombre.
   Déplacer un budget publié pour coller à la mesure est ce qu'INV6 et D146
   interdisent.
2. **R.3.7 — le second run.** Il faut une prise supplémentaire depuis le
   pre-state committé pour prouver que la démo n'est pas un coup de chance.
   Le budget « une prise » est dépassé depuis le run 2 (1,83 USD).
3. **R.7.7 — une v1.1.1 à l'intérieur de R, ou non.** Les corrections de R.7,
   R.8 et maintenant R.9.1/R.9.2 n'atteignent un utilisateur que par un nouvel
   artefact, et F-03 voyagerait dans la même release.

## Dernière tâche validée

**R.9 — le triage des cinq trouvailles du run 1, close.**

- **R.9.1 (F-16)** — `launch_kicad_ui` ne trouvait pas KiCad sur la machine où
  KiCad est installé. La chaîne D149 déménage dans
  `konnect_core::kicad_locate` (partagée serveur + installeur) et couvre le
  binaire GUI. Prouvé de bout en bout par le principal, `kicad_binary` configuré
  nulle part : résolution au chemin d'installation standard, `launched: true`,
  processus `kicad` vivant ; cas négatif INV4 intact.
- **R.9.2 (F-14)** — un board sans section `(layers)` n'est plus « malformed » :
  `get_layer_list` rend le stackup par défaut de KiCad, `"declared": false`.
  Verdict de KiCad à l'appui : `kicad-cli pcb drc` ouvre ce board, 0 violation.
- **R.9.6** — trouvé en revue de R.9.2 : la première réponse rendait 2 layers,
  or `kicad-cli pcb upgrade` en écrit **24**, dont `Edge.Cuts`. Table mesurée
  dans `konnect_sexp::layers::default_stackup()`, liée à `canonical_id` par test.
- **R.9.3 (F-15)** — **décidé : R documente**. Les deux lectures de pads
  annoncent `"source": "file"`, `docs/TROUBLESHOOTING.md` porte le symptôme, le
  reroutage IPC devient candidat R.6.
- **R.9.4 (F-13)** et **R.9.5 (F-17)** — enregistrés, non corrigés, avec leur
  preuve ; candidats R.6. Le tableau des dispositions est dans
  `docs/launch/demo-run-1.md`.

**R.3.6** est close aussi : paire avant/après
(`resources/images/demo-{before,after}.png`), rendue par `kicad-cli pcb render`
au même zoom et au même pivot, dans le README au-dessus du Quick start. Sa
légende ne porte **aucune** revendication de temps — c'est R.3.10 qui l'écrira.

Validation : gate verte sur l'arbre modifié — `fmt`, `clippy -D warnings`,
**1 406 passed, 0 failed, 38 ignored sur 57 suites**, build release.

## Décisions actives

- **D150** — un défaut qui rendrait une réponse fausse plutôt qu'absente se
  corrige *ou* s'annonce. R.9.3 choisit d'annoncer : chaque lecture qui peut
  diverger de l'état live dit sa source (`"file"` / `"ipc"`), parce que
  rerouter tout le surface de lecture PCB sur IPC est de taille capacité, et
  que sauver le board de l'utilisateur sans le lui demander est pire.
- **D149** — la découverte d'un binaire externe est **une chaîne, pas un
  défaut** : configuré → `PATH` → préfixes → registre, l'échec final rendant la
  valeur inchangée pour que l'erreur du système survive. Une valeur configurée
  n'est jamais remplacée. Tri par version décroissante avant tri par préfixe.
  La chaîne vit dans `konnect_core::kicad_locate` et couvre `kicad-cli`,
  `kicad` et `ipc_address`.
- **D148** — R.1 a trouvé un seul défaut satisfaisant l'exception de la phase
  (découverte de `kicad-cli`) → lot R.7, corrigé.
- **D147** — la release publie sept assets et aucun fichier de sommes de
  contrôle (F-04). Classé *packaging*.
- **D146** — un chiffre public qu'une release ne remesure pas doit être remesuré
  sur l'artefact publié.
- **D145** — un test qui écrit puis relit un état horodaté attend la valeur
  observable du mtime, jamais une durée.
- **D144** — l'E2E gatante se lance à la main avant le tag, jamais après.
- **D143** — `RELEASE_NOTES.md` est le corps de la release courante.
- **D142 à D111** et les décisions V1 antérieures (INV6, D97…D101) : inchangées.
- Invariants de R : **INV-R1** l'artefact testé est celui qui est publié ;
  **INV-R2** une case = une preuve ; **INV-R3** tout problème est classé
  UX / packaging / documentation / configuration / produit avant correction ;
  **INV-R4** le parcours est consigné tel qu'un inconnu le vit.

## Blocage actif

Aucun blocage technique. Trois décisions utilisateur, listées dans
*Tâche actuelle*.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — R.1 à R.9.
- `examples/demo/` — pre-state (schématique + board), consigne, setup,
  vérification, et la commande qui régénère la paire d'images. **Ne pas éditer
  en place** : la démo tourne sur une copie.
- `docs/launch/demo-run-1.md` (les cinq trouvailles et leur disposition),
  `demo-run-2.md` (la prise réussie et l'histogramme des appels),
  `first-run-walk.md`.
- `crates/konnect-core/src/kicad_locate.rs` — la chaîne D149, partagée.
- Harnais des prises, préflight vert, dans le scratchpad de la session
  `bbc5fec2-…` : `r3-demo/` (`preflight.sh` gratuit, `run-demo2.sh` dépense,
  `verify.sh`), `demo2/` portant l'état final du run 2.

## Non-bloquants enregistrés, non traités

- macOS non signé et non notarisé ; les notes donnent la commande `xattr`.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9). Hors périmètre.
- F-03 : `packaging/metadata.json` renvoie vers le dépôt **amont**. À corriger
  avec la release de R.7.7.
- F-04 : aucune somme de contrôle publiée avec les sept assets.
- F-07 : la description d'`apply_template` affirme câbler les composants.
- F-11 : `plugin/plugin.json` déclare une action IPC API que KiCad 10 ne rend
  jamais.
- L'API KiCad est activée sur cette machine depuis R.1.11.
- Projet jetable de R.1 : `C:\Users\FlowUP\Documents\r1-walk-test\`.

## NEXT ACTION

Obtenir les trois décisions utilisateur (R.3.10, budget de R.3.7, R.7.7). Puis,
selon la réponse : lancer **R.3.7** (`bash r3-demo/run-demo2.sh <rundir>` puis
`verify.sh`), écrire ce que R.3.10 décide là où la démo est publiée, et ouvrir
**R.4** une fois R.3 close.
