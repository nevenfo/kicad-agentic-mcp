# PROGRESS

## Phase actuelle

**X — Preuve réelle des mutations.** X1 à X9 validées sur
`ai/mutation-proof-hardening`. Zéro faux succès sur les capacités déclarées
supportées, prouvé par falsification ; quatre défauts de corruption corrigés ou
écartés ; `flip_component` importé et prouvé ; `update_pcb_from_schematic`
explicitement reporté avec son analyse.

## Tâche actuelle

Aucune. La phase X est complète ; reste la décision utilisateur sur la
livraison (PR vers `agentic/main`, et release éventuelle).

## Dernière tâche validée

**X7, X8, X9 — corpus arbitré, `flip_component`, décision de report.**

Validation :
- Corpus `crates/konnect-core/tests/kicad_arbitration.rs`, 4 tests arbitrés par
  `kicad-cli`, dont `the_oracle_can_fail` qui prouve que l'oracle peut rougir
  (0,2 → rien, 1,5 → une violation, 0,2 → rien). Sans lui, les autres seraient
  verts même si l'arbitre s'était tu.
- `gate.ps1` gagne une étape `arbitrated` : ces preuves ne tournaient nulle
  part, ni en CI (aucun KiCad) ni au gate. Elle saute bruyamment sans
  `kicad-cli`.
- `flip_component` importé d'upstream `ab337816`, adapté, 21 tests unitaires
  plus l'arbitrage KiCad. Publié `SUPPORTED`/`kicad-parsed` dès l'import.
- `update_pcb_from_schematic` : décision **C — report explicite**, analyse
  complète des invariants consignée dans `plan.md` (X9).
- `gate.ps1` complet vert : fmt, clippy `-D warnings`, tests workspace,
  doctests, build release, étape arbitrée.

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
- Fixtures oracles : `clearance_pair.kicad_pcb` (0,75 mm de cuivre → 0,2 mm
  silencieux, 1,5 mm ⇒ une violation) et `flip_pair.kicad_pcb` (empreinte
  asymétrique `C310`). Oracle de placement : `kicad-cli pcb export pos`,
  colonne `Side` en `top`/`bottom`, indépendante de la langue.
- Emplacements KiCad : contraintes globales dans `.kicad_pro`
  (`board.design_settings.rules`, mm sans unité) ; règles personnalisées dans
  `.kicad_dru` (`(version 1)`, valeurs **avec** unité) ; couche active dans
  `.kicad_prl`, donc session, donc IPC. Rien de tout cela n'est dans le board.
- `min_via_size`/`min_via_drill`/`min_trace_width` sont refusés par nom, pas
  aliasés : ils désignent des contraintes que KiCad n'a pas.
- `set_active_layer` est IPC pur, sans repli fichier (le repli réinventerait le
  champ fautif). KiCad n'expose **aucune** commande de flip : `flip_component`
  est nécessairement fichier, refusé tant que KiCad tient ce board — et son
  aller-retour est géométriquement exact, seule la graphie bouge (`(at x y 0)`
  revient en `(at x y)` ; un `(effects …)` réécrit gagne une espace).
- `refuse_if_board_open_in_kicad` (fork) refuse quand l'IPC dit que KiCad tient
  *ce* board, procède sinon. Écart assumé avec upstream : pas de veto par
  verrou si le transport est injoignable — cohérent avec le fork, où le
  `.kicad_pcb` passe par l'IPC et le garde de verrou ne vise que `.kicad_sch`.
- `scripts/live-pcb-e2e.ps1` et `gate.ps1` se lancent avec `pwsh`, pas Windows
  PowerShell 5.1, où stderr de cargo devient une erreur terminante.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé. Les tests
  live tournent sur un `KICAD_CONFIG_HOME` dédié.

## Blocage actif

Aucun.

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

Ouvrir la PR de `ai/mutation-proof-hardening` vers `agentic/main` et attendre
la CI 7/7. Aucune action autonome au-delà : publier une release, ou reprendre
D1.8 du plan Hi-Fi, relève d'une décision de l'utilisateur.
