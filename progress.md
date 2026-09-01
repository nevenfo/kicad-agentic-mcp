# PROGRESS

## Phase actuelle

**W — v1.1.4, les trois limitations Pareto : terminée et publiée.** W.1 à W.5
validées, `v1.1.4` publiée et installée.

## Tâche actuelle

Aucune. La phase W est close ; la suite attend une décision de l'utilisateur
(voir NEXT ACTION).

## Dernière tâche validée

**W.5 — Publication de `v1.1.4`.**

W.5.1 : `1.1.3` → `1.1.4` dans `Cargo.toml`,
`crates/schematic-viewer/Cargo.toml`, `tauri.conf.json` et les deux lockfiles ;
ligne de statut du README ; `RELEASE_NOTES.md` réécrit. `packaging/metadata.json`
n'a pas bougé : c'est un gabarit rempli au build depuis `Cargo.toml`.

W.5.2 : commit `ad7b7a4`, puis `caa12aa` (correctif de test), PR #16 mergée en
`42fb497` sur `agentic/main`, tag `v1.1.4` sur ce commit, workflow `Release`
vert.

W.5.3 : `v1.1.4` installée comme seule version en vigueur.

Validation :
- `gate.ps1` complet vert localement (fmt, `clippy -D warnings`, tests
  workspace, doctests, build release), exit 0.
