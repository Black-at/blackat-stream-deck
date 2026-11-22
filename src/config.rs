//! Fichier de configuration (TOML) : pages, touches, actions.
//!
//! Emplacement : `%APPDATA%\soomfon-deck\config.toml`. Créé avec un exemple au premier lancement.
//! La première page du fichier est la page racine ; les autres sont des dossiers.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    layout::{BACK_POS, KEYS, SIDE},
    media::MediaKey,
};

/// Exemple écrit au premier lancement.
pub const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Luminosité des écrans, 0 à 100.
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    /// Qualité JPEG des images envoyées (plus bas = envoi USB plus rapide).
    #[serde(default = "default_quality")]
    pub jpeg_quality: u8,
    /// Dossier des icônes. Par défaut : `resources/icons` à côté du programme.
    #[serde(default)]
    pub icons_dir: Option<PathBuf>,
    #[serde(default)]
    pub obs: ObsConfig,
    #[serde(default)]
    pub streamerbot: StreamerbotConfig,
    /// Écran latéral (à droite des touches), de haut en bas.
    #[serde(default = "default_side")]
    pub side: Vec<SideWidget>,
    #[serde(rename = "page")]
    pub pages: Vec<Page>,
}

/// Contenu d'une case de l'écran latéral (affichage seul).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideWidget {
    /// Case noire.
    None,
    /// Titre et artiste de la musique en cours (Spotify ou autre lecteur).
    Music,
    /// Heure et date.
    Clock,
    /// Logo Blackat.
    Logo,
    /// Durée du live (et état de l'enregistrement).
    StreamTimer,
    /// Scène OBS affichée.
    Scene,
    /// Coil : cadran de la pause entre deux taffes.
    Coil,
}

impl SideWidget {
    /// Ordre proposé dans l'éditeur.
    pub const ALL: [SideWidget; 7] = [
        SideWidget::Music,
        SideWidget::Clock,
        SideWidget::Logo,
        SideWidget::StreamTimer,
        SideWidget::Scene,
        SideWidget::Coil,
        SideWidget::None,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SideWidget::None => "none",
            SideWidget::Music => "music",
            SideWidget::Clock => "clock",
            SideWidget::Logo => "logo",
            SideWidget::StreamTimer => "stream_timer",
            SideWidget::Scene => "scene",
            SideWidget::Coil => "coil",
        }
    }
}

fn default_side() -> Vec<SideWidget> {
    vec![SideWidget::Music, SideWidget::Clock, SideWidget::Logo]
}

/// Connexion à OBS. Tout est facultatif : par défaut on lit les réglages du serveur WebSocket
/// directement dans la configuration d'OBS.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObsConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
}

/// Connexion à Streamer.bot. Tout est facultatif : par défaut on lit l'adresse et le port du
/// serveur WebSocket dans les réglages de Streamer.bot. Le mot de passe n'est utile que si
/// l'authentification est activée dans Streamer.bot.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamerbotConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    #[serde(default)]
    pub keys: Vec<KeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyConfig {
    /// Position 0..14, ligne par ligne de gauche à droite.
    pub pos: usize,
    /// Chemin relatif au dossier d'icônes (ex. `obs/record.svg`) ou chemin absolu.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// Couleur de fond `#rrggbb`.
    #[serde(default)]
    pub color: Option<String>,
    pub action: Action,
}

/// Ce que fait une touche.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Change la scène affichée en direct.
    ObsScene { scene: String },
    /// Démarre / arrête l'enregistrement.
    ObsRecord,
    /// Met en pause / reprend l'enregistrement.
    ObsRecordPause,
    /// Démarre / arrête le live.
    ObsStream,
    /// Relance une source média depuis le début (memes, sons…).
    ObsMedia { input: String },
    /// Coupe / rétablit le son d'une entrée audio.
    ObsMute { input: String },
    /// Affiche / masque une source dans une scène.
    ObsSource { scene: String, source: String },
    /// Sauvegarde le replay buffer.
    ObsReplaySave,
    /// Affiche la durée du live (aucune action à l'appui).
    StreamTimer,
    /// Ouvre un dossier (une autre page du fichier).
    Folder { page: String },
    /// Ouvre un dossier rempli automatiquement avec les scènes d'OBS.
    ObsScenesFolder,
    /// Touche multimédia Windows : musique (Spotify…) ou volume.
    Media { key: MediaKey },
    /// Lance une action Streamer.bot (par son nom).
    Streamerbot { action: String },
    /// Coil : « j'ai tiré », pause imposée de `seconds` avant la prochaine taffe.
    Coil { seconds: u32 },
    /// Raccourci clavier ou petite macro : `ctrl+shift+m`, ou `ctrl+c, ctrl+v` (voir plugins/hotkey.rs).
    Hotkey { keys: String },
    /// Ouvre un programme, un fichier, un dossier ou un site web.
    Launch {
        target: String,
        #[serde(default)]
        args: String,
    },
    /// Soundboard : joue un son (mp3, wav…) ; un nouvel appui l'arrête.
    Sound {
        file: String,
        #[serde(default = "default_volume")]
        volume: u8,
    },
    /// Compte à rebours affiché sur la touche ; un nouvel appui l'annule.
    Countdown { seconds: u32 },
    /// Touche décorative.
    None,
}

