# PROGRESS

## Phase actuelle

**Q — Release v1.1.0 : DONE.** Ouverte le 2026-08-26 sur demande explicite de
l'utilisateur juste après la fusion de la phase P, périmètre publication
seulement. Q.1 à Q.6 sont closes, aucune case ouverte ne reste.

**v1.1.0 est publiée** : https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.0
— tag annoté sur `80da119` (fusion de la PR #11), 7 assets, corps posé depuis
`RELEASE_NOTES.md`. `agentic/main` est à jour ; la branche `ai/Q-release-1.1.0`
reste sur le remote.

## Tâche actuelle

Aucune en cours.

## Dernière tâche validée

**Q.5 — l'artefact publié est ouvert, pas cru.**

`konnect-pcm-v1.1.0-windows.zip` téléchargé depuis la release et ouvert :
8 entrées, un **seul** `versions[]` lisant `1.1.0` / `stable` /
`kicad_version 10.0` / `platforms ["windows"]` sans champ `download_*` inventé,
le manifeste de plugin, le viewer bundlé, et le `konnect.exe` extrait qui
répond **`konnect 1.1.0`**.

Q.5 a corrigé une affirmation de Q.2.4 : la taille du binaire y avait été
supposée inchangée. Le binaire publié fait **23,7 MiB**, contre les 21,8 MB
annoncés par les notes et les 22 MB annoncés par le README. Mesuré contre le
binaire publié de v1.0.0 — 22 860 288 octets alors, 24 848 384 maintenant,
**+1,9 MiB** — ce qui tranche aussi l'unité : le « MB » de ce dépôt a toujours
été des MiB. Corrigé dans les trois sites et le corps de la release réémis ; le
fichier figé dans le tag garde l'ancien chiffre, un tag ne se déplace pas pour
corriger de la prose.

Validation de la phase :
- **workflow Release run `32948098418` : vert**, 9 jobs dont un sauté
  volontairement — `Live IPC against a running pcbnew`, que le mode gatant
  écarte. 4 binaires standalone, 3 paquets PCM, 7 assets
- CI run `32947127320` sur `fb18d96` : verte sur les trois OS, plus Clippy,
  Format, PCM et le viewer
- E2E gatante run `32947078548` sur `123e228` : verte sur tous ses steps,
  lancée **avant** le tag (D144)
- l'arbre du commit de fusion `80da119` est **identique octet pour octet** à
  celui de `fb18d96`, l'arbre sur lequel les gates ont tourné
- `cargo metadata --locked` sur les **deux** manifestes : PASS — le contrôle
  exact que la v1.0.0 avait raté
- gate local : fmt PASS, clippy `-D warnings` PASS, **1385 tests, 0 échec**

## Décisions actives

- **D146** — un chiffre public qu'une release ne remesure pas doit être
  remesuré **sur l'artefact publié**, pas reconduit. Q.2.4 avait déclaré la
  taille du binaire inchangée sans l'ouvrir ; elle avait bougé de 1,9 MiB.
  L'unité du dépôt est le MiB, écrit « MB ».

- **D145** — un test qui écrit puis relit un état horodaté attend la **valeur
  observable** du mtime, jamais une durée : la granularité d'horodatage n'est
  pas portable (100 ns sur NTFS, aucune sous-seconde sur un ext4 à inodes de
  128 octets), et un horodatage déjà écrit ne bouge pas tout seul. Corollaire :
  un mutex de test qui ne garde qu'une variable d'environnement se prend avec
  `into_inner()`, sinon un panic transforme un rouge en trois.

- **D144** — l'E2E gatante se lance **à la main avant le tag**, jamais après.
  Elle n'a pas de déclencheur par PR, `release.yml` en dépend, et un rouge
  découvert après le tag laisserait un tag publié sans release.

- **D143** — `RELEASE_NOTES.md` est le corps de la release **courante**, pas un
  changelog cumulatif : `gh release edit` le pose comme corps et l'historique
  vit sur GitHub Releases. Corollaire : un chiffre qu'une release ne remesure
  pas doit dire de quelle version il parle — les figures du benchmark décrivent
  v1.0.0 et le disent explicitement.

- **D142** — la version est **v1.1.0**, pas la v1.0.1 initialement demandée. La
  phase P a déplacé quatre comportements qu'un client observe :
  `create_netclass`/`assign_net_to_class` écrivent le `.kicad_pro` voisin et non
  plus le board (l'ancienne forme faisait sortir `kicad-cli` en code 3, D112) ;
  `run_drc` lit `unconnected_items` et refuse un board au cuivre non routé que
  le gate d'évidence approuvait ; les symboles power entrent dans le graphe de
  nets ; `register_*_library` répond `inserted`/`unchanged`/`updated`. Rien
  n'est cassant, donc le mineur est le numéro exact.

- Les décisions **D140 à D111** de la phase P et les décisions V1 antérieures
  (INV6, D97…D101) restent actives, inchangées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `.github/workflows/release.yml` — sur `v*` : 4 binaires standalone, 3 paquets
  PCM (macOS universel via `lipo`), validation du schéma `packages.v1`, et
  l'E2E gatante en `needs` du job `release`.
- `.github/workflows/e2e-kicad.yml` — sans déclencheur par PR ; dispatch avec
  `-R nevenfo/kicad-agentic-mcp` (D114).
- `crates/schematic-viewer/Cargo.lock` — hors workspace, hors `gate.ps1` ;
  toute release doit le bumper explicitement, sous peine du rouge d'O.7.3.
- `crates/konnect-schematic-editor/src/library.rs` — `fingerprint_children`,
  `env_lock()` et le test d'obsolescence d'index (Q.6).
- `crates/konnect-core/src/router/mod.rs` — le test qui rend « 202 » mesurable.

## Non-bloquants enregistrés, non traités

- `packaging/metadata.json` porte encore l'`identifier`
  `com.github.mixelpixx.konnect` et `mixelpixx` comme auteur — sans effet sur
  une release GitHub, bloquant seulement une soumission au dépôt d'addons
  officiel, hors périmètre décidé pour cette phase.
- macOS part **non signé et non notarisé**, comme à v1.0.0 ; les notes donnent
  la commande `xattr` exacte. Signer exige un compte Apple Developer.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9), dont plusieurs
  rouges. La release n'en dépendait pas.

## NEXT ACTION

Aucune tâche de plan ouverte : v1.1.0 est publiée et vérifiée sur son artefact.
Les seules sections encore ouvertes du plan sont **I.1 — Custom KiCad gate**
(`TODO (default: NO)`, à réévaluer à la sortie de KiCad 11) et **D.5.3**,
conditionnelle par construction ; aucune ne se lance d'elle-même. La prochaine
action demande donc une **décision de l'utilisateur** — les candidats nommés à
l'ouverture de cette phase étant l'hygiène des dépendances (les 8 PR
Dependabot), les builds Linux/macOS avec signature, ou la création de symboles
et footprints.
