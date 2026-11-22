# Overlay Studio — guide

Onglet **Overlays** de l'éditeur Soomfon (`soomfon-editor.exe`, bouton « Overlays » en haut).

## Où sont les fichiers

| Dossier | Rôle |
|---|---|
| `Documents\Blackat-Overlays\overlays\` | **Tes overlays d'origine.** Le Studio ne les modifie jamais. |
| `Documents\Blackat-Overlays\studio\` | **Copie Studio**, versionnée avec Git. C'est elle que l'éditeur modifie. |
| `studio\shared\studio.js` | Les réglages (JSON), écrits par l'éditeur. |
| `studio\shared\studio-apply.js` | Applique les réglages au chargement de chaque overlay. |

Les overlays restent de simples fichiers HTML : ils fonctionnent dans OBS même éditeur fermé.

## Bibliothèque (colonne de gauche)

- **Thème commun** : couleurs, polices, marges, animations, pseudo, serveur, jeu et réseaux, partagés par tous les overlays.
- **Les 6 overlays** : Starting Soon, Be Right Back, Ending, Just Chatting, Gameplay, Alertes.
  Le point vert indique un overlay activé. Un overlay désactivé n'affiche plus rien dans OBS.
- **Dupliquer / Exporter / Supprimer** (colonne de droite, section Bibliothèque).
  La suppression demande une confirmation et ne touche que la copie Studio.
- **Importer** : un fichier `.html`, avec ses réglages si un `reglages.json` se trouve à côté.
  **Exporter** crée un dossier `<id>/` contenant `<id>.html` et `reglages.json`.

## Réglages (colonne de droite)

Les réglages sont groupés en Apparence, Typographie, Position, Animations et Textes.

- Chaque modification est **enregistrée automatiquement**.
- « Réinitialiser » remet un réglage à sa valeur d'origine ; « Tout réinitialiser » fait de même pour tout l'overlay.
- Un titre sur plusieurs lignes s'écrit avec `/` (ex. `Le live / commence`).

## Aperçu et OBS (centre)

- L'aperçu est **rendu par OBS lui-même** (OBS doit être ouvert).
- **Mode studio** : OBS ne rend pas une source navigateur qui n'est affichée nulle part. Pendant que l'onglet Overlays est ouvert, l'éditeur affiche donc une scène **« Soomfon · Aperçu »** côté *Aperçu* du mode studio, jamais à l'antenne.
  - ⚠️ **N'appuie pas sur « Transition » dans OBS pendant l'édition**, sinon l'aperçu passe à l'antenne.
  - En quittant l'onglet ou en fermant l'éditeur, OBS est remis comme avant : mode studio dans son état d'origine, ta scène d'aperçu d'origine, et scène « Soomfon · Aperçu » supprimée.
- **Résolution** : 1080p, 1440p, 4K ou personnalisée. Les overlays sont dessinés en 1920×1080 et mis à l'échelle sans déformation.
- **Copier l'URL** : l'adresse `file:///…/studio/<id>.html`, à coller dans une source Navigateur d'OBS.
- **Utiliser la version Studio dans « … »** : fait pointer une de tes sources existantes vers la copie Studio, en gardant ses options (`?min=5`…). C'est uniquement sur ton clic.
- **Rafraîchir dans OBS** (ou automatiquement à chaque réglage) : recharge les sources qui affichent la version Studio. Attention, rafraîchir les alertes en plein live vide la file d'alertes en cours.
- **Taille → source OBS** : donne la résolution choisie aux sources qui affichent la version Studio.

## Streamer.bot

Rien ne change : les overlays se connectent eux-mêmes au WebSocket Streamer.bot (`127.0.0.1:8080`).
L'aperçu utilise le mode test intégré (`?test=1`) pour montrer des alertes et des soutiens fictifs.

## Nouveaux overlays

- **Just Chatting** (`just-chatting.html`) : écran complet avec un cadre caméra (place ta webcam **par-dessus** dans OBS), le sujet du moment et les 4 derniers soutiens.
- **Gameplay** (`gameplay.html`) : petit bloc transparent dans un coin (bas droite par défaut), avec le dernier follow, sub et don en direct.
