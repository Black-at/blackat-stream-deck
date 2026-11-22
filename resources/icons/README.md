# Icônes

Aperçu de toutes les icônes : ouvre `preview.html` via un petit serveur local (les navigateurs bloquent les images relatives en `file://`).

| Dossier | Contenu | Format |
|---|---|---|
| `nav/` | Retour, dossier, accueil | SVG blanc, fond transparent |
| `obs/` | Enregistrement, direct, replay, capture, scène, micro, son, sources | SVG |
| `scenes/` | Starting soon, BRB (pause café), fin, gameplay, just chatting, webcam, bureau | SVG |
| `misc/` | Meme, chrono, marqueur, clip, musique, lecture/suivant/précédent, sourdine casque, clavier, lancement, volume | SVG |
| `apps/` | Logos des applis installées sur ce PC (OBS, Discord, Spotify, Streamer.bot, Steam, Epic, Edge) | PNG 256 px |

## Conventions
- **SVG** : `viewBox="0 0 96 96"`, tracé blanc (`#fff`) épaisseur 6, sans fond. Le fond coloré de la touche est ajouté par l'app, ce qui permet de garder une seule icône pour toutes les couleurs.
- **Tes propres images** : PNG, JPG ou SVG, carrés de préférence (≥ 85 px). Le boîtier affiche 85 × 85 px par touche.

## Ajouter le logo d'une autre appli
```
powershell -ExecutionPolicy Bypass -File scripts\extract-app-icons.ps1 -Exe "C:\chemin\vers\appli.exe" -Name mon-appli
```

Les SVG de `nav/`, `obs/`, `scenes/` et `misc/` ont été dessinés pour ce projet. Les logos de `apps/` sont extraits de tes applis installées et restent la propriété de leurs éditeurs (usage personnel).
