# Résultats de la sonde matérielle (phase 1) — 2026-11-09

Boîtier : SOOMFON 15 touches, USB `5548:6670`, protocole mirajazz v1 (rapports de 512 octets).

## Carte des touches

Chaque case donne `index image → index brut reçu à l'appui`. L'index image est celui passé à `write_image`, et l'index brut vaut toujours index image + 1.

|            | Col 1   | Col 2   | Col 3  | Col 4  | Col 5  |
|------------|---------|---------|--------|--------|--------|
| **Haut**   | 12 → 13 | 9 → 10  | 6 → 7  | 3 → 4  | 0 → 1  |
| **Milieu** | 13 → 14 | 10 → 11 | 7 → 8  | 4 → 5  | 1 → 2  |
| **Bas**    | 14 → 15 | 11 → 12 | 8 → 9  | 5 → 6  | 2 → 3  |

- Position « logique » p (0..14, ligne par ligne) → index image : `[12, 9, 6, 3, 0, 13, 10, 7, 4, 1, 14, 11, 8, 5, 2]`.
- **Touche Retour (en bas à droite)** = position 14 = index image **2** = index brut **3**.
- **Écran latéral (à droite des touches)** = index image **15, 16, 17**, de haut en bas (vérifié le 2026-11-12) : 3 cases d'affichage 85×85, même orientation que les touches (rotation 90° + double miroir). **Affichage seul** : ce n'est pas tactile et aucun appui n'est envoyé.

## Comportement

- **Orientation** : rotation 90° + miroir horizontal et vertical. Validé visuellement, le repère blanc apparaît bien en haut à gauche.
- **Appui uniquement** : l'octet d'état vaut toujours `0x00` et aucun événement n'est envoyé au relâchement. Le protocole v1 ne permet donc **ni appui long ni maintien**. Les actions se déclenchent à l'appui.
- Format des rapports d'entrée : `41 43 4B 00 00 4F 4B 00 00 <index brut> …` (« ACK » … « OK »).
- Une écriture (keep-alive) pendant qu'une lecture est en attente fonctionne sans blocage sous Windows.
- Si le logiciel officiel « Stream Controller » tourne, il doit être fermé.

## Mesures

| Mesure | Valeur |
|---|---|
| Connexion | < 1 ms |
| Encodage JPEG de 18 touches (q90) | ≈ 4 ms, 45,5 Ko au total (≈ 2,5 Ko par touche) |
| **Envoi USB de 18 images** | **≈ 650 ms (≈ 36 ms par touche)** |
| RAM de la sonde au repos | 5,6 Mo (working set), 1,0 Mo privé, CPU 0 s |
| RAM du logiciel officiel, pour comparer | 13,3 Mo |

### Démon (phases 2 à 5), mesuré le 2026-11-09

| Mesure | Valeur |
|---|---|
| RAM au repos | **9,4 Mo** (working set), 2,4 Mo privés |
| CPU | 0,03 s cumulées depuis le lancement (≈ 0 % au repos) |
| Binaire | 4,6 Mo |
| Page complète (15 touches) | ≈ 0,6 s |
| Changement de page (10 touches) | ≈ 0,34 s |
| Changement de scène (2 touches) | ≈ 80 ms |
| Une touche (REC, micro) | ≈ 7 ms |

- Le `STP` (commande d'affichage) coûte environ 25 ms : on l'envoie une fois par paquet de 5 touches.
- La qualité JPEG compte peu : on passe de 4–6 rapports de 512 o par touche en q80 à 3–4 en q50.
- OBS 32 envoie `outputActive = false` sur une **pause** d'enregistrement : l'état se déduit de `outputState`.
- Écrire juste après un branchement échoue (`0x8007001F`) ou bloque : le démon attend 1,5 s et borne chaque écriture.

**Goulot d'étranglement identifié : l'envoi USB.** Chaque touche coûte environ 7 rapports de 512 octets. Pistes pour la phase rendu :
1. N'envoyer que les touches qui changent.
2. Réduire la taille des JPEG (qualité 75–80, fonds unis) pour passer d'environ 6 rapports à 3–4 par touche.
3. Pré-encoder chaque page au chargement de la configuration.
