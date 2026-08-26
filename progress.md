# PROGRESS

## Phase actuelle

**Q — Release v1.1.0.** Ouverte le 2026-08-26 sur demande explicite de
l'utilisateur, juste après la fusion de la phase P. Périmètre : **publication
seulement** — aucune capacité nouvelle, aucun travail Dependabot, aucune
création de symbole ou footprint, pas de KiCad 11. Q.1 et Q.2 sont closes.

Branche de travail `ai/Q-release-1.1.0`, PR **#11** ouverte contre
`agentic/main`, commit `3089596`.

## Tâche actuelle

**Q.4 — tag et publication.** Q.3 et Q.6 sont closes ; la CI est verte sur
`123e228`, le commit que le tag portera, et l'E2E gatante y est rejouée.

## Dernière tâche validée

**Q.1 et Q.2 — la version bouge partout où elle est portée, et les documents
publics disent la version qu'ils livrent.**

- **Q.1** — cinq fichiers, pas deux : `Cargo.toml` (`[workspace.package]`),
  `Cargo.lock`, `crates/schematic-viewer/Cargo.toml`, son `tauri.conf.json` et
  **son propre `Cargo.lock`**. Ce dernier est le piège d'O.7.3 : hors du
  workspace, jamais touché par `gate.ps1`, et seul responsable du rouge CI à
  v1.0.0. `cargo update --workspace` sur les deux manifestes ne déplace que les
  15 entrées locales, aucune dépendance externe.
- **Q.2** — `RELEASE_NOTES.md` devient le corps de la v1.1.0 : section *What
  changed in v1.1.0* nommant les quatre comportements observables, les
  correctifs de fidélité, et la phrase qui manquait — les chiffres du benchmark
  décrivent toujours **v1.0.0**, cette release ne l'a pas rejoué. macOS non
  signé est documenté avec la commande `xattr` exacte, pas euphémisé. README
  ligne 27.

Validation :
- `cargo metadata --locked --format-version 1` sur **les deux** manifestes :
  PASS. C'est le contrôle exact que la v1.0.0 avait raté
- `cargo fmt --all -- --check` : PASS
- `cargo clippy --workspace --locked --all-targets -- -D warnings` : PASS, 0
- `cargo test --workspace --locked --lib --tests --no-fail-fast` : **57 suites,
  1385 tests, 0 échec**, mêmes comptes qu'en phase P
- « 202 tools / 22 toolsets » est **mesuré**, pas supposé :
  `find_capabilities_description_quotes_the_real_corpus_size` échoue si le
  registre et la prose divergent, et `ALL_TOOLSETS` porte 22 entrées

**Q.3 — les trois gates verts sur le commit à taguer.**

- checks de PR, run `32943774777` : **7 jobs verts**, dont `Schematic viewer`,
  celui qui avait rougi à v1.0.0, et `PCM packaging validation`
- E2E, dispatchée à la main sur la branche, run `32943778088` : **verte sur
  tous ses steps** — boucle de conception, conformance du corpus de démos,
  corpus de layers, les neuf sondes, le wedge IPC et le paquet PCM ; rien de
  sauté sauf « upload artifacts on failure ». Le dispatch manuel inclut en
  outre `Live IPC against a running pcbnew`, que le mode gatant écarte, et il
  est vert aussi

**Q.6 — un test mesurait le système de fichiers du runner, pas l'index.**
Trouvé par cette phase : la CI a rougi sur `c377a41`, un commit qui ne touche
que `plan.md` et `progress.md`.
`a_symbol_added_inside_an_existing_library_makes_the_index_stale` écrit le cache
d'index, puis crée un symbole dans `Device.kicad_symdir` et exige que le cache
devienne périmé. `fingerprint_children` hache le mtime de chaque entrée, donc
l'assertion ne tient que si le système de fichiers **réestampille** ce
répertoire. Qu'il le fasse est un fait de **machine** : NTFS horodate en unités
de 100 ns et le test est vert **30 fois sur 30** ici, tandis qu'un volume ext4 à
inodes de 128 octets n'a **aucun** champ sous-seconde — là-bas le symbole tombe
dans le tic déjà enregistré. Classe de D140, un cran plus bas.

La **première correction était fausse et la CI l'a dit** : attendre une
milliseconde présuppose une granularité plus fine que la milliseconde, ce qui
est précisément la question posée. Un horodatage déjà écrit ne bouge pas tout
seul ; ce qu'il faut attendre est la **valeur observable**. Le test recrée
désormais le symbole jusqu'à ce que le mtime du répertoire diffère de celui que
le cache a enregistré, borné à ~2 s, et son message d'échec imprime les deux
horodatages.

