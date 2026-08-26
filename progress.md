# PROGRESS

## Phase actuelle

**R — Launch & adoption : IN PROGRESS.** Ouverte le 2026-08-26, juste après la
publication de v1.1.0 (phase Q close). Périmètre : **adoption, pas capacité**.
Aucun refactor, aucune feature opportuniste, aucun travail KiCad 11, aucun
Dependabot / signature macOS / dépôt d'addons officiel sauf blocage réel de R.

Branche : `ai/R-launch-adoption`, ouverte sur `90d0928`.

Closes et committées : **R.1** (le parcours d'installation, onze preuves),
**R.2** (README et Quick start), **R.7** sauf R.7.7, **R.8** (dérivation de
`ipc_address`), **R.5** (route de feedback : trois formulaires, `docs/adoption.md`,
Discussions décidées *off*), et **R.3.1 à R.3.3** (la tâche de démo choisie et
mesurée, le board de départ et la consigne committés sous `examples/demo/`).

Restent : **R.3.4 à R.3.7** (l'exécution chronométrée, qui dépense du quota),
**R.4** (kit de lancement, dépend de R.3 close), **R.6** (la porte de décision),
et **R.7.7**, qui est une décision utilisateur, pas un travail.

## Décisions utilisateur du 2026-08-26

- **Aucune release avant la clôture de R.** Les corrections de R.7 et R.8
  partiront dans une **seule v1.1.1** en fin de phase, pas une release par
  trouvaille. Le README dit franchement que v1.1.0 exige les étapes manuelles.
- **F-12 est corrigé** (lot R.8), même traitement que R.7. Fait.
- **Budget démo : une prise.** Toute dépense supplémentaire repasse par
  l'utilisateur.

## Tâche actuelle

**R.3.4 — la prise chronométrée.** Le harnais est monté et son **préflight est
vert sans rien dépenser** :

- binaire **publié** v1.1.0 (INV-R1) :
  `…\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect\bin\konnect.exe`,
  lancé avec un `--config` qui nomme `kicad_cli` et `ipc_address` à la main,
  comme le README de la démo le documente pour v1.1.0 ;
- `pcbnew` tourne sur une **copie** du pre-state committé (même md5 que
  `examples/demo/konnect-demo.kicad_pcb`, 739 o) ; `open_project` répond
  `kicad_ui_running: true, IPC is available` à travers ce binaire ;
- `find_capabilities` sur la tâche rend `place_component`, `route_pad_to_pad`,
  `route_trace` — le chemin outillé existe.

Ce que la prise exécutera : `claude -p` avec la **consigne committée telle
quelle**, `--mcp-config` sur konnect, une allowlist réduite à
`mcp__konnect Glob Read` — donc **toute écriture passe par KiCad**, aucune par
le disque — sortie `stream-json` pour garder la liste ordonnée des outils
appelés, et un chronomètre externe en plus du `duration_ms` du run.

**Deux décisions utilisateur bloquent, et rien d'autre :**

1. **Le budget.** R.3.4 est la prise budgétée. Mais **R.3.7 exige un second run**
   depuis le même pre-state pour prouver que la démo n'est pas un coup de chance :
   cela fait **deux** prises, pas une.
2. **R.7.7** : une v1.1.1 à l'intérieur de R, ou non. La correction de R.7 et
   celle de R.8 n'atteignent un utilisateur que par un nouvel artefact, et F-03
   (`packaging/metadata.json` renvoyant vers le dépôt amont) voyagerait dans la
   même release.

## Dernière tâche validée

**R.5 — une route de feedback qui répond aux questions du mainteneur.**

Trois formulaires d'issue (`.github/ISSUE_TEMPLATE/`) remplacent un tracker vide.
Le *first-run report* fait six questions, la plupart à un clic, et produit les
cinq métriques que `docs/adoption.md` tallie : installation réussie, temps
jusqu'à la première tâche, premier blocage, tâche tentée, succès ou échec.
Chaque métrique a un *inconnu* explicite — une réponse manquante ne doit jamais
se lire comme une bonne. La route est liée depuis les trois endroits où l'on
échoue : fin du Quick start, fin de `TROUBLESHOOTING`, notes de release.
Discussions reste **off**, raison consignée. `docs/adoption.md` fixe aussi la
ligne de base du lancement (0 étoile, 0 issue, 0 rapport, 1 téléchargement — le
mainteneur) et affirme qu'aucune télémétrie n'existe ni n'est prévue.

## Décisions actives

- **D149** — la découverte d'un binaire externe est **une chaîne, pas un
  défaut** : configuré → `PATH` → préfixes → registre, et l'échec final rend la
  valeur inchangée pour que l'erreur du système survive. Une valeur configurée
  explicitement n'est **jamais** remplacée, même invalide. Les candidats se
  trient par version décroissante avant de se trier par préfixe. R.8 applique la
  même forme à `ipc_address` : explicite → `KICAD_API_SOCKET` → défaut de
  plateforme `<temp>/kicad/api.sock`, construit et non testé sur le disque, car
  sous Windows c'est un *named pipe*, pas un fichier.

- **D148** — R.1 a trouvé **un seul** défaut satisfaisant l'exception de la
  phase (découverte de `kicad-cli`), classé **produit, bloquant** → lot **R.7**,
  corrigé.

- **D147** — la release publie sept assets et **aucun fichier de sommes de
  contrôle** (F-04). Classé *packaging*.

- **D146** — un chiffre public qu'une release ne remesure pas doit être remesuré
  **sur l'artefact publié**. L'unité du dépôt est le MiB, écrit « MB ».

- **D145** — un test qui écrit puis relit un état horodaté attend la **valeur
  observable** du mtime, jamais une durée.

- **D144** — l'E2E gatante se lance **à la main avant le tag**, jamais après.

- **D143** — `RELEASE_NOTES.md` est le corps de la release **courante**, pas un
  changelog cumulatif.

- Les décisions **D142 à D111** et les décisions V1 antérieures (INV6, D97…D101)
  restent actives, inchangées.

- Invariants propres à R : **INV-R1** l'artefact testé est celui qui est publié ;
  **INV-R2** une case = une preuve ; **INV-R3** tout problème est classé
  UX / packaging / documentation / configuration / produit **avant** correction ;
  **INV-R4** le parcours est consigné tel qu'un inconnu le vit, détours compris.

## Blocage actif

Aucun blocage technique. Deux décisions utilisateur en attente, listées dans
*Tâche actuelle* : le budget des prises de R.3.4/R.3.7, et R.7.7.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — R.1 à R.6, plus R.7 et R.8.
- `examples/demo/` — le pre-state (739 o), la consigne, la procédure de setup et
  de vérification. **Ne pas éditer en place** : la démo tourne sur une copie.
- `docs/launch/first-run-walk.md` — le parcours, les preuves, les dix frictions.
- `docs/adoption.md`, `.github/ISSUE_TEMPLATE/` — la route de feedback de R.5.
- `crates/konnect/src/install.rs` (`resolve_binary`), `crates/konnect/src/config.rs`
  (`default_ipc_address`), `crates/konnect-core/src/tools/ipc_boundary.rs`.
- Harnais de R.3.4, monté et préflight vert :
  `%LOCALAPPDATA%\Temp\claude\C--Users-FlowUP-kicad-agentic-mcp-konnect-agentic\bbc5fec2-bbe6-47b5-9b5e-dc813c244cf2\scratchpad\r3-demo\`
  (`preflight.sh` gratuit, `run-demo.sh <rundir> [modèle]` dépense,
  `verify.sh <rundir>` sauve par IPC puis fait trancher `kicad-cli pcb drc`).
  `run1/` porte la copie du pre-state, ouverte dans `pcbnew`.

## Non-bloquants enregistrés, non traités

- macOS non signé et non notarisé ; les notes donnent la commande `xattr`.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9). Hors périmètre.
- F-03 : `packaging/metadata.json` renvoie le premier utilisateur vers le dépôt
  **amont** (`github.com/mixelpixx/Konnect`). À corriger avec la release de R.7.7.
- F-04 : aucune somme de contrôle publiée avec les sept assets.
- F-07 : la description d'`apply_template` affirme câbler les composants ; elle
  les place et rend la liste des connexions à faire. **Produit non bloquant**.
- F-11 : `plugin/plugin.json` déclare une action IPC API `show-button: true` que
  KiCad 10 ne rend jamais ; seul l'Action Plugin SWIG fonctionne, et il disparaît
  en KiCad 11.
- L'API KiCad est activée sur cette machine depuis R.1.11. L'état initial ne
  l'avait pas.
- Projet jetable de R.1 : `C:\Users\FlowUP\Documents\r1-walk-test\`, dont le board
  porte deux empreintes 0805 non enregistrées, posées pendant la mesure de R.3.1.
  Ne pas le confondre avec le pre-state de la démo.

## NEXT ACTION

Obtenir la décision de budget, puis lancer **R.3.4** :
`bash r3-demo/run-demo.sh r3-demo/run1`, chronométrer, puis
`bash r3-demo/verify.sh r3-demo/run1` pour **R.3.5**. Si le mur des 40 s est
franchi, la tâche se rétrécit — le budget de temps ne bouge pas (INV6).
