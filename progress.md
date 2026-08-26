# PROGRESS

## Phase actuelle

**R — Launch & adoption.** Tous les lots de R sont clos : **R.1 à R.9**, et les
six critères de sortie de la phase sont cochés. Branche
`ai/R-launch-adoption`, poussée.

Il reste **un seul lot ouvert**, ouvert par la décision de R.7.7 :
**R.10 — la release v1.1.1**, dont le plan borné est écrit dans `plan.md` et
**attend la validation de l'utilisateur avant toute exécution**.

## Tâche actuelle

**R.10 — v1.1.1.** Rien n'a été modifié pour elle. Périmètre fermé : R.7
(`kicad_cli`), R.8 (`ipc_address`), R.9.1 (binaire GUI `kicad`), R.9.2 (stackup
non déclaré), F-03 (`packaging/metadata.json`). Sept tâches, dont trois qui
demandent une décision ou une attention particulière :

- **R.10.1** — l'`identifier` PCM `com.github.mixelpixx.konnect` est **gardé** :
  c'est le nom du dossier d'installation, présent dans le README, les deux
  configs d'exemple, le harnais de démo et toute installation existante. Seuls
  l'auteur et la homepage changent.
- **R.10.5** — l'E2E réel se lance **à la main avant le tag** (D144).
- **R.10.7** — l'artefact **publié** est installé et parcouru sans aucun
  `konnect-settings.json` (INV-R1). C'est la seule preuve de ce que v1.1.1
  prétend.

## Dernière tâche validée

**R.6 — la porte de décision**, `docs/launch/decision-gate.md`, plus **R.4**
(kit de lancement) et **R.3** close.

- **Tally R.5 : vide.** 0 étoile, 0 fork, 0 issue extérieure, 0 rapport de
  premier lancement, 2 téléchargements (ceux du mainteneur). Le projet n'a
  jamais été annoncé nulle part : c'est une donnée sur la **diffusion**, pas sur
  la demande.
- **Onze candidats, un critère chacun**, écrits avant que la donnée puisse en
  sélectionner un — et neuf de ces critères nomment un rapport, un
  téléchargement ou une installation extérieurs.
- **Recommandation : publier, puis décider avec des données.** Ordre : livrer
  v1.1.1, appliquer les métadonnées et poster le kit, rouvrir la porte quand le
  tally cesse d'être nul. Explicitement **non** recommandé : ouvrir une phase de
  capacité PCB sur les cinq défauts du run 1.
- **R.4** : quatre brouillons d'annonce (forum KiCad, r/KiCad, Show HN,
  annuaires MCP), métadonnées de dépôt proposées mais **non appliquées**, cinq
  lieux avec leurs exigences, six choses non revendiquées, liste go/no-go de six
  lignes. **Rien n'a été publié.**
- **R.3** : la démo reproduit (runs 2 et 3, 5 → 0 non connectés, 11 segments,
  aucune erreur), et son temps est publié en deux nombres mesurés.

Validation : gate verte sur l'arbre modifié — `fmt`, `clippy -D warnings`,
**1 406 passed, 0 failed, 38 ignored sur 57 suites**, build release. Les
documents de R.4 et R.6 ne touchent pas au code.

## Décisions actives

- **Décisions utilisateur du 2026-08-26 (fin de R)** : publier **les deux
  nombres** de la démo (R.3.10) ; **acheter la seconde prise** (R.3.7, faite) ;
  **une seule v1.1.1 à la fin de R** (R.7.7).
- **D150** — une lecture qui peut diverger de l'état live se corrige *ou*
  annonce sa source (`"file"` / `"ipc"`). R.9.3 annonce, parce que rerouter
  toute la surface de lecture PCB est de taille capacité.
- **D149** — la découverte d'un binaire externe est **une chaîne, pas un
  défaut** : configuré → `PATH` → préfixes → registre ; une valeur configurée
  n'est jamais remplacée ; tri par version décroissante avant préfixe. Vit dans
  `konnect_core::kicad_locate`, couvre `kicad-cli`, `kicad` et `ipc_address`.
- **D147** — la release publie sept assets et aucun fichier de sommes de
  contrôle (F-04). Classé *packaging*.
- **D146** — un chiffre public qu'une release ne remesure pas doit être remesuré
  sur l'artefact publié, ou daté de la version qui l'a mesuré.
- **D145** — un test qui écrit puis relit un état horodaté attend la valeur
  observable du mtime, jamais une durée.
- **D144** — l'E2E gatante se lance à la main **avant** le tag.
- **D143** — `RELEASE_NOTES.md` est le corps de la release **courante**.
- **D148**, **D142 à D111** et les décisions V1 antérieures (INV6, D97…D101) :
  inchangées.
- Invariants de R : **INV-R1** l'artefact testé est celui qui est publié ;
  **INV-R2** une case = une preuve ; **INV-R3** tout problème est classé avant
  correction ; **INV-R4** le parcours est consigné tel qu'un inconnu le vit.

## Blocage actif

Aucun blocage technique. **R.10 attend la validation du périmètre et du numéro
de version par l'utilisateur** avant que quoi que ce soit soit modifié.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — R.1 à R.10.
- `docs/launch/` — `first-run-walk.md` (R.1), `demo-run-{1,2,3}.md` (R.3),
  `launch-kit.md` + quatre brouillons `announce-*.md` (R.4),
  `decision-gate.md` (R.6). `docs/adoption.md` et `.github/ISSUE_TEMPLATE/`
  (R.5).
- `examples/demo/` — pre-state, consigne, setup, vérification, commande de
  régénération des images. **Ne pas éditer en place.**
- `packaging/metadata.json` — F-03, à corriger en R.10.1.
- `crates/konnect-core/src/kicad_locate.rs` — la chaîne D149, partagée.
- Harnais de démo (scratchpad de la session `bbc5fec2-…`) : `r3-demo/`
  (`preflight.sh` gratuit, `run-demo2.sh` dépense, `verify.sh`).

## Non-bloquants enregistrés, non traités

- macOS non signé et non notarisé ; les notes donnent la commande `xattr`.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9). Candidat R.6.
- F-04 : aucune somme de contrôle publiée avec les sept assets.
- F-07 : la description d'`apply_template` affirme câbler les composants.
- F-08, F-11 : UI de KiCad, et un bouton déclaré que KiCad 10 ne rend jamais.
- F-13, F-15, F-17 et l'absence d'un outil *route this net* : enregistrés, avec
  leur critère de promotion dans `decision-gate.md`.
- L'API KiCad est activée sur cette machine depuis R.1.11 — un premier
  utilisateur, lui, doit l'activer.
- Projet jetable de R.1 : `C:\Users\FlowUP\Documents\r1-walk-test\`.

## NEXT ACTION

Faire valider par l'utilisateur le **périmètre et le numéro de version de R.10**
(v1.1.1, cinq correctifs, rien d'autre), puis exécuter R.10.1 → R.10.7 dans
l'ordre, en s'arrêtant avant le tag pour l'E2E manuel (D144).
