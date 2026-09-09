# PROGRESS

## Phase actuelle

**X — Preuve réelle des mutations.** Une mutation KiCad ne peut plus être
publiée `SUPPORTED` sur la foi de notre propre code : la vérité vient de
`kicad-cli` ou de l'IPC officiel. Objectif = zéro faux succès, pas plus
d'outils supportés. La phase W (`v1.1.4`) est close et publiée.

## Tâche actuelle

X2 — faire du niveau de preuve **exigé** une propriété de la capacité.

## Dernière tâche validée

**X1 — Reproduction des deux défauts contre KiCad.**

Validation :
- Projet jetable = copie du demo `microwave` de l'installation. Baseline :
  `kicad-cli pcb drc` charge le document, exit 0.
- Transcription fidèle de `set_constraint` appliquée → `kicad-cli` refuse :
  « Inattendu min_clearance … ligne 77 », exit 3. Le handler retourne pourtant
  `{"success": true, "changed": [...]}` sans aucune condition.
- Écriture de `set_active_layer` appliquée → `kicad-cli` refuse : « Inattendu
  active_layer … ligne 33 », exit 3. Le handler retourne `{"active_layer": …}`.
- Les deux défauts sont donc de même classe : succès RPC sur un document que
  KiCad ne charge plus.

## Décisions actives

- Les contraintes globales vivent dans `.kicad_pro` →
  `board.design_settings.rules`, en mm flottants. Relevé sur deux projets
  indépendants (`HifiAmp_TPA3255`, demo `CM5_MINIMA_3`), pas supposé.
- `min_via_size` et `min_via_drill`, arguments actuels de `set_design_rules`,
  ne nomment **aucune** clé KiCad. Les clés réelles sont `min_via_diameter` et
  `min_through_hole_diameter`.
- Un `.kicad_pcb` produit par KiCad ne porte aucune de ces clés dans `(setup)` :
  zéro occurrence mesurée sur le projet réel.
- `active_layer` est une préférence **locale** de session : `.kicad_prl`,
  `board.active_layer`, un **entier** (0 = F.Cu), fichier couramment gitignoré.
  Ce n'est pas un état du document.
- L'IPC officiel expose `SetActiveLayer`/`GetActiveLayer`
  (`board_commands.proto`). Le proto est dans le dépôt, le binding client Rust
  n'existe pas encore → `set_active_layer` relève du cas « voie fiable ».
- Les contraintes globales ne sont **pas** exposées par l'IPC
  (`project_settings.proto` ne couvre que les netclasses) : transport =
  fichier `.kicad_pro`, arbitre = `kicad-cli`.
- L'architecture de preuve existe déjà et n'est pas à reconstruire :
  `capability::MANIFEST` + `coverage::scan` dérivent le statut des tests
  trouvés. Le défaut est unique et localisé : `Proof::Test` suffit à
  `Status::Supported`, sans que la capacité puisse exiger mieux.
- Une baisse du pourcentage de couverture est un résultat acceptable ; les
  critères ne sont jamais assouplis pour préserver un chiffre.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé : son
  contenu (50 octets, `hostname` + `username`) ne permet pas de décider la
  fraîcheur. Présence vaut refus.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié, jamais sur le
  profil réel de l'utilisateur.

## Blocage actif

Aucun.

## Observations hors périmètre, non corrigées

- `ToolErrorKind::from_anyhow` ne reconnaît pas `konnect_sexp::SexpError::
  Conflict` nu : une course GUI se dégrade en `handler_error` au lieu de
  `conflict`. Préexistant.
- `crates/konnect-core/tests/board_and_labels.rs:129` porte une fragilité CRLF
  en assertion négative : satisfaite sans rien vérifier sous un checkout
  Windows, réelle sur ubuntu et macos.

## Fichiers / zones utiles

- Défauts : `crates/konnect-core/src/tools/verification.rs`
  (`set_constraint`, `handle_set_design_rules`, `handle_get_design_rules`) et
  `crates/konnect-core/src/tools/pcb_board.rs:756`
  (`handle_set_active_layer`).
- Preuve : `crates/konnect-core/src/capability/{mod.rs,coverage.rs,render.rs}`,
  rendu vers `docs/capability-matrix.md` par
  `crates/konnect-core/tests/capability_matrix.rs`.
- Tests concernés : `crates/konnect-core/tests/config_and_rules.rs`,
  `crates/konnect-core/tests/board_and_labels.rs`.
- IPC : `crates/konnect-ipc/proto/board/board_commands.proto` (SetActiveLayer),
  `crates/konnect-ipc/src/client.rs`.
- `gate.ps1` (racine), `scripts/live-pcb-e2e.ps1`.
- `kicad-cli` : `%LOCALAPPDATA%\Programs\KiCad\10.0\bin\kicad-cli.exe` (10.0.6).
- Projet jetable : copie du demo `microwave` livré avec l'installation KiCad.

## Préconditions de tout test live

1. Un seul répertoire par identifiant de plugin sous `3rdparty` — trois copies
   tuent l'éditeur 3 s après démarrage.
2. Aucune autre instance KiCad ne détient le socket d'API.
3. Aucun dialogue modal : l'assistant de configuration et l'avis de format
   ancien font répondre `AS_NOT_READY` sur un pipe pourtant présent.
4. Les toolsets sont opt-in : sans `load_toolset`, un refus `toolset_not_loaded`
   fait passer une assertion pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir : la fermeture se retente.

## NEXT ACTION

X2.1 — Ajouter à `Capability` le niveau de preuve exigé, dérivé du couple
(effet, adaptateur) plutôt que déclaré à la main, puis faire rétrograder
`Capability::status` toute capacité dont la preuve trouvée est plus faible.
Validation : `cargo test -p konnect-core` vert et au moins une capacité
effectivement rétrogradée.
