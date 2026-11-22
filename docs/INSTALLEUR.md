# Installeur Windows (.msi)

`target/installer/Soomfon-Deck-<version>.msi` installe le Stream Deck sur un PC, sans droits administrateur.

## Ce qu'il fait

| | |
|---|---|
| Dossier | `%LOCALAPPDATA%\Programs\Blackat` (le même que la restauration d'un pack) : `soomfon-deck.exe`, `soomfon-editor.exe`, `blackat-launcher.exe`, `resources\icons\` |
| Menu Démarrer | **Soomfon-Deck** → éditeur ; **Blackat** → launcher. Taper « soomfon-deck » dans la recherche de la barre des tâches ouvre l'éditeur |
| Démarrage avec Windows | entrée « Soomfon Deck » (`HKCU\…\Run`) ; décochable dans l'éditeur (« Démarrer avec Windows ») ou le Gestionnaire des tâches |
| Pendant l'installation | arrête le Stream Deck s'il tourne (fichier verrouillé), puis le relance à la fin |
| Mise à jour | une nouvelle version (ou la même, régénérée) remplace l'ancienne |
| Désinstallation | Paramètres → Applications → « Soomfon Deck (Blackat) » : retire fichiers, raccourcis et démarrage auto. La config (`%APPDATA%\soomfon-deck`) est conservée |

Le `.msi` n'est pas signé : au premier lancement, Windows SmartScreen peut afficher « Windows a protégé votre ordinateur » → **Informations complémentaires** → **Exécuter quand même**.

## Fabriquer le .msi

Outil : WiX Toolset 3.14.1 (`wix314-binaries.zip`, github.com/wixtoolset/wix3), décompressé dans `C:\dev\tools\wix314` (aucune installation).

```
cargo build --release --workspace
python installer/make-msi.py --wix C:\dev\tools\wix314
```

`installer/make-msi.py` génère `target/installer/soomfon-deck.wxs` (liste des icônes comprise), puis appelle `candle` et `light`. Le Stream Deck en cours d'exécution verrouille `soomfon-deck.exe` : l'arrêter avant `cargo build`.

Le `UpgradeCode` du script ne doit jamais changer : c'est lui qui permet à une nouvelle version de remplacer l'ancienne.
