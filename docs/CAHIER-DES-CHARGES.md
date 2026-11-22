# Cahier des charges — soomfon-deck

Décisions validées le 2026-11-09.

## Matériel
- Boîtier : SOOMFON Stream Controller 15 touches LCD (étiquette **XF-CN001**).
- Identifiant USB constaté sur la machine : **`5548:6670`** (famille Mirabox HSV293S), interface HID vendeur page `0xFFA0` / usage `1`.
- Protocole mirajazz **v1** : paquets de 512 octets, images JPEG 85×85, rotation 90° + miroir X/Y, 18 index de touches (15 visibles).
- Le logiciel officiel « Stream Controller » doit être **fermé** quand l'app tourne.
- Carte des touches, limites (pas d'appui long) et mesures : voir [MATERIEL.md](MATERIEL.md).

## Cible
- **Windows 11 uniquement.**
- OBS Studio **32.2.2**, sur le **même PC** que le jeu → WebSocket local (`ws://127.0.0.1:4455`).
- Contrainte n°1 : consommation minimale. Objectifs : RAM < 15 Mo, CPU ≈ 0 % au repos, < 20 ms entre l'appui et la réaction d'OBS.

## Fonctionnalités
| Sujet | Décision |
|---|---|
| OBS | Scènes, enregistrement, direct, et **memes = sources média OBS** déclenchées (pas d'audio dans l'app). |
| Autres actions | Toutes retenues : raccourcis clavier, muet Discord, Spotify, marqueurs/clips Twitch, lancement d'applis. |
| Plugins (2026-11-22) | Ajoutés : **raccourci clavier / macro**, **lancer** (appli, site, fichier), **soundboard** (l'app joue désormais des sons, via le lecteur MCI de Windows, sans bibliothèque audio) et **compte à rebours**. Code dans `src/plugins/`. |
| Retour visuel | Oui : scène active en surbrillance, REC/LIVE, **chrono du live**. |
| Dossiers | **1 seul niveau** : page racine → dossier. Pas de dossier dans un dossier. |
| Touche Retour | **En bas à droite**, fixe dans chaque dossier (14 actions + Retour). |
| Profils | **Non** (une seule configuration). |
| Éditeur | **Slint** (natif Rust), lancé à la demande, séparé du démon. |
| Icônes | Dossier `resources/icons` fourni avec l'app ; l'utilisateur pourra ajouter les siennes. |

## Profil utilisateur
- Ne connaît pas Rust → tout doit être configurable depuis l'éditeur, le code est commenté en français.

## Pistes à confirmer plus tard
- Streamer.bot est installé : il pourrait servir de passerelle pour les actions Twitch (marqueurs, clips) plutôt que d'intégrer l'API Twitch directement.