- CI 7/7 verte sur `caa12aa` (PR #16) **et** sur le commit de merge `42fb497`,
  celui que le tag désigne.
- Release `v1.1.4` publiée, ni draft ni prerelease, 7 artefacts dont
  `konnect-pcm-v1.1.4-{windows,linux,macos}.zip`.
- Paquet Windows ouvert et vérifié : `metadata.json` en `1.1.4`,
  `bin/konnect.exe --version` → `konnect 1.1.4`, les deux exécutables présents.
- Installé sous `3rdparty/plugins/com_github_mixelpixx_konnect` (un seul
  répertoire), icône sous `3rdparty/resources/`, registre PCM
  `%APPDATA%\kicad\10.0\installed_packages.json` porté à `1.1.4` (sauvegarde
  `.bak-v1.1.3` à côté).
- Runtime prouvé, pas seulement le numéro : handshake MCP contre le binaire
  installé → `serverInfo {name: konnect, version: 1.1.4}`, 21 outils au
  démarrage (le chiffre annoncé par les notes), et `kicad_describe` sert bien
  `set_footprint_graphics`, l'outil neuf de cette release.
- Rollback unique : `com_github_mixelpixx_konnect.rollback-v1.1.3-20260901`
  (vérifié `konnect 1.1.3`). Celui de `v1.1.2` a été supprimé conformément à la
  politique ; son artefact reste téléchargeable depuis la release `v1.1.2`.

## Décisions actives

- La CI ne se déclenche que sur `push` vers `main`/`agentic/main` et sur
  `pull_request` vers ces branches : pousser une branche de travail seule ne
  produit **aucun** run. Le candidat passe donc obligatoirement par une PR.
- `gh` résolvait par défaut le dépôt amont `mixelpixx/Konnect`, d'où un
  « No commits between … » trompeur à la création de PR. Le défaut est
  désormais `nevenfo/kicad-agentic-mcp`.
- Un test ne doit pas asserter les fins de ligne sans le vouloir.
  `.gitattributes` déclare `* text=auto`, donc un checkout Windows neuf livre
  les fixtures `.kicad_sch` en CRLF, alors qu'un fichier déjà présent en LF le
  reste : le gate local peut être vert là où la CI Windows échoue. Une
  assertion de placement normalise `\r\n` avant de comparer ; la préservation
  des fins de ligne garde son propre test,
  `a_crlf_sheet_is_written_back_as_crlf`.
- `v1.1.4` reste un patch malgré l'ajout de `set_footprint_graphics` et le
  changement de sens de `fields: {"clé": null}` ; les notes signalent cette
  exception délibérée.
- Le chiffre « catalogue de 215 outils » des notes est daté de sa mesure
  (2026-08-24) : la surface est aujourd'hui de 203 outils + 13 méta-outils.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Sonde
  réelle : `~<nom>.kicad_sch.lck` et `~<projet>.kicad_pro.lck` apparaissent à
  l'ouverture d'Eeschema, contenu `{"hostname":…,"username":…}`, 50 octets,
  **sans PID ni horodatage** ; une fermeture propre les retire. La fraîcheur
  n'étant pas décidable, elle n'est pas décidée : présence vaut refus.
- Le garde ne vise que `.kicad_sch`. Le `.kicad_pcb` passe par l'IPC, et le
  lock `.kicad_pro` n'est pas celui du document muté.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le
  profil réel de l'utilisateur.
- `set_footprint_graphics` est une API typée par primitive, pas un éditeur de
  texte `.kicad_mod` : une couche par appel, tout le reste reporté tel quel.
- Le repère de broche 1 ne se devine pas : c'est une déclaration du client,
  `true` par défaut, l'oubli sur une pièce polarisée étant l'erreur coûteuse.
- L'alignement du courtyard sur la grille KLC se fait vers l'extérieur.
- Les trois attributs natifs sont des tags du bloc symbole, jamais des
  propriétés : `(property "dnp" "yes")` s'affiche dans la liste des champs et
  ne change ni le netlist, ni la BOM, ni « Update PCB from schematic ».
- `null` dans `fields` signifie suppression de la propriété.
- Les empreintes Hi-Fi défectueuses sont corrigées et prouvées **sur copie**
  (`scripts/live-footprint-fix.ps1`). L'application in-place dans
  `HifiAmp_TPA3255_Local.pretty\` relève de D1.8 du plan Hi-Fi et attend
  l'utilisateur.

## Blocage actif

Aucun.

## Observations hors périmètre, non corrigées

- `ToolErrorKind::from_anyhow` ne reconnaît pas
  `konnect_sexp::SexpError::Conflict` nu. Les outils qui appellent
  `write_atomic_if_unchanged` directement dégradent donc une course GUI en
  `handler_error` au lieu de `conflict`. Défaut préexistant, orthogonal à W.1.
- `crates/konnect-core/tests/board_and_labels.rs:129` porte la même fragilité
  CRLF que le test corrigé en W.5.2, mais en assertion **négative** : sous un
  checkout Windows elle est satisfaite sans rien vérifier. Elle reste réelle
  sur ubuntu et macos, donc aucune couverture n'est perdue au total.

## Fichiers / zones utiles

- Porteurs de version : `Cargo.toml`,
  `crates/schematic-viewer/{Cargo.toml,tauri.conf.json}`, `README.md`,
  `RELEASE_NOTES.md`. `packaging/build-pcm.{ps1,sh}` remplit
  `packaging/metadata.json` depuis la version du workspace.
- `.github/workflows/{ci.yml,release.yml}`
- `gate.ps1` (racine), `scripts/live-editor-lock.ps1`,
  `scripts/live-schematic-e2e.ps1`, `scripts/live-pcb-e2e.ps1`,
  `scripts/live-footprint-fix.ps1`, `scripts/live-b28-on-board.ps1`
- Plugin installé :
  `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\plugins\com_github_mixelpixx_konnect`
- Registre PCM : `C:\Users\FlowUP\AppData\Roaming\kicad\10.0\installed_packages.json`
- `kicad-cli` : `%LOCALAPPDATA%\Programs\KiCad\10.0\bin\kicad-cli.exe`
- Fixture : `C:\Users\FlowUP\Documents\KiCad\KonnectValidationV31`
- Projet Hi-Fi : `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi`
  (dépôt git propre, dernier commit `a55870a`)

## Préconditions de tout test live

1. Un seul répertoire sous `3rdparty` par identifiant de plugin. Trois copies
   de `com_github_mixelpixx_konnect` tuent l'éditeur 3 s après démarrage
   (`0xC0000005`, `wxbase332u_vc_x64_custom.dll`), pipe publié puis perdu.
   Identique en `10.0.3` et `10.0.6` : ce n'est pas une régression de version.
2. Aucune autre instance KiCad ne détient le socket d'API — sinon les requêtes
   partent au mauvais éditeur et reviennent en « does not handle … for this
   document type ».
3. Aucun dialogue modal : l'assistant `Configuration de KiCad` et l'avis de
   format de fichier ancien font répondre `AS_NOT_READY` sur un pipe présent.
4. Les toolsets sont opt-in : un client charge `load_toolset` avant d'appeler
   (`sch_components`, `library`, …), sinon chaque outil répond
   `toolset_not_loaded` et une assertion de refus passe pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir : la fermeture propre se
   retente.

## NEXT ACTION

Aucune action autonome sûre : la phase W est close et publiée. La suite
naturelle est D1.8 du plan Hi-Fi — appliquer in-place les corrections
d'empreintes dans `HifiAmp_TPA3255_Local.pretty\`, aujourd'hui prouvées
seulement sur copie — mais elle touche le projet de l'utilisateur et attend sa
décision. Ouvrir une phase X ou reprendre le plan Hi-Fi relève du même choix.
