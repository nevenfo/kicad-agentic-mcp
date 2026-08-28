# PROGRESS

## Phase actuelle

**T — Reprise du benchmark Hi-Fi.**

## Tâche actuelle

**T.2.1 — Définition de la prochaine étape du benchmark.**

## Dernière tâche validée

**T.1.1 — Placement U1 depuis la bibliothèque projet.**

Validation :

- alias projet `HifiAmp_TPA3255_Local` :
  `${KIPRJMOD}/HifiAmp_TPA3255.kicad_sym` ;
- U1 unique : `HifiAmp_TPA3255_Local:LM5010ASD` à `(100.33, 100.33)` ;
- sauvegarde et relecture MCP : PASS.

## Décisions actives

- Résolution : table projet, table globale, bibliothèques installées ;
  `${KIPRJMOD}` est ancré au dossier du schéma.
- `save_project` conserve son API et devient document-aware ; un schéma déjà
  persisté est un succès explicite, sans commande PCB.
- Un appel IPC sans chemin reste conservateur et requiert `documents` explicites
  pour la protection transactionnelle.
- Le serveur MCP installé correspond au build de HEAD `9bcd9fb` ; l'ancien
  exécutable reste disponible comme rollback `konnect.exe.pre-9bcd9fb.bak`.

## Blocage actif

La suite du benchmark Hi-Fi après le placement U1 n'est décrite ni dans le
dépôt ni dans la mémoire de projet ; une cible fonctionnelle est indispensable
avant toute nouvelle modification du schéma.

## Fichiers / zones utiles

- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_pro`
- `C:\Users\FlowUP\Documents\Etabli\Projets\Chaine Hifi\HifiAmp_TPA3255.kicad_sch`

## NEXT ACTION

T.2.1 — Obtenir le brief de la prochaine étape du benchmark Hi-Fi et fixer son
critère de validation avant toute nouvelle écriture dans le projet KiCad.
