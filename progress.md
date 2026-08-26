# PROGRESS

## Phase actuelle

**R — Launch & adoption : IN PROGRESS.** Ouverte le 2026-08-26 sur demande
explicite de l'utilisateur, juste après la publication de v1.1.0 (phase Q close,
toutes ses cases fermées). Périmètre : **adoption, pas capacité**. Aucun refactor,
aucune feature opportuniste, aucun travail KiCad 11, aucun Dependabot / signature
macOS / dépôt d'addons officiel sauf blocage réel de R.

Objectif : qu'un inconnu passe seul de la page de release à une première tâche
validée par KiCad, et que les retours des premiers utilisateurs suffisent à
décider la phase technique suivante.

Branche : `ai/R-launch-adoption`, ouverte sur `90d0928`.

## Tâche actuelle

**R.1.3 — installer le paquet PCM publié par le chemin documenté** : KiCad 10 →
Plugin and Content Manager → *Install from File* → redémarrage, puis lire le
chemin d'installation **sur le disque** au lieu de le supposer depuis le README.
Étape GUI : passe par `desktop-control`.

## Dernière tâche validée

**R.1.1 et R.1.2 — l'état initial et l'artefact publié.**

État initial, non reproductible une fois R.1.3 lancée, donc figé ici :
- `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\` est **vide** — aucun plugin
  Konnect installé sur cette machine
- KiCad **10.0.3** release build, wxWidgets 3.3.2, à
  `C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\`
- **aucun** client MCP ne connaît `konnect` : pas de
  `%APPDATA%\Claude\claude_desktop_config.json`, pas de `.mcp.json` dans le
  dépôt, `claude mcp list` répond « No MCP servers configured »

Artefact sous test, téléchargé depuis la page de release et jamais rebâti :
`konnect-pcm-v1.1.0-windows.zip`, **12 258 180 octets**, SHA-256
`25fe29cac9b0f812dd337e5700e466db9dad769bdbbfa89c85b6e11d3d167dd0`, 8 entrées,
`konnect.exe` de 24 848 384 octets.

## Décisions actives

- **D147** — le dépôt publie sept assets et **aucun fichier de sommes de
  contrôle**. Un utilisateur ne peut pas vérifier ce qu'il a téléchargé sans
  refaire le build. Classé *packaging*, à traiter en R.2 ou R.4 selon le coût ;
  enregistré ici pour ne pas être redécouvert.

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

- Invariants propres à R : **INV-R1** l'artefact testé est celui qui est publié,
  jamais un build local ; **INV-R2** une case = une preuve ; **INV-R3** tout
  problème est classé UX / packaging / documentation / configuration / produit
  **avant** correction ; **INV-R4** le parcours est consigné tel qu'un inconnu le
  vit, détours compris.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `plan.md` § *Phase R* (l. 5135) — tâches, dépendances, ordre d'exécution.
- Artefact et travail de R.1 :
  `%LOCALAPPDATA%\Temp\claude\C--Users-FlowUP-kicad-agentic-mcp-konnect-agentic\ab608642-35fc-4d58-b755-c2e65a52c322\scratchpad\r1-walk\`
- `packaging/metadata.json` — le paquet publié annonce `identifier`
  `com.github.mixelpixx.konnect`, auteur `mixelpixx`, homepage
  `github.com/mixelpixx/Konnect`. Le Plugin Manager de KiCad renverra donc le
  premier utilisateur vers le dépôt **amont**, pas vers celui-ci : ce qui était
  un non-bloquant en phase Q casse directement la boucle de retour de R.5.
- `README.md` — l'installation y est décrite ; R.2 la réécrit à partir de ce que
  R.1 aura réellement exécuté.
- `examples/mcp.example.json`, `examples/claude_desktop_config.example.json` —
  publient le chemin `…\3rdparty\plugins\com_github_mixelpixx_konnect\bin\konnect.exe`,
  à confronter au chemin réel lu en R.1.3.

## Non-bloquants enregistrés, non traités

- macOS part **non signé et non notarisé** ; les notes donnent la commande
  `xattr` exacte. Signer exige un compte Apple Developer.
- Huit PR Dependabot ouvertes (#1, #2, #3, #5, #6, #7, #8, #9), dont plusieurs
  rouges. Hors périmètre de R sauf blocage réel.
- Le dépôt est public avec **0 étoile, 0 issue, aucun topic, aucune homepage,
  Discussions désactivées**. C'est la ligne de base d'adoption que R.4 et R.5
  déplacent, pas un blocage.

## NEXT ACTION

Exécuter **R.1.3** : installer `konnect-pcm-v1.1.0-windows.zip` par KiCad 10 →
Plugin and Content Manager → *Install from File*, redémarrer KiCad, puis relever
sur le disque le chemin réel du binaire installé et le comparer à celui que
publient le README et les deux fichiers d'exemple.