impl Action {
    /// L'action dépend-elle d'OBS (pour griser la touche si OBS est absent) ?
    pub fn needs_obs(&self) -> bool {
        matches!(
            self,
            Action::ObsScene { .. }
                | Action::ObsRecord
                | Action::ObsRecordPause
                | Action::ObsStream
                | Action::ObsMedia { .. }
                | Action::ObsMute { .. }
                | Action::ObsSource { .. }
                | Action::ObsReplaySave
                | Action::StreamTimer
                | Action::ObsScenesFolder
        )
    }
}

fn default_volume() -> u8 {
    crate::plugins::sound::DEFAULT_VOLUME
}

fn default_brightness() -> u8 {
    70
}

fn default_quality() -> u8 {
    80
}

/// Chemin du fichier de config.
pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}

/// `%APPDATA%\soomfon-deck`
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("soomfon-deck")
}

/// Cherche `resources/icons` à côté de l'exécutable, puis en remontant (cas `target/release`),
/// puis dans le dossier courant.
pub fn find_icons_dir() -> PathBuf {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            candidates.push(dir.join("resources").join("icons"));
        }
    }
    candidates.push(PathBuf::from("resources").join("icons"));
    candidates.iter().find(|p| p.is_dir()).cloned().unwrap_or_else(|| {
        log::warn!("Dossier resources/icons introuvable : les touches n'auront pas d'icône");
        candidates.remove(0)
    })
}

/// Charge la config ; la crée à partir de l'exemple si elle n'existe pas encore.
pub fn load_or_create(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("création de {}: {e}", dir.display()))?;
        }
        std::fs::write(path, DEFAULT_CONFIG).map_err(|e| format!("écriture de {}: {e}", path.display()))?;
        log::info!("Config d'exemple créée : {}", path.display());
    }
    load(path)
}

