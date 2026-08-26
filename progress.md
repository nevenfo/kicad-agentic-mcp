# PROGRESS

## Phase actuelle

**R — Launch & adoption : IN PROGRESS.** Ouverte le 2026-08-26 sur demande
explicite de l'utilisateur, juste après la publication de v1.1.0 (phase Q close).
Périmètre : **adoption, pas capacité**. Aucun refactor, aucune feature
opportuniste, aucun travail KiCad 11, aucun Dependabot / signature macOS / dépôt
d'addons officiel sauf blocage réel de R.

Branche : `ai/R-launch-adoption`, ouverte sur `90d0928`.

**R.1 est close** (onze cases, onze preuves) et **R.7 est close à une case
près** : le seul défaut produit bloquant est corrigé et prouvé. Reste R.7.7, qui
n'est pas un travail mais une **décision utilisateur**.

## Décisions utilisateur du 2026-08-26

- **Aucune release avant la clôture de R.** Les corrections de R.7 et R.8
  partiront dans une **seule v1.1.1** en fin de phase, pas une release par
  trouvaille. Le README dit franchement que v1.1.0 exige les étapes manuelles.
- **F-12 est corrigé** (lot R.8), même traitement que R.7.
- **Budget démo : une prise.** Toute dépense supplémentaire repasse par
  l'utilisateur.

## Tâche actuelle

**R.3.4 — la prise unique.** R.8 est close et validée par le principal.
**R.3.1 est tranchée** : la démo
est une **édition PCB live par l'API IPC, regardée dans le canevas de KiCad**.
Mesuré avant de choisir : deux `place_component` posent deux empreintes 0805 sur
le board ouvert en **176 ms**, chacune répondant `"source": "ipc"` — l'écriture
atteint pcbnew, pas le fichier. Candidats écartés et raisons dans `plan.md`.

Reste R.3.2 à R.3.7 : projet de départ committé, consigne, exécution chronométrée
et capture. **L'exécution pilotée par un modèle consomme du quota** et attend une
décision de l'utilisateur.

En attente d'une décision utilisateur, **R.7.7** : la correction de la
découverte de `kicad-cli` n'atteint un utilisateur que par un nouvel artefact, et
F-03 (`packaging/metadata.json` renvoyant vers le dépôt amont) voyagerait dans la
même release. Décider une v1.1.1 à l'intérieur de R appartient à l'utilisateur ;
en attendant, la correction vit sur la branche et ne bloque pas R.3.

## Dernière tâche validée

**R.7.1 à R.7.6 et R.7.8 — le serveur trouve KiCad là où KiCad s'installe.**

