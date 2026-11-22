# Audit & faisabilité : FIFINE SC3 Controller + Overlay Studio

Rapport du 2026-11-10. Phases 1 (audit) et 2 (faisabilité) du cahier des charges.
**Aucun fichier du projet ni des overlays n'a été modifié** pendant cet audit.

---

## 1. Audit de Soomfon (soomfon-deck)

### Architecture actuelle

Un workspace Rust de ~4 600 lignes, 3 exécutables et une bibliothèque partagée :

| Élément | Rôle | Fichiers clés |
|---|---|---|
| **Bibliothèque** `soomfon_deck` | config, rendu des touches, OBS, boîtier USB | `src/lib.rs` |
| **Démon** `soomfon-deck.exe` | pilote le boîtier SOOMFON en arrière-plan (9 Mo, ~0 % CPU) | `src/main.rs`, `app.rs`, `device.rs`, `obs.rs` |
| **Éditeur** `soomfon-editor.exe` | interface Slint de configuration du Stream Deck | `editor/src/main.rs`, `editor/ui/editor.slint` |
| **Lanceur** `blackat-launcher.exe` | démarre OBS, Streamer.bot, le démon et l'éditeur | `launcher/` |

- **Communication éditeur → démon** : le fichier `%APPDATA%\soomfon-deck\config.toml`. L'éditeur l'écrit de façon atomique et le démon le recharge automatiquement (`watcher.rs`).
- **OBS** : `obws 0.15` (WebSocket v5). Le mot de passe est lu automatiquement dans la config d'OBS.
- **Interface** : Slint 1.18. Le style « Blackat » (nuit bleue, cartes, pilules) est partagé entre le lanceur et l'éditeur.
- **Dépendances déjà présentes et réutilisables** :
  - `windows` 0.61, pour Core Audio, via `async-hid` ;
  - `async-hid`, pour lire du HID ;
  - `obws`, qui permet déjà le rafraîchissement d'une source navigateur (`press_properties_button`) et la capture d'une source (`take_screenshot`) ;
  - `serde_json`, `toml`, `notify`.

### Système de plugins

**Il n'y en a pas.** Les fonctions sont des modules Rust + des pages Slint. Recommandation : **ne pas en créer**. Deux modules de plus ne justifient pas une architecture de plugins ; on garde des modules séparés (`audio/`, `overlays/`) dans la bibliothèque.

### Points d'intégration naturels

1. **Éditeur** : il devient « l'app Soomfon » avec une navigation à gauche. Le contenu actuel devient l'onglet « Stream Deck », et on ajoute « Volumes » et « Overlays ».
2. **Démon** : il reste le seul processus permanent. Il exécutera aussi les actions de volume par application.
3. **Config** : `config.toml` reçoit les nouvelles sections. Les overlays gardent leurs propres fichiers (voir §3).

### Risques de régression

| Risque | Parade |
|---|---|
| Casser le Stream Deck en touchant la config | Nouvelles sections facultatives, tests d'aller-retour existants étendus |
| Alourdir le démon | Core Audio appelé seulement à l'appui d'une touche (pas de boucle) |
| Casser les overlays | Ajout d'**une ligne** par fichier HTML, valeurs par défaut = rendu actuel, sauvegarde Git avant |

---

## 2. Faisabilité matérielle : FIFINE SC3

### Ce que Windows voit (périphérique branché, `USB\VID_3142&PID_0C33`)

| Interface | Type | Détail |
|---|---|---|
| MI_00 | Audio USB (UAC) | 1 sortie « Haut-parleurs (fifine SC3) » + 1 entrée « Microphone (fifine SC3) » |
| MI_03 | HID, page `0x000C` usage `0x0001` | « Consumer Control » (touches multimédia) |
| MI_04 | HID, page `0xFF00` usage `0x55AA` | Interface vendeur propriétaire |

Aucun logiciel FIFINE n'est installé (rien à observer pour apprendre un protocole).

### Mesures

