# PROGRESS

## Phase actuelle

**R — Launch & adoption : terminée.** R.1 à R.10.8 et tous les critères de
sortie sont cochés. La release publique **v1.1.1** est validée et la PR #12 est
mergée dans `agentic/main`.

## Tâche actuelle

Aucune tâche technique. La préparation du lancement public est terminée ; les
publications externes restent manuelles et sous le contrôle de l'utilisateur.

## Dernière tâche validée

**Préparation manuelle du lancement — 2026-08-27.** Les métadonnées GitHub
(description, homepage, 12 topics) sont appliquées. Les quatre brouillons sont
alignés sur v1.1.1, `docs/launch/ready-to-post.md` contient les textes finaux et
`docs/adoption.md` est prêt pour les premiers retours sans donnée fabriquée.

Validation :

- métadonnées relues par `gh repo view` ;
- compteurs GitHub pré-annonce datés et sourcés ;
- `git diff --check` PASS.

## Décisions actives

- Aucun post, soumission Show HN ou entrée d'annuaire n'est publié par l'agent.
- L'identifier PCM `com.github.mixelpixx.konnect` reste inchangé.
- INV-R1 à INV-R4 restent les invariants des futures releases et mesures.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- Copie prête : `docs/launch/ready-to-post.md`.
- Brouillons sources : `docs/launch/announce-*.md`.
- Suivi externe : `docs/adoption.md`.
- Release : `https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.1`.

## NEXT ACTION

Après la publication manuelle par l'utilisateur, enregistrer le premier retour
extérieur dans `docs/adoption.md`, puis rouvrir la porte R.6 lorsque le tally
n'est plus nul.