Chaîne de résolution partagée entre le serveur et `install::detect_kicad()`,
dans `install::resolve_binary` : (1) valeur configurée explicite, utilisée telle
quelle et **jamais** remplacée ; (2) nom nu par défaut trouvé sur `PATH` ;
(3) préfixes d'installation connus, désormais y compris
`%LOCALAPPDATA%\Programs\KiCad\<ver>\bin\` ; (4) registre, `HKLM\SOFTWARE\KiCad`
puis la clé de désinstallation `HKCU`/`HKLM` → `InstallLocation`. Rien trouvé →
valeur inchangée, l'échec de spawn reste bruyant.

Validé par le principal, pas repris du worker :
- gate sur l'arbre final — `fmt` propre, `clippy --workspace --locked
  --all-targets -- -D warnings` silencieux, suite complète à **1 392 passés,
  0 échec, 38 ignorés sur 57 suites** (1 385 à la v1.1.0, plus les sept tests
  de ce lot)
- preuve de bout en bout, binaire release, aucun `kicad_cli` configuré :
  `kicad_cli: found at standard install path -> …\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`
  au démarrage, puis `verify:"auto"` → `{"check":"erc","errors":0,"warnings":0}`
- preuve négative, INV4 : un `kicad_cli` configuré à `konnect-no-such-kicad-cli`
  est journalisé *using configured value as-is* et échoue toujours sur
  `Failed to spawn kicad-cli` — aucune substitution silencieuse introduite

Deux défauts trouvés en relecture et corrigés dans le lot :
- le worker avait d'abord fait retomber **toute** valeur non résolue vers le
  registre, ce qui remplaçait un `kicad_cli` explicitement bidon par le vrai
  binaire ; le test préexistant
  `a_validator_that_could_not_run_is_an_error_not_a_pass` l'a attrapé
- **R.7.8** : la liste de candidats était ordonnée préfixe d'abord, si bien
  qu'un KiCad 9 installé pour tous l'emportait sur un KiCad 10 par utilisateur.
  Réordonnée version d'abord, avec un test qui l'exige, et la même inversion
  corrigée dans `plugin/settings_dialog.py` — sinon le dialogue du plugin et le
  serveur auraient pu désigner deux KiCad différents

## Décisions actives

- **D149** — la découverte d'un binaire externe est **une chaîne, pas un
  défaut** : configuré → `PATH` → préfixes → registre, et l'échec final rend la
  valeur inchangée pour que l'erreur du système survive. Corollaire tiré du
  rouge de cette phase : une valeur configurée explicitement n'est **jamais**
  remplacée, même invalide — sinon un appelant croit avoir testé ce qu'il a
  nommé. Corollaire d'ordonnancement : les candidats se trient par version
  décroissante avant de se trier par préfixe.

- **D148** — R.1 a trouvé **un seul** défaut satisfaisant l'exception de la
  phase : le serveur ne découvrait pas `kicad-cli` sur une installation KiCad
  Windows par défaut, ce qui faisait échouer ERC, DRC, tous les exports et
  `verify:"auto"` à la première utilisation. Classé **produit, bloquant** →
  lot **R.7**, corrigé.

- **D147** — la release publie sept assets et **aucun fichier de sommes de
  contrôle** (F-04). Classé *packaging*.

- **D146** — un chiffre public qu'une release ne remesure pas doit être remesuré
  **sur l'artefact publié**. L'unité du dépôt est le MiB, écrit « MB ».

- **D145** — un test qui écrit puis relit un état horodaté attend la **valeur
  observable** du mtime, jamais une durée. Corollaire : un mutex de test gardant
  une seule variable d'environnement se prend avec `into_inner()`.

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

Aucun.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — R.1 à R.6, plus **R.7** ouvert par R.1.
- `docs/launch/first-run-walk.md` — le parcours, les preuves, les dix frictions.
- `crates/konnect/src/install.rs` — `resolve_binary`, `kicad_standard_paths`,
  `detect_kicad_from_registry`, `KicadCliSource`, et `mod resolve_binary_tests`.
- `crates/konnect/src/main.rs::resolve_and_log` — la résolution au démarrage,
  journalisée une fois par binaire.
- `plugin/settings_dialog.py::detect_kicad_cli` — même ordre, mêmes emplacements.
- `packaging/metadata.json` — auteur `mixelpixx`, homepage
  `github.com/mixelpixx/Konnect` : le Plugin Manager renvoie le premier
  utilisateur vers le dépôt **amont** (F-03).
- `plugin/plugin.json` — déclare une action IPC API `show-button: true` que
  KiCad 10 ne rend jamais (F-11) ; seul l'Action Plugin SWIG d'`__init__.py`
  fonctionne, et il disparaît en KiCad 11.
- Travail de R.1 et R.7 :
  `%LOCALAPPDATA%\Temp\claude\C--Users-FlowUP-kicad-agentic-mcp-konnect-agentic\ab608642-35fc-4d58-b755-c2e65a52c322\scratchpad\r1-walk\`
  (`mcp.sh` pointe le binaire **installé**, `mcp-new.sh` le binaire construit,
  `mcp-bogus.sh` la preuve négative).
- Projet de test réel : `C:\Users\FlowUP\Documents\r1-walk-test\`.

## Non-bloquants enregistrés, non traités

- macOS non signé et non notarisé ; les notes donnent la commande `xattr`.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9). Hors périmètre.
- Dépôt public à **0 étoile, 0 issue, aucun topic, aucune homepage, Discussions
  désactivées** — ligne de base d'adoption que R.4 et R.5 déplacent.
- F-07 : la description d'`apply_template` affirme câbler les composants ; elle
  les place et rend la liste des connexions à faire. **Produit non bloquant**,
  laissé tel quel par R.
- Le board `r1-walk-test.kicad_pcb` ouvert dans KiCad porte deux empreintes 0805
  posées par IPC pendant la mesure de R.3.1, **non enregistrées**. Projet jetable ;
  ne pas le confondre avec le projet de départ que R.3.2 committera.
- L'API KiCad est activée sur cette machine depuis R.1.11 (case *Activer l'API
  KiCad*, socket `ipc://…\Temp\kicad\api.sock`). L'état initial ne l'avait pas.

## NEXT ACTION

Trois décisions utilisateur attendent, aucune ne bloque les autres lots :
**R.7.7** (une v1.1.1 ou non), **F-12** (dériver `ipc_address` comme R.7 l'a fait
pour `kicad-cli`, ou le laisser en configuration manuelle documentée), et le
**budget** de l'exécution pilotée par modèle de R.3.4. En attendant, préparer
**R.3.2** et **R.3.3** : le board de départ committé sous `examples/` et la
consigne exacte, tous deux vérifiables sans dépense.