1. **Sonde HID** (outil jetable, hors projet) : écoute des deux collections HID pendant 90 s, alors que **les 4 sliders ont été bougés** (confirmé par l'utilisateur). **0 rapport reçu.**
2. **Core Audio** : surveillance du volume Windows des deux périphériques fifine pendant 75 s, sliders bougés. **Aucun changement** (sortie 100 %, micro 85,5 %).
3. Le slider HEADPHONE **change bien le son du casque** : il agit donc dans le mélangeur lui-même.

### Conclusion

> **Les sliders du SC3 ne sont pas lisibles par Windows.** Ce sont des réglages analogiques internes, libellés MIC / LINE IN / HEADPHONE / LINE OUT, et non des voies de mixage numériques. Aucun logiciel ne peut connaître leur position. **L'attribution d'une application Windows à un slider du SC3 est impossible avec ce matériel.** Je ne la simulerai pas.

### Ce qui reste faisable : le contrôle du volume par application

La partie logicielle (sessions audio Windows, volume et muet par appli, sans toucher le volume général) est **faisable**. Elle passe par Core Audio (`IAudioSessionManager2` → `IAudioSessionEnumerator` → `ISimpleAudioVolume`), avec la crate `windows`, déjà présente. Il lui faut seulement **une autre source de commande** que les sliders du SC3 :

| Option | Matériel | Ressenti | Coût |
|---|---|---|---|
| **A. Touches du SOOMFON** (recommandé pour démarrer) | déjà possédé | Un dossier « Volumes » : pour chaque appli, une touche − / une touche + / une touche qui **affiche le % en direct** et coupe le son à l'appui | 0 € |
| **B. Contrôleur à potentiomètres type « deej »** (Arduino + 4 à 5 potentiomètres, protocole série ouvert) | à acheter ou fabriquer | **Vrais sliders physiques**, position lue en continu | ~15–25 € |
| **C. Surface MIDI** (ex. Korg nanoKONTROL2, 8 faders) | à acheter | Vrais faders, standard MIDI | ~50–60 € |

L'architecture (profils, attribution par slider, plage min/max, inversion, cartes à l'écran) est la même pour A, B et C : seule la « source » change. On peut démarrer avec A et brancher B ou C plus tard sans tout refaire.

---

## 3. Overlay Studio : analyse des overlays existants

### Où ils sont

`C:\Users\Blackat\Documents\Blackat-Overlays\overlays\`. Ce dossier n'est pas versionné (pas de Git).

| Fichier | Scène OBS (collection « Sans nom ») | Source navigateur |
|---|---|---|
| `starting.html?min=5` | debut | « debuuttt », 1920×1080 |
| `brb.html` | pause | « pausee », 1920×1080 |
| `ending.html` | FIN | « fin », 1920×1080 |
| `alerts.html` (Follow, Sub, Gift, Bits, Raid, Dons) | écran 1 | « alertes », 1920×1080, fond transparent |

Les sources sont chargées en `file:///…` (URL, pas « fichier local »). **Just Chatting** et **Gameplay** n'existent pas encore : il faudrait les créer.

### Structure (déjà très favorable)

- `shared/config.js` : **tous les textes et réglages** dans un objet `window.OVERLAY_CONFIG` (pseudo, réseaux, titres, durées des alertes, sons, connexion Streamer.bot).
- `shared/tokens.css` : **couleurs, polices, courbes et durées d'animation, marges** en variables CSS (`--sunset`, `--gold`, `--font-display`, `--dur-base`, `--pad-x`…).
- `shared/base.css` : mise en page commune. Le canevas 1920×1080 est mis à l'échelle par `--fit`, et la taille des titres passe par `--display-size`.
- `shared/events.js` : connexion **directe** des overlays au WebSocket Streamer.bot (`127.0.0.1:8080`, abonnement Twitch / StreamElements / Streamlabs / Ko-fi), normalisation des événements, mémoire des soutiens (localStorage), **mode test intégré** (`?test=1`, `Events.emit(...)`).

### Faisabilité

| Exigence | Faisable ? | Comment |
|---|---|---|
| Réglages sans toucher au code, en JSON | ✅ | Un fichier `shared/studio.js` contenant `window.OVERLAY_STUDIO = { …JSON… }`. C'est un JSON enveloppé, car les pages en `file://` ne peuvent pas lire un `.json` dans OBS. Un petit `shared/studio-apply.js` fusionne ces valeurs dans `OVERLAY_CONFIG` et pose les variables CSS. **Aucun HTML régénéré.** |
| Ne rien casser | ✅ | Une seule ligne ajoutée par HTML. Sans `studio.js`, ou avec des valeurs vides, le rendu reste identique à aujourd'hui. Sauvegarde Git du dossier avant toute modification. |
| Couleurs, polices, tailles, marges, durées | ✅ immédiat | Elles sont déjà en variables CSS. |
| Textes, réseaux, durées des alertes | ✅ immédiat | Ils sont déjà dans `config.js`. |
| Déplacer des éléments, bordures, ombres, flous | ✅ avec adaptation | Ajouter quelques variables CSS de position et d'effet (ex. position de la zone d'alertes), avec des valeurs par défaut identiques. |
| Rafraîchir OBS automatiquement | ✅ | `obws` → bouton « refreshnocache » de la source. Attention : rafraîchir `alerts.html` en plein live vide la file d'alertes en cours, donc ce sera une option. |
| Prévisualisation fidèle et en direct | ✅ (choix à faire) | Voir ci-dessous. |
| Dimensions 1080p / 1440p / 4K / perso | ✅ | Le canevas est déjà mis à l'échelle par `--fit`. On change la taille de la source OBS via obws. |
| Copier l'URL, vérifier la disponibilité | ✅ | Chemins connus, existence des fichiers, présence de la source dans OBS. |
| Fonctionne éditeur fermé | ✅ | Les overlays restent des fichiers statiques lus par OBS. |
| Streamer.bot inchangé | ✅ | `events.js` n'est pas touché. Les alertes de test de l'aperçu passent par `Events.emit`, sans Streamer.bot. |
| Bibliothèque : dupliquer, renommer, supprimer, importer, exporter | ✅ | Copie d'un HTML + de sa section de réglages. Import/export en `.zip`. |

