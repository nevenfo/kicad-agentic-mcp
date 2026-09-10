# PROGRESS

## Phase actuelle

**X — Preuve réelle des mutations : livrée.** X1 à X9 validées, PR #19 mergée
dans `agentic/main`. Zéro faux succès sur les capacités déclarées supportées,
prouvé par falsification ; quatre défauts de corruption corrigés ou écartés ;
`flip_component` importé et prouvé ; `update_pcb_from_schematic` explicitement
reporté avec son analyse.

Aucune phase n'est ouverte. La suite — release, ou reprise du benchmark Hi-Fi —
attend une décision de l'utilisateur.

## Tâche actuelle

Aucune.

## Dernière tâche validée

**Livraison de la phase X — merge de la PR #19 dans `agentic/main`.**

Validation :
- PR #19, `ai/mutation-proof-hardening` → `agentic/main`, CI 7/7 verte avant
  merge, puis mergée en merge commit `9dd4b26`.
- CI post-merge sur `agentic/main` verte 7/7 : Format, Clippy,
  Check & Test (ubuntu / macos / windows), Schematic viewer, PCM packaging.
- X9.3 reste décochée : conditionnelle, écartée par la décision C de X9.

## Décisions actives

- **Le niveau de preuve exigé est une propriété de la capacité**, dérivé de
  (effet, domaine, write target, adaptateur) : `Capability::required_proof`.
  Une preuve plus faible publie `UNPROVEN`. C'est le correctif structurel.
- `Test` < `Bench` < `Arbitrated` (`kicad_reloads` : KiCad recharge) < `Live`
  (`kicad_reads_back` : la session relit). Helpers nommés par
  `coverage::{ARBITER, LIVE_ARBITER}`, découverts par scan. Un test `#[ignore]`d
  qui appelle un arbitre compte — la CI n'a pas KiCad, l'exiger rendrait la
  preuve forte inatteignable ; le document dit d'où elle vient (`gate.ps1`).
- Couverture domaines KiCad 74,5 % → 28,9 %, 77 `UNPROVEN`. Baisse assumée,
  aucun critère assoupli ; baseline upstream re-gelée par le même scanner
  (42 → 13) pour rester tool-for-tool.
- **`kicad-cli` ne valide ni `.kicad_pro` ni `.kicad_dru`** : un fichier
  illisible donne exit 0 et les défauts KiCad. L'oracle est l'**effet** (le DRC
  bouge), jamais le code de sortie. Le champ `type` d'une violation est stable,
  sa `description` est traduite : ne jamais asserter dessus.
- Fixtures oracles : `clearance_pair.kicad_pcb` (0,2 mm silencieux, 1,5 mm ⇒
  une violation) et `flip_pair.kicad_pcb` (`C310` asymétrique). Oracle de
  placement : `kicad-cli pcb export pos`, colonne `Side`, indépendante de la
  langue.
- Emplacements KiCad : contraintes globales dans `.kicad_pro`
  (`board.design_settings.rules`, mm sans unité) ; règles personnalisées dans
  `.kicad_dru` (valeurs **avec** unité) ; couche active dans `.kicad_prl`, donc
  session, donc IPC. Rien de cela n'est dans le board. Détail complet : `plan.md`
  phase X.
- `min_via_size`/`min_via_drill`/`min_trace_width` sont refusés par nom, pas
  aliasés : ils désignent des contraintes que KiCad n'a pas.
- `set_active_layer` est IPC pur, sans repli fichier (le repli réinventerait le
  champ fautif). KiCad n'expose **aucune** commande de flip : `flip_component`
  est nécessairement fichier, refusé tant que KiCad tient ce board ; son
  aller-retour est géométriquement exact, seule la graphie bouge.
- `refuse_if_board_open_in_kicad` (fork) refuse quand l'IPC dit que KiCad tient
  *ce* board, procède sinon. Écart assumé avec upstream : pas de veto par
  verrou si le transport est injoignable — cohérent avec le fork, où le
  `.kicad_pcb` passe par l'IPC et le garde de verrou ne vise que `.kicad_sch`.
- `scripts/live-pcb-e2e.ps1` et `gate.ps1` se lancent avec `pwsh`, pas Windows
  PowerShell 5.1, où stderr de cargo devient une erreur terminante.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Les tests
  live tournent sur un `KICAD_CONFIG_HOME` dédié.
- Convention de livraison : une branche `ai/<phase>` par phase, une PR par
  phase, merge commit, branche distante conservée après merge.

## Blocage actif

Aucun.

## Décision utilisateur en attente

La version publiée reste **v1.1.4** (tag `v1.1.4`, `Cargo.toml` inchangé) : la
phase X n'a pas bumpé la version. Trois suites possibles, aucune n'est
autonome :

1. **Release** de la phase X (numéro de version et périmètre à fixer par
   l'utilisateur avant toute modification).
2. **Reprise du benchmark Hi-Fi** à D1.8 (plan hors de ce dépôt).
3. **Nouvelle phase** sur les observations hors périmètre ci-dessous.

## Observations hors périmètre, non corrigées

- 77 capacités `UNPROVEN` (majorité `sexpr`). Aucune n'est démontrée fautive ;
  personne ne les a soumises à KiCad. Suite de travail, action par classe
  documentée dans la matrice.
- `ToolErrorKind::from_anyhow` ne reconnaît pas `SexpError::Conflict` nu :
  une course GUI se dégrade en `handler_error`. Préexistant.
- `board_and_labels.rs` porte une assertion négative fragile aux CRLF sous
  Windows ; réelle sur ubuntu et macos.

## Fichiers / zones utiles

- Contrat de preuve : `crates/konnect-core/src/capability/{mod.rs,coverage.rs,
  render.rs,baseline.rs}` → `docs/capability-matrix.md`, régénéré par
  `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`.
- Corrigés : `tools/verification.rs` (`set_design_rules`,
  `set_layer_constraints`), `tools/pcb_board.rs` (`set_active_layer`).
- Helpers d'arbitrage : `crates/konnect-core/tests/harness/mod.rs`
  (`kicad_reloads`, `kicad_reads_back`, `Harness::live`, `CLEARANCE_BOARD`).
- IPC : `crates/konnect-ipc/src/client.rs` (`get_active_layer`,
  `set_active_layer`), `proto/board/board_commands.proto`.
- `ReadbackMismatch` : `crates/konnect-core/src/mcp/error.rs`.
- `gate.ps1`, `scripts/live-pcb-e2e.ps1` (via `pwsh`).
- `kicad-cli` 10.0.6 : `%LOCALAPPDATA%\Programs\KiCad\10.0\bin\kicad-cli.exe`.
- Projet jetable : copie du demo `microwave` de l'installation KiCad.
- Remote `upstream` : `https://github.com/mixelpixx/Konnect.git` (push
  désactivé).

## Préconditions de tout test live

1. Un seul répertoire par identifiant de plugin sous `3rdparty`.
2. Aucune autre instance KiCad ne détient le socket d'API.
3. Aucun dialogue modal, sinon `AS_NOT_READY` sur un pipe pourtant présent.
4. Les toolsets sont opt-in : sans `load_toolset`, un refus
   `toolset_not_loaded` fait passer une assertion pour la mauvaise raison.
5. `CloseMainWindow` poste `WM_CLOSE` sans le garantir.

## NEXT ACTION

Obtenir de l'utilisateur la décision consignée sous « Décision utilisateur en
attente » — release (avec numéro de version et périmètre), reprise Hi-Fi D1.8,
ou nouvelle phase — puis ouvrir l'unité correspondante dans `plan.md`. Aucune
action autonome ne reste dans le périmètre livré.