Les deux autres rouges du premier run sont **un seul** défaut d'amplification :
le panic empoisonnait `ENV_LOCK` et tout test suivant mourait en `PoisonError`
au lieu de rendre son verdict — `env_lock()` récupère le garde avec
`into_inner()` (P.7.6 au niveau du mutex).

Validation :
- CI run `32946678707` sur `123e228` : **7 jobs verts**, dont
  `Check & Test (ubuntu-latest)`, seule machine où ce test ait jamais rougi.
  C'est le seul oracle qui compte ici
- le run rouge `32945946481` a **prouvé** la correction du mutex : le même
  défaut s'y rapporte « 34 passed; 1 failed » au lieu de trois échecs
- gate local sur `123e228` : fmt PASS, clippy `-D warnings` PASS, **1385 tests,
  0 échec**, plus 30 exécutions consécutives du test réparé, toutes vertes
- aucun code de production touché : tout est dans `mod suggestion_tests`

## Décisions actives

- **D142** — le numéro de version est **v1.1.0**, pas la v1.0.1 demandée à
  l'ouverture. La phase P a déplacé quatre comportements qu'un client observe :
  `create_netclass`/`assign_net_to_class` écrivent le `.kicad_pro` voisin et
  non plus le board (l'ancienne forme faisait sortir `kicad-cli` en code 3,
  D112) ; `run_drc` lit `unconnected_items` et refuse désormais un board au
  cuivre non routé que le gate d'évidence approuvait ; les symboles power
  entrent dans le graphe de nets ; `register_*_library` répond `inserted` /
  `unchanged` / `updated` et accepte `replace_existing`. Un numéro de patch
  sous-annoncerait les quatre. Rien n'est cassant, donc le mineur est le
  numéro exact.

- **D143** — `RELEASE_NOTES.md` est le corps de la release **courante**, pas un
  changelog cumulatif : `gh release edit` le pose comme corps, et l'historique
  des versions vit sur GitHub Releases. Corollaire : un chiffre qu'une release
  ne remesure pas doit dire de quelle version il parle — les figures du
  benchmark décrivent v1.0.0 et le disent maintenant explicitement.

- **D145** — un test qui écrit puis relit un état horodaté doit attendre la
  **valeur observable** du mtime, jamais une durée : la granularité
  d'horodatage n'est pas portable (100 ns sur NTFS, aucune sous-seconde sur un
  ext4 à inodes de 128 octets), et un horodatage déjà écrit ne bouge pas tout
  seul. Corollaire de méthode : un mutex de test qui ne garde qu'une variable
  d'environnement se prend avec `into_inner()`, sinon un panic transforme un
  rouge en trois.

- **D144** — l'E2E gatante se lance **à la main avant le tag**, jamais après.
  Elle n'a pas de déclencheur par PR, `release.yml` en dépend, et un rouge
  découvert après le tag laisserait un tag publié sans release — que défaire
  est précisément ce que le contrat Git interdit.

- Les décisions **D140 à D111** de la phase P et les décisions V1 antérieures
  (INV6, D97…D101) restent actives, inchangées.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `.github/workflows/release.yml` — déclenché sur `v*` : 4 binaires standalone,
  3 paquets PCM (macOS universel via `lipo`), validation du schéma
  `packages.v1`, et l'E2E gatante en `needs` du job `release`.
- `.github/workflows/e2e-kicad.yml` — sans déclencheur par PR ; dispatch à la
  main avec `-R nevenfo/kicad-agentic-mcp` (D114).
- `crates/schematic-viewer/Cargo.lock` — hors workspace, hors `gate.ps1` ;
  toute release doit le bumper explicitement.
- `packaging/metadata.json`, `packaging/build-pcm.{ps1,sh}`,
  `packaging/validate-pcm.py` — inchangés depuis v1.0.0.
- `crates/konnect-core/src/router/mod.rs` — le test qui rend « 202 » mesurable.
- `crates/konnect-schematic-editor/src/library.rs` — `fingerprint_children`
  (mtime haché en ms), `env_lock()` et le test d'obsolescence d'index (Q.6).

## Non-bloquants enregistrés, non traités dans cette phase

- `packaging/metadata.json` porte encore l'`identifier`
  `com.github.mixelpixx.konnect` et `mixelpixx` comme auteur — sans effet sur
  une release GitHub, bloquant seulement le jour d'une soumission au dépôt
  d'addons officiel, hors périmètre ici.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9), dont plusieurs
  rouges. La release n'en dépend pas.

## NEXT ACTION

Exécuter **Q.4** dès que l'E2E gatante rejouée sur `123e228` (run
`32947078548`) est verte : fusionner la PR #11, poser le tag annoté `v1.1.0` sur
le commit de fusion, vérifier les 8 jobs du workflow Release et ses 7 assets,
puis poser `RELEASE_NOTES.md` comme corps de la release.
