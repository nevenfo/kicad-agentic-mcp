# PROGRESS

## Phase actuelle

**Y — Release v1.2.0.** La phase X est mergée dans `agentic/main` (`9dd4b26`,
CI 7/7 verte). Y existe parce que le plugin installé chez l'utilisateur est en
v1.1.4 et ne porte donc pas `flip_component` : le bootstrap client n'installe
qu'une release stable strictement plus récente, donc la release est le seul
canal honnête pour livrer la phase X.

## Tâche actuelle

**Y2.1 — PR #20 vers `agentic/main`, CI en cours.** Y1 validée et commitée
(`b3c51cc`).

## Dernière tâche validée

**Y1 — version, notes et compte d'outils.**

Validation :
- `gate.ps1` vert de bout en bout, étape `arbitrated` comprise (4 tests, dont
  `the_oracle_can_fail`).
- `konnect --version` du build local annonce `1.2.0` et le binaire porte
  `flip_component`.
- Compte d'outils corrigé 203 → 204 sur 22 toolsets, d'après le registre testé
  contre `tools_for` : `plugin/plugin.json` et `packaging/metadata.json` sont
  servis au gestionnaire de plugins, publier 203 aurait livré un chiffre faux.

## Décisions actives

- **Numéro mineur v1.2.0, choisi par l'utilisateur** : la matrice publiée n'est
  pas comparable à celle de v1.1.4 (28,9 % contre 74,5 %, 77 `UNPROVEN`), non
  par régression mais parce qu'elle est mesurée contre une règle plus stricte.
- **Le niveau de preuve exigé est une propriété de la capacité**
  (`Capability::required_proof`, dérivé de effet/domaine/write target/adaptateur).
  Une preuve plus faible publie `UNPROVEN`. `Test` < `Bench` < `Arbitrated`
  (`kicad_reloads`) < `Live` (`kicad_reads_back`). Un test `#[ignore]`d qui
  appelle un arbitre compte : la CI n'a pas KiCad, l'exiger rendrait la preuve
  forte inatteignable ; `gate.ps1` dit d'où elle vient.
- **`kicad-cli` ne valide ni `.kicad_pro` ni `.kicad_dru`** : un fichier
  illisible donne exit 0. L'oracle est l'**effet** (le DRC bouge), jamais le
  code de sortie. Le champ `type` d'une violation est stable, sa `description`
  est traduite : ne jamais asserter dessus.
- Emplacements KiCad : contraintes globales dans `.kicad_pro`
  (`board.design_settings.rules`, mm sans unité) ; règles personnalisées dans
  `.kicad_dru` (valeurs **avec** unité) ; couche active dans `.kicad_prl`, donc
  session, donc IPC. Rien de cela n'est dans le board. Détail : `plan.md`,
  phase X.
- `set_active_layer` est IPC pur, sans repli fichier. KiCad n'expose **aucune**
  commande de flip : `flip_component` est nécessairement fichier, refusé tant
  que KiCad tient ce board ; son aller-retour est géométriquement exact.
- Le bootstrap client (`~/.agents/konnect/konnect-bootstrap.ps1`) compare la
  version que le binaire **annonce** à la dernière release stable et n'installe
  que si elle est strictement plus récente. Un build local posé à la main
  survivrait donc, mais mentirait sur sa version : écarté pour cette raison.
- `scripts/live-pcb-e2e.ps1` et `gate.ps1` se lancent avec `pwsh`, pas Windows
  PowerShell 5.1.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Les tests
  live tournent sur un `KICAD_CONFIG_HOME` dédié.
- Convention de livraison : une branche `ai/<phase>` par phase, une PR par
  phase, merge commit, branche distante conservée.

## Blocage actif

Aucun.

## Observations hors périmètre, non corrigées

- **Flake de test** : `kam-llm openai_compat::absent_backend_is_unreachable_not_a_panic`
  a rendu `Malformed` au lieu de `Unreachable` une fois sur deux exécutions du
  gate. Le test libère un port éphémère puis parie que rien ne le reprend ;
  isolé il passe 3 fois sur 3, et la CI le passe sur les trois OS. Antérieur à
  la phase Y, dont l'invariant interdit qu'un correctif s'y glisse.
- 77 capacités `UNPROVEN` (majorité `sexpr`). Aucune n'est démontrée fautive ;
  personne ne les a soumises à KiCad. Action par classe dans la matrice.
- `ToolErrorKind::from_anyhow` ne reconnaît pas `SexpError::Conflict` nu : une
  course GUI se dégrade en `handler_error`. Préexistant.
- `board_and_labels.rs` porte une assertion négative fragile aux CRLF sous
  Windows ; réelle sur ubuntu et macos.

## Fichiers / zones utiles

- Release : `Cargo.toml`, `crates/schematic-viewer/{Cargo.toml,tauri.conf.json}`,
  `RELEASE_NOTES.md`, `plugin/plugin.json`, `packaging/metadata.json`.
- Le workflow `Release` se déclenche sur tag `v*` et produit lui-même
  `konnect-pcm-v<version>-windows.zip` ; il exige l'E2E KiCad vert.
- Contrat de preuve : `crates/konnect-core/src/capability/` →
  `docs/capability-matrix.md`, régénéré par `KAM_UPDATE_MATRIX=1 cargo test
  -p konnect-core --test capability_matrix`.
- Helpers d'arbitrage : `crates/konnect-core/tests/harness/mod.rs`.
- Plugin installé : `~/Documents/KiCad/10.0/3rdparty/plugins/
  com_github_mixelpixx_konnect/bin/konnect.exe`.
- `kicad-cli` 10.0.6 : `%LOCALAPPDATA%\Programs\KiCad\10.0\bin\kicad-cli.exe`.
- Projet Hi-Fi (dépôt distinct, continuité propre) :
  `~/Documents/Etabli/Projets/Chaine Hifi`, branche `main`, à jour.

## Préconditions de tout test live

1. Un seul répertoire par identifiant de plugin sous `3rdparty`.
2. Aucune autre instance KiCad ne détient le socket d'API.
3. Aucun dialogue modal, sinon `AS_NOT_READY` sur un pipe pourtant présent.
4. Les toolsets sont opt-in : sans `load_toolset <nom>`, un refus
   `toolset_not_loaded` fait passer une assertion pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir.

## NEXT ACTION

Y2.1 — attendre la CI 7/7 de la PR #20, merger, puis taguer `v1.2.0` sur le
commit de merge et pousser le tag pour déclencher le workflow `Release`.