/// Lit et valide la config.
pub fn load(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("lecture de {}: {e}", path.display()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Config, String> {
    let cfg: Config = toml::from_str(text).map_err(|e| format!("config invalide : {e}"))?;
    cfg.validate()?;
    Ok(cfg)
}

impl Config {
    /// Vérifie les règles de navigation et de placement. Renvoie toutes les erreurs d'un coup.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        if self.pages.is_empty() {
            errors.push("il faut au moins une page (la page racine)".to_string());
        }
        if self.brightness > 100 {
            errors.push(format!("brightness = {} : doit être entre 0 et 100", self.brightness));
        }
        if !(30..=100).contains(&self.jpeg_quality) {
            errors.push(format!("jpeg_quality = {} : doit être entre 30 et 100", self.jpeg_quality));
        }
        if self.side.len() > SIDE {
            errors.push(format!("side : {} cases au plus (écran latéral)", SIDE));
        }

        let mut ids = HashSet::new();
        for page in &self.pages {
            if !ids.insert(page.id.as_str()) {
                errors.push(format!("page « {} » définie deux fois", page.id));
            }
        }

        let root_id = self.pages.first().map(|p| p.id.as_str());
        for (index, page) in self.pages.iter().enumerate() {
            let is_folder = index > 0;
            let mut used = HashSet::new();
            for key in &page.keys {
                let at = format!("page « {} », touche {}", page.id, key.pos);
                if key.pos >= KEYS {
                    errors.push(format!("{at} : position hors limites (0 à {})", KEYS - 1));
                }
                if !used.insert(key.pos) {
                    errors.push(format!("{at} : position utilisée deux fois"));
                }
                if is_folder && key.pos == BACK_POS {
                    errors.push(format!("{at} : la position {BACK_POS} est réservée à la touche Retour"));
                }
                if let Some(color) = &key.color {
                    if parse_color(color).is_none() {
                        errors.push(format!("{at} : couleur « {color} » invalide (format #rrggbb)"));
                    }
                }
                match &key.action {
                    Action::Folder { page: target } => {
                        if is_folder {
                            errors.push(format!("{at} : pas de dossier dans un dossier (1 seul niveau)"));
                        } else if Some(target.as_str()) == root_id {
                            errors.push(format!("{at} : un dossier ne peut pas pointer vers la page racine"));
                        } else if !ids.contains(target.as_str()) {
                            errors.push(format!("{at} : page « {target} » introuvable"));
                        }
                    }
                    Action::ObsScenesFolder if is_folder => {
                        errors.push(format!("{at} : pas de dossier dans un dossier (1 seul niveau)"));
                    }
                    Action::Sound { volume, .. } if *volume > 100 => {
                        errors.push(format!("{at} : volume = {volume} : doit être entre 0 et 100"));
                    }
                    Action::Countdown { seconds: 0 } | Action::Coil { seconds: 0 } => {
                        errors.push(format!("{at} : la durée doit être d'au moins 1 seconde"));
                    }
                    _ => {}
                }
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    /// Index d'une page à partir de son id.
    pub fn page_index(&self, id: &str) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }
}

// ── Écriture (utilisée par l'éditeur) ─────────────────────────────────────────

/// En-tête placé en haut du fichier écrit par l'éditeur.
const SAVED_HEADER: &str = "\
# ─────────────────────────────────────────────────────────────────────────────
#  soomfon-deck — configuration (écrite par soomfon-editor, modifiable à la main)
#
#  Positions des touches (pos) :
#      0   1   2   3   4
#      5   6   7   8   9
#     10  11  12  13  14   ← dans un dossier, 14 = Retour (automatique)
#
#  La première page est la page racine, les suivantes sont des dossiers.
# ─────────────────────────────────────────────────────────────────────────────
";

/// Chaîne TOML correctement échappée.
fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn action_inline(a: &Action) -> String {
    let t = |name: &str| format!("{{ type = \"{name}\" }}");
    match a {
        Action::ObsScene { scene } => format!("{{ type = \"obs_scene\", scene = {} }}", q(scene)),
        Action::ObsRecord => t("obs_record"),
        Action::ObsRecordPause => t("obs_record_pause"),
        Action::ObsStream => t("obs_stream"),
        Action::ObsMedia { input } => format!("{{ type = \"obs_media\", input = {} }}", q(input)),
        Action::ObsMute { input } => format!("{{ type = \"obs_mute\", input = {} }}", q(input)),
        Action::ObsSource { scene, source } => {
            format!("{{ type = \"obs_source\", scene = {}, source = {} }}", q(scene), q(source))
        }
        Action::ObsReplaySave => t("obs_replay_save"),
        Action::StreamTimer => t("stream_timer"),
        Action::Folder { page } => format!("{{ type = \"folder\", page = {} }}", q(page)),
        Action::ObsScenesFolder => t("obs_scenes_folder"),
        Action::Media { key } => format!("{{ type = \"media\", key = \"{}\" }}", key.id()),
        Action::Streamerbot { action } => format!("{{ type = \"streamerbot\", action = {} }}", q(action)),
        Action::Coil { seconds } => format!("{{ type = \"coil\", seconds = {seconds} }}"),
        Action::Hotkey { keys } => format!("{{ type = \"hotkey\", keys = {} }}", q(keys)),
        Action::Launch { target, args } if args.is_empty() => format!("{{ type = \"launch\", target = {} }}", q(target)),
        Action::Launch { target, args } => {
            format!("{{ type = \"launch\", target = {}, args = {} }}", q(target), q(args))
        }
        Action::Sound { file, volume } => format!("{{ type = \"sound\", file = {}, volume = {volume} }}", q(file)),
        Action::Countdown { seconds } => format!("{{ type = \"countdown\", seconds = {seconds} }}"),
        Action::None => t("none"),
    }
}

/// Sérialise la config dans le même style que l'exemple : une touche par ligne.
pub fn to_toml(cfg: &Config) -> String {
    let mut out = String::from(SAVED_HEADER);
    out.push_str(&format!("\nbrightness = {}      # 0 à 100\n", cfg.brightness));
    out.push_str(&format!("jpeg_quality = {}    # 30 à 100\n", cfg.jpeg_quality));
    if let Some(dir) = &cfg.icons_dir {
        out.push_str(&format!("icons_dir = {}\n", q(&dir.to_string_lossy())));
    }
    // Avant les sections [obs] / [streamerbot] : c'est une valeur de premier niveau.
    let side: Vec<String> = cfg.side.iter().map(|w| q(w.id())).collect();
    out.push_str(&format!(
        "side = [{}]    # écran de droite, de haut en bas : music, clock, logo, stream_timer, scene, coil, none\n",
        side.join(", ")
    ));
    if cfg.obs != ObsConfig::default() {
        out.push_str("\n[obs]\n");
        if let Some(h) = &cfg.obs.host {
            out.push_str(&format!("host = {}\n", q(h)));
        }
        if let Some(p) = cfg.obs.port {
            out.push_str(&format!("port = {p}\n"));
        }
        if let Some(pw) = &cfg.obs.password {
            out.push_str(&format!("password = {}\n", q(pw)));
        }
    }
    if cfg.streamerbot != StreamerbotConfig::default() {
        out.push_str("\n[streamerbot]\n");
        if let Some(h) = &cfg.streamerbot.host {
            out.push_str(&format!("host = {}\n", q(h)));
        }
        if let Some(p) = cfg.streamerbot.port {
            out.push_str(&format!("port = {p}\n"));
        }
        if let Some(pw) = &cfg.streamerbot.password {
            out.push_str(&format!("password = {}\n", q(pw)));
        }
    }

    for page in &cfg.pages {
        out.push_str(&format!("\n[[page]]\nid = {}\nkeys = [\n", q(&page.id)));
        let mut keys: Vec<&KeyConfig> = page.keys.iter().collect();
        keys.sort_by_key(|k| k.pos);
        for k in keys {
            let mut fields = vec![format!("pos = {}", k.pos)];
            if let Some(i) = &k.icon {
                fields.push(format!("icon = {}", q(i)));
            }
            if let Some(l) = &k.label {
                fields.push(format!("label = {}", q(l)));
            }
            if let Some(c) = &k.color {
                fields.push(format!("color = {}", q(c)));
            }
            fields.push(format!("action = {}", action_inline(&k.action)));
            out.push_str(&format!("  {{ {} }},\n", fields.join(", ")));
        }
        out.push_str("]\n");
    }
    out
}

/// Écrit la config de façon atomique (fichier temporaire puis renommage) : le démon ne lit
/// jamais un fichier à moitié écrit. Refuse d'écrire une config invalide.
pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    cfg.validate()?;
    let text = to_toml(cfg);
    // Garde-fou : ce qu'on écrit doit se relire à l'identique.
    parse(&text).map_err(|e| format!("erreur interne d'écriture de la config : {e}"))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("écriture de {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("remplacement de {}: {e}", path.display()))
}

/// `#rrggbb` → [r, g, b]
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exemple_par_defaut_valide() {
        parse(DEFAULT_CONFIG).expect("la config d'exemple doit être valide");
    }

    #[test]
    fn dossier_dans_un_dossier_refuse() {
        let err = parse(
            r#"
            [[page]]
            id = "racine"
            keys = [{ pos = 0, action = { type = "folder", page = "a" } }]
            [[page]]
            id = "a"
            keys = [{ pos = 0, action = { type = "folder", page = "b" } }]
            [[page]]
            id = "b"
            "#,
        )
        .unwrap_err();
        assert!(err.contains("pas de dossier dans un dossier"), "{err}");
    }

    #[test]
    fn position_retour_reservee() {
        let err = parse(
            r#"
            [[page]]
            id = "racine"
            [[page]]
            id = "a"
            keys = [{ pos = 14, action = { type = "obs_record" } }]
            "#,
        )
        .unwrap_err();
        assert!(err.contains("réservée à la touche Retour"), "{err}");
    }

    #[test]
    fn page_inconnue_et_position_hors_limites() {
        let err = parse(
            r#"
            [[page]]
            id = "racine"
            keys = [
              { pos = 0, action = { type = "folder", page = "nulle-part" } },
              { pos = 15, action = { type = "obs_stream" } },
            ]
            "#,
        )
        .unwrap_err();
        assert!(err.contains("introuvable"), "{err}");
        assert!(err.contains("hors limites"), "{err}");
    }

    #[test]
    fn position_14_autorisee_a_la_racine() {
        parse(
            r#"
            [[page]]
            id = "racine"
            keys = [{ pos = 14, action = { type = "obs_record" } }]
            "#,
        )
        .unwrap();
    }

    #[test]
    fn ecriture_puis_relecture_identique() {
        let mut cfg = parse(DEFAULT_CONFIG).unwrap();
        // Caractères piégeux dans les noms.
        cfg.pages[1].keys.push(KeyConfig {
            pos: 3,
            icon: Some(r"C:\Mes images\meme.png".into()),
            label: Some("Il a dit \"bruh\"".into()),
            color: Some("#5a2a7a".into()),
            action: Action::ObsSource { scene: "écran 1".into(), source: "Caméra #2".into() },
        });
        cfg.obs.port = Some(4456);
        cfg.streamerbot.port = Some(8081);
        cfg.pages[1].keys.push(KeyConfig {
            pos: 4,
            icon: None,
            label: None,
            color: None,
            action: Action::Media { key: MediaKey::Next },
        });
        cfg.pages[1].keys.push(KeyConfig {
            pos: 5,
            icon: Some("apps/streamerbot.png".into()),
            label: Some("Clip".into()),
            color: None,
            action: Action::Streamerbot { action: "Créer un clip \"rapide\"".into() },
        });
        let plugins = [
            Action::Hotkey { keys: "ctrl+shift+m, alt+tab".into() },
            Action::Launch { target: r"C:\Program Files\obs-studio\bin\64bit\obs64.exe".into(), args: String::new() },
            Action::Launch { target: "https://twitch.tv".into(), args: "--new-window \"x\"".into() },
            Action::Sound { file: r"C:\Sons\bruh.mp3".into(), volume: 35 },
            Action::Countdown { seconds: 300 },
        ];
        for (i, action) in plugins.iter().enumerate() {
            cfg.pages[1].keys.push(KeyConfig { pos: 6 + i, icon: None, label: None, color: None, action: action.clone() });
        }
        cfg.side = vec![SideWidget::Scene, SideWidget::None, SideWidget::StreamTimer];
        let text = to_toml(&cfg);
        let back = parse(&text).unwrap();
        assert_eq!(to_toml(&back), text);
        assert_eq!(back.side, vec![SideWidget::Scene, SideWidget::None, SideWidget::StreamTimer]);
        assert_eq!(back.pages[1].keys[0].label.as_deref(), Some("Il a dit \"bruh\""));
        assert_eq!(back.obs.port, Some(4456));
        assert_eq!(back.streamerbot.port, Some(8081));
        assert!(back.pages[1].keys.iter().any(|k| k.action == Action::Media { key: MediaKey::Next }));
        assert!(back.pages[1].keys.iter().any(|k| k.action == Action::Streamerbot { action: "Créer un clip \"rapide\"".into() }));
        for action in &plugins {
            assert!(back.pages[1].keys.iter().any(|k| k.action == *action), "{action:?}");
        }
    }

    #[test]
    fn plugins_valeurs_par_defaut_et_limites() {
        let cfg = parse(
            r#"
            [[page]]
            id = "racine"
            keys = [
              { pos = 0, action = { type = "sound", file = "a.mp3" } },
              { pos = 1, action = { type = "launch", target = "https://twitch.tv" } },
            ]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.pages[0].keys[0].action, Action::Sound { file: "a.mp3".into(), volume: 80 });
        assert!(!cfg.pages[0].keys[1].action.needs_obs());
        let err = parse(
            r#"
            [[page]]
            id = "racine"
            keys = [
              { pos = 0, action = { type = "sound", file = "a.mp3", volume = 150 } },
              { pos = 1, action = { type = "countdown", seconds = 0 } },
            ]
            "#,
        )
        .unwrap_err();
        assert!(err.contains("volume = 150"), "{err}");
        assert!(err.contains("au moins 1 seconde"), "{err}");
    }

    #[test]
    fn couleurs() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_color("ff8000"), None);
        assert_eq!(parse_color("#fff"), None);
    }
}