### Prévisualisation : deux approches

| | **WebView2 intégré** (crate `wry`) | **Capture OBS** (`take_screenshot`, déjà dispo) |
|---|---|---|
| Fidélité | Chromium, le même moteur qu'OBS | Exacte : c'est OBS qui rend la source |
| Temps réel / animations | ✅ fluide, réglages injectés sans recharger | Images fixes (2 à 5 par seconde maximum) |
| OBS doit être ouvert | Non | Oui |
| Coût | +1 dépendance, ~100 Mo pendant que le Studio est ouvert | Aucun |

**Recommandation : les deux.** WebView2 pour éditer en direct, plus un bouton « Vérifier dans OBS » qui affiche une capture réelle.

---

## 4. Plan d'intégration proposé

| Phase | Contenu | Livrable vérifiable |
|---|---|---|
| **3a. Volumes : moteur** | Module `audio` (Core Audio) : lister les sessions par appli, lire/régler volume et muet, regrouper les sessions d'une même appli, sans toucher au volume général | Test : baisser Spotify sans toucher Discord ni le volume Windows |
| **3b. Volumes : commande** | Option A (touches SOOMFON) : actions `app_volume` (+/−/muet), touche qui affiche le % en direct, profils (Gaming, Streaming, Just Chatting, Travail) | Dossier « Volumes » fonctionnel sur le boîtier |
| **3c. Volumes : interface** | Onglet « Volumes » : 4 cartes (appli, icône, volume, actif, type de contrôle, plage min/max, inversion, bouton Test), liste des applis générée depuis les sessions audio, applis absentes signalées | Chaque bouton testé |
| **4a. Overlays : socle** | Git dans `Blackat-Overlays`, `studio.js` + `studio-apply.js`, une ligne par HTML, rendu identique vérifié par captures OBS avant/après | Aucune différence visible |
| **4b. Overlays : Studio** | Onglet « Overlays » : bibliothèque, éditeur (apparence, typo, position, animations), aperçu WebView2 + capture OBS, résolutions, rafraîchissement OBS, copie d'URL | Chaque réglage visible dans l'aperçu et dans OBS |
| **5. Intégration** | Navigation Stream Deck / Volumes / Overlays dans l'app Soomfon (style Blackat) | — |
| **6. Tests** | Liste du cahier des charges, sur ton PC, avec OBS et Streamer.bot | Rapport de test |
| **7. Docs** | Guide d'utilisation, limites matérielles (SC3) | `docs/` |

### Décisions prises (2026-11-10)

| Sujet | Décision |
|---|---|
| Volumes par application | **Abandonnés.** Pas de module audio : le SC3 n'est pas exploitable et aucune autre commande n'est souhaitée. |
| Aperçu des overlays | **Capture OBS seule** (`take_screenshot`) : rendu exact, aucune dépendance ajoutée, OBS doit être ouvert. |
| Just Chatting / Gameplay | **À créer** dans le style des overlays existants. |
| Overlays existants | **Non modifiés.** Le Studio travaille sur une **copie à part**, versionnée avec Git. Les sources OBS ne basculent sur la copie que sur demande explicite. |

### Plan révisé

| Phase | Contenu |
|---|---|
| **4a. Socle** | Copie de `Blackat-Overlays/overlays` vers un dossier Studio versionné avec Git. Ajout de `studio.js` / `studio-apply.js` dans la copie. Rendu identique vérifié par capture OBS. |
| **4b. Nouveaux overlays** | `just-chatting.html` et `gameplay.html` dans le même style, branchés sur Streamer.bot comme les autres. |
| **4c. Studio** | Onglet « Overlays » : bibliothèque (activer, dupliquer, renommer, supprimer avec confirmation, importer/exporter), éditeur sans code, aperçu par capture OBS, résolutions, rafraîchissement OBS, copie d'URL, vérification. |
| **5. Intégration** | Navigation Stream Deck / Overlays dans l'app Soomfon. |
| **6–7. Tests & docs** | Sur ton PC, avec OBS et Streamer.bot. |

### Décisions qui étaient à prendre (pour mémoire)

1. **Volumes** : option A (touches SOOMFON, tout de suite), B (deej) ou C (MIDI) ?
2. **Aperçu des overlays** : WebView2 + capture OBS (recommandé), ou capture OBS seule (aucune dépendance de plus) ?
3. **Just Chatting / Gameplay** : faut-il créer ces deux overlays dans le même style ?
4. **Overlays** : feu vert pour initialiser Git dans `Blackat-Overlays` puis ajouter une ligne `<script>` à chaque HTML ?
