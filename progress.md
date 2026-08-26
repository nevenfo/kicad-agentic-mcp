# PROGRESS

## Phase actuelle

**R — Launch & adoption : IN PROGRESS.** Ouverte le 2026-08-26 sur demande
explicite de l'utilisateur, juste après la publication de v1.1.0 (phase Q close).
Périmètre : **adoption, pas capacité**. Aucun refactor, aucune feature
opportuniste, aucun travail KiCad 11, aucun Dependabot / signature macOS / dépôt
d'addons officiel sauf blocage réel de R.

Branche : `ai/R-launch-adoption`, ouverte sur `90d0928`.

**R.1 est walké** : release → installation → connexion MCP → projet KiCad réel →
première tâche → verdict de KiCad. Le compte rendu complet, avec la liste de
frictions classée, est `docs/launch/first-run-walk.md`. Une seule case de R.1
reste ouverte, **R.1.4**, faute de preuve GUI.

## Tâche actuelle

**R.1.11 — vérifier le plugin là où KiCad le montre réellement.** R.1.4 posait
la mauvaise question : le README envoie vers *Tools → External Plugins*, qui est
le mécanisme des anciens Action Plugins SWIG, alors que le `plugin.json` de
Konnect déclare un plugin **IPC API** (`runtime.type: "exec"`). KiCad expose
ceux-là comme **bouton de barre d'outils** de l'éditeur de PCB et sous
*Preferences → Plugins*, et ils restent **inertes tant que l'API n'est pas
activée** — or KiCad la livre désactivée (`api.enable_server: false`, mesuré
dans `kicad_common.json`). R.1.4 reste ouverte et non prouvée (INV-R2), trois
tentatives ayant été interrompues par un dialogue UAC étranger à KiCad.

## Dernière tâche validée

**R.1.1 à R.1.10 — le parcours d'un inconnu, mesuré.**

- État initial, non reproductible une fois l'installation faite : `3rdparty\`
  **vide**, KiCad **10.0.3** installé **par utilisateur** dans
  `%LOCALAPPDATA%\Programs\KiCad\10.0\`, aucun client MCP ne connaissant
  `konnect`.
- Artefact publié épinglé : `konnect-pcm-v1.1.0-windows.zip`, 12 258 180 octets,
  SHA-256 `25fe29ca…67dd0`. Le `konnect.exe` **installé** est identique octet
  pour octet à celui du zip publié (`57f272cb…1868c`) et répond `konnect 1.1.0`.
- Chemin d'installation lu **sur le disque** :
  `…\3rdparty\plugins\com_github_mixelpixx_konnect\bin\konnect.exe` — identique
  caractère pour caractère à ce que publient le README et les deux fichiers
  `examples/`.
- Connexion MCP prouvée par un handshake réel : `protocolVersion 2025-06-18`,
  `serverInfo konnect 1.1.0`, **21 outils** au démarrage — le chiffre annoncé.
- Première tâche sur un vrai projet créé par KiCad : `apply_template ldo_3v3`
  via la gateway, **108 ms**, 5 symboles placés, schéma de 230 → 2 576 octets.
- Verdict de KiCad obtenu **à la main** : `kicad-cli sch erc` → *0 violation*,
  exit 0. Le serveur, lui, n'a pas pu le produire (voir R.7).

Neuf frictions consignées et classées (INV-R3) : F-01 produit, F-02 doc,
F-03 UX, F-04 packaging, F-05 doc, F-06 doc, F-07 produit, F-08 UX, **F-09 doc**
— l'étape de vérification du README nomme le mauvais menu et tait l'activation
de l'API. F-10 reste **non tranché** : l'ancien Action Plugin SWIG apparaît-il
ou non dans *Tools → External Plugins* ; c'est R.1.11 qui le règle.

## Décisions actives

- **D148** — R.1 a trouvé **un seul** défaut satisfaisant l'exception de la
  phase : le serveur ne découvre pas `kicad-cli` sur une installation KiCad
  Windows par défaut. `default_kicad_cli()` renvoie le nom nu `kicad-cli.exe`
  résolu par `PATH`, et l'installateur KiCad ne met pas son `bin` sur `PATH` ;
  `detect_kicad()` n'est jamais appelée par le serveur et rate de toute façon
  `%LOCALAPPDATA%\Programs\KiCad` et la clé de registre par utilisateur.
  Conséquence : ERC, DRC, tous les exports et `verify:"auto"` échouent à la
  première utilisation. Classé **produit, bloquant** → lot **R.7**.

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

**R.1.4 / R.1.11 uniquement, et il est environnemental.** Un dialogue UAC /
secure desktop étranger à KiCad a interrompu trois fois l'accès à l'interface de
Pcbnew. Faits déjà établis autrement : le plugin est déployé sur le disque, il
est enregistré dans l'`installed_packages.json` de KiCad en version 1.1.0, et
son `plugin.json` déclare l'action `start-server` avec `show-button: true` et le
scope `pcb`. Prochaine tentative : activer l'API dans *Preferences → Plugins*
puis relever bouton de barre d'outils, liste des plugins et menu *External
Plugins*. N'empêche aucun autre lot d'avancer.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — R.1 à R.6, plus **R.7** ouvert par R.1.
- `docs/launch/first-run-walk.md` — le parcours, les preuves, les huit frictions.
- `crates/konnect/src/config.rs:75` `default_kicad_cli()` — le nom nu résolu par
  `PATH`. Cœur de R.7.
- `crates/konnect/src/install.rs:402` `detect_kicad()` — liste Windows sans
  `%LOCALAPPDATA%\Programs\KiCad`, sonde registre en `HKLM` seul ; appelée
  seulement par `run_install` et `print_status`, jamais par le serveur.
- `plugin/settings_dialog.py::detect_kicad_cli` — même angle mort registre.
- `packaging/metadata.json` — auteur `mixelpixx`, homepage
  `github.com/mixelpixx/Konnect` : le Plugin Manager renvoie le premier
  utilisateur vers le dépôt **amont** (F-03).
- Ancre de correction mesurée sur cette machine :
  `HKCU\…\Uninstall\KiCad 10.0` → `InstallLocation` =
  `C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0`.
- Travail de R.1 :
  `%LOCALAPPDATA%\Temp\claude\C--Users-FlowUP-kicad-agentic-mcp-konnect-agentic\ab608642-35fc-4d58-b755-c2e65a52c322\scratchpad\r1-walk\`
  (`mcp.sh`, un client MCP minimal réutilisable).
- Projet de test réel : `C:\Users\FlowUP\Documents\r1-walk-test\`.

## Non-bloquants enregistrés, non traités

- macOS non signé et non notarisé ; les notes donnent la commande `xattr`.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9). Hors périmètre.
- Dépôt public à **0 étoile, 0 issue, aucun topic, aucune homepage, Discussions
  désactivées** — ligne de base d'adoption que R.4 et R.5 déplacent.
- F-07 : la description d'`apply_template` affirme câbler les composants ; elle
  les place et rend la liste des connexions à faire. Classé **produit non
  bloquant**, laissé tel quel par R.

## NEXT ACTION

Ouvrir **R.7.1** : écrire d'abord le test qui échoue — `kicad_cli` non
configuré, aucun `kicad-cli` sur `PATH`, une installation KiCad sous préfixe par
utilisateur doit être trouvée — avant de toucher à la résolution dans
`crates/konnect/src/config.rs`. R.1.4 se reprend dès que le bureau est libre et
ne bloque rien.
