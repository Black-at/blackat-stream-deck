# Pack Blackat — sauvegarde et réinstallation

## Sauvegarder (fait)

Blackat Launcher → lien **« Sauvegarder ma config (changer de PC) »** → mot de passe (2 fois) → **Créer le pack**.
Le pack `Blackat-Pack-AAAA-MM-JJ.zip` est créé sur le Bureau en quelques secondes.

Pour une copie parfaite, ferme OBS et Streamer.bot avant. Sinon, le launcher propose « Sauvegarder quand même ».

### Contenu

| Élément | Origine | Dans le pack |
|---|---|---|
| Config OBS (profils, scènes, sources, connexions des panneaux) | `%APPDATA%\obs-studio`, **sans** logs, crashes, updates, profiler_data et caches navigateur | `data.blackat` → `obs/` |
| Plugins OBS tiers | `C:\ProgramData\obs-studio\plugins` | `obs-plugins/` |
| Streamer.bot (programme + config, dossier portable) | `Documents\Streamer.bot`, **sans** cache WebView2 ni logs | `streamerbot/` |
| Overlays (origine + copie Studio + historique Git) | `Documents\Blackat-Overlays` | `overlays/` |
| Fichiers utilisés par les scènes mais rangés ailleurs | détectés dans les JSON de scènes (images, sons, vidéos) | `media/NNN/…` + liste dans `manifest.json` |
| Config du Stream Deck | `%APPDATA%\soomfon-deck\config.toml` | `soomfon/` |
| Programmes Blackat (launcher, Stream Deck, éditeur, icônes) | dossier du launcher | `app/` (en clair) |
| Installeur officiel d'OBS, même version (signature vérifiée « OBS Project, LLC ») | téléchargé une fois depuis GitHub, gardé dans `%LOCALAPPDATA%\Blackat\cache` | `installers/` (en clair) |

### Sécurité

- `data.blackat` est un zip **chiffré AES-256**. Il contient la clé de stream (`service.json`), les jetons Twitch de Streamer.bot et le mot de passe WebSocket d'OBS.
- Le mot de passe n'est stocké nulle part : **sans lui, pas de restauration possible**.
- `manifest.json` (en clair) ne contient que des versions, des chemins et des listes de fichiers.
- Vérifié : le bon mot de passe déchiffre les 460 fichiers, un mauvais est refusé.

### En ligne de commande (tests, automatisation)

```
blackat-launcher.exe --backup <dossier> --password <mdp> [--force]
blackat-launcher.exe --verify-pack <pack.zip> --password <mdp> --report <fichier>
```

## Installer (fait)

Sur le nouveau PC : clic droit sur le pack → **Extraire tout** → lancer `Blackat Launcher.exe` du dossier extrait.
Le launcher détecte le pack (`manifest.json` + `data.blackat` à côté de lui) et ouvre l'écran **Installation** :
mot de passe → **Installer** → **Lancer Blackat**.

| # | Étape | Détail |
|---|---|---|
| 1 | Vérification du pack | déchiffre tout `data.blackat` (mauvais mot de passe → refus, rien n'est écrit) ; OBS et Streamer.bot doivent être fermés |
| 2 | Mise de côté de l'existant | `obs-studio`, `Streamer.bot`, `Blackat-Overlays`, `soomfon-deck` renommés en `<dossier>.bak-AAAA-MM-JJ-HH-MM` (heure UTC) : **rien n'est supprimé** |
| 3 | Installation d'OBS | ignorée si OBS est déjà installé ; sinon installeur du pack (signature « Valid » exigée), `/S`, une invite UAC ; sans installeur : winget |
| 4 | Configuration OBS | `%APPDATA%\obs-studio` |
| 5 | Plugins OBS | `C:\ProgramData\obs-studio\plugins` (droits admin requis : sinon simple avertissement) |
| 6 | Streamer.bot | `Documents\Streamer.bot` (programme portable + config + connexions) |
| 7 | Overlays et médias | `Documents\Blackat-Overlays` ; médias des scènes remis à leur place (profil réécrit), un fichier déjà présent est conservé |
| 8 | Stream Deck (Blackat) | programmes + icônes dans `%LOCALAPPDATA%\Programs\Blackat`, `config.toml`, raccourci **Blackat** sur le Bureau |
| 9 | Chemins et audio | `C:\Users\<ancien>` → nouveau profil dans tous les JSON/INI d'OBS et de Streamer.bot (écritures `\`, `\\`, `/`, `file:///`, casse ignorée, `Blackat2` non touché) ; périphériques audio absents → « Par défaut » |
| 10 | Contrôle final | chaque fichier local cité par les scènes doit exister ; les manquants sont listés |

### Testé (PC simulé, jamais sur la vraie config)

- Pack réel (460 fichiers) restauré en **3 s** dans un faux profil `Users\NouveauPC` : 16 chemins réécrits, aucun reste de l'ancien profil, JSON des scènes et de Streamer.bot valides, 4/4 médias, tous les fichiers des scènes présents, raccourci créé dans le faux Bureau.
- 2ᵉ passe : les 4 dossiers existants mis de côté en `.bak`.
- Mauvais mot de passe : refus à l'étape 1, aucun fichier écrit.
- Fenêtre testée avec `BLACKAT_RESTORE_SANDBOX` (même parcours, même résultat).
- Non testé en vrai : l'installation d'OBS elle-même (OBS est déjà installé ici) et la copie des plugins dans ProgramData.

### En ligne de commande (tests)

```
blackat-launcher.exe --restore <pack extrait> --password <mdp> --sandbox <dossier>
```

Volontairement limité au **PC simulé** (`--sandbox` obligatoire) : la vraie installation se fait depuis la fenêtre.
Variables de test de la fenêtre : `BLACKAT_RESTORE_SANDBOX=<dossier>` (+ `BLACKAT_TEST_PASSWORD`, actif seulement en bac à sable).
