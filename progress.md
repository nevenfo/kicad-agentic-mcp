# PROGRESS

## Phase actuelle

**X — Preuve réelle des mutations.** X1 à X6 validées et poussées sur
`ai/mutation-proof-hardening`. Le cœur de la campagne — zéro faux succès sur
les capacités déclarées supportées — est en place et prouvé par falsification.
Restent X7 (benchmark PCB live), X8 (upstream + `flip_component`), X9
(`update_pcb_from_schematic`).

## Tâche actuelle

X7/X8 — relever l'état réel d'upstream, puis construire le corpus PCB live.

## Dernière tâche validée

**X6 — Audit ciblé des autres mutations à risque.**

Validation :
- Troisième défaut de la classe X1 trouvé et corrigé :
  `set_layer_constraints` insérait un `(rule …)` dans le `(setup …)` du board,
  que `kicad-cli` refuse (« Inattendu rule », exit 3). Réécrit vers
  `<board>.kicad_dru`, idempotent, préservant règles et commentaires tiers.
- Prouvé par `kicad_enforces_the_layer_rule_it_was_given` : la violation
  attendue apparaît. Plus aucun code n'insère dans `(setup …)`.
- Angle mort du contrat X2 corrigé : les écritures `Derived` (exports,
  rapports) exigeaient un rechargement KiCad dénué de sens pour un gerber.
- Gate complet vert : `cargo fmt`, `clippy -D warnings`, `cargo test
  --workspace`, plus les suites arbitrées et la suite live.

## Décisions actives

- **Le niveau de preuve exigé est une propriété de la capacité**, dérivée de
  (effet, domaine, write target, adaptateur) : `Capability::required_proof`.
  Une preuve plus faible publie `UNPROVEN`. C'est le correctif structurel :
  `Proof::Test` ne pouvait plus être distingué d'une preuve KiCad.
- Preuves : `Test` < `Bench` < `Arbitrated` (`kicad_reloads`, KiCad recharge)
  < `Live` (`kicad_reads_back`, la session relit). Les deux helpers sont
  nommés par `coverage::{ARBITER, LIVE_ARBITER}` et découverts par scan.
- Un test `#[ignore]`d qui appelle un arbitre compte : la CI n'installe pas
  KiCad, donc l'exiger rendrait la preuve forte inatteignable. Le document
  dit d'où viennent ces preuves (`gate.ps1`, pas la CI).
- Couverture domaines KiCad : 74,5 % → 27,9 %, 78 `UNPROVEN`. Baisse assumée,
  aucun critère assoupli. Baseline upstream re-gelée par le même scanner
  (42 → 13) pour que la comparaison reste tool-for-tool.
- **`kicad-cli` ne valide ni le `.kicad_pro` ni le `.kicad_dru`** : un fichier
  illisible donne exit 0 et les défauts KiCad. L'oracle est donc l'**effet**
  sur le DRC, jamais le code de sortie seul.
- Oracle DRC : le champ `type` d'une violation est stable ; la `description`
  est traduite. Ne jamais asserter sur la description.
- Fixture `clearance_pair.kicad_pcb` : 0,75 mm de cuivre entre deux pistes de
  nets différents → `min_clearance` 0,2 mm silencieux, 1,5 mm ⇒ exactement une
  violation `clearance`. C'est l'oracle de toutes les règles.
- `.kicad_pro` : `board.design_settings.rules`, mm flottants sans unité, clés
  triées, indentation 2 espaces — `to_string_pretty` reproduit le format.
  `.kicad_dru` : `(version 1)` puis des `(rule …)`, valeurs **avec** unité.
- `min_via_size`/`min_via_drill`/`min_trace_width` sont refusés par nom, pas
  aliasés : ils désignent des contraintes que KiCad n'a pas.
- `set_active_layer` est IPC pur, sans repli fichier : le repli consisterait à
  réinventer le champ fautif. Write target `Derived` (il n'écrit rien).
- `scripts/live-pcb-e2e.ps1` **doit** être lancé avec `pwsh`, pas Windows
  PowerShell 5.1, où stderr de cargo devient une erreur terminante.
- Le lock natif KiCad n'est jamais supprimé, déplacé ni jugé périmé.
- Les tests live tournent sur un `KICAD_CONFIG_HOME` dédié.

## Blocage actif

Aucun.

## Observations hors périmètre, non corrigées

- 78 capacités `UNPROVEN` : 70 `sexpr`, 4 `ipc→sexpr`, 3 `ipc`, 1 `cli`.
  Aucune n'est démontrée fautive ; personne ne les a soumises à KiCad. Suite
  de travail, action par classe documentée dans la matrice.
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

X8.1 — `git fetch upstream`, relever le SHA et l'état réel d'upstream
aujourd'hui, puis inspecter `flip_component` et `update_pcb_from_schematic`
sans merge ni rebase. Validation : SHA upstream consigné et périmètre de
comparaison arrêté, avant de construire le corpus X7 qui doit les mesurer.
