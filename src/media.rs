//! Touches multimédia : lecture/pause, piste suivante/précédente, volume.
//!
//! Musique : on pilote directement le lecteur via les contrôles multimédias de Windows (SMTC),
//! sans simuler de touche. Priorité au lecteur **en train de jouer**, sinon à **Spotify**,
//! sinon au lecteur que Windows considère comme courant. Ainsi un onglet Twitch/YouTube ouvert
//! ou un jeu au premier plan ne « vole » plus la commande. En secours (aucun lecteur trouvé) :
//! touche multimédia simulée. Volume : touches de volume de Windows.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl MediaKey {
    pub const ALL: [MediaKey; 6] =
        [MediaKey::PlayPause, MediaKey::Next, MediaKey::Previous, MediaKey::VolumeUp, MediaKey::VolumeDown, MediaKey::Mute];

    /// Nom dans le fichier de config.
    pub fn id(self) -> &'static str {
        match self {
            MediaKey::PlayPause => "play_pause",
            MediaKey::Next => "next",
            MediaKey::Previous => "previous",
            MediaKey::VolumeUp => "volume_up",
            MediaKey::VolumeDown => "volume_down",
            MediaKey::Mute => "mute",
        }
    }

    /// Libellé dans l'éditeur.
    pub fn label(self) -> &'static str {
        match self {
            MediaKey::PlayPause => "Lecture / pause",
            MediaKey::Next => "Piste suivante",
            MediaKey::Previous => "Piste précédente",
            MediaKey::VolumeUp => "Volume Windows +",
            MediaKey::VolumeDown => "Volume Windows −",
            MediaKey::Mute => "Couper le son de Windows",
        }
    }

    /// Texte et icône proposés par défaut sur la touche.
    pub fn default_look(self) -> (&'static str, &'static str) {
        match self {
            MediaKey::PlayPause => ("Lecture", "misc/play-pause.svg"),
            MediaKey::Next => ("Suivant", "misc/next.svg"),
            MediaKey::Previous => ("Précédent", "misc/previous.svg"),
            MediaKey::VolumeUp => ("Volume +", "misc/volume-up.svg"),
            MediaKey::VolumeDown => ("Volume −", "misc/volume-down.svg"),
            MediaKey::Mute => ("Muet", "obs/speaker-off.svg"),
        }
    }

    /// Code de touche virtuelle Windows (VK_MEDIA_* / VK_VOLUME_*).
    fn vk(self) -> u8 {
        match self {
            MediaKey::PlayPause => 0xB3,
            MediaKey::Next => 0xB0,
            MediaKey::Previous => 0xB1,
            MediaKey::VolumeUp => 0xAF,
            MediaKey::VolumeDown => 0xAE,
            MediaKey::Mute => 0xAD,
        }
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
}

/// Exécute la commande, hors de la boucle du démon (les appels Windows attendent la réponse du lecteur).
pub fn press(key: MediaKey) {
    std::thread::spawn(move || run(key));
}

/// Exécute la commande et attend la réponse du lecteur.
pub fn run(key: MediaKey) {
    #[cfg(windows)]
    if matches!(key, MediaKey::PlayPause | MediaKey::Next | MediaKey::Previous) {
        match smtc::control(key) {
            Ok(app) => return log::info!("Musique : {} → {app}", key.label()),
            Err(e) => log::info!("Musique : pas de lecteur pilotable ({e}), touche multimédia simulée"),
        }
    }
    send_key(key);
}

/// Musique en cours, pour l'écran latéral.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

/// Surveille la musique en cours (toutes les 2 s, seulement quand `wanted` est vrai) et appelle
/// `notify` à chaque changement. Tourne dans son propre fil : la boucle du démon n'attend jamais.
pub fn watch_now_playing(wanted: Arc<AtomicBool>, notify: impl Fn(Option<NowPlaying>) + Send + 'static) {
    std::thread::spawn(move || {
        let mut last: Option<Option<NowPlaying>> = None;
        #[cfg(windows)]
        let mut manager = None;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !wanted.load(Ordering::Relaxed) {
                last = None;
                continue;
            }
            #[cfg(windows)]
            let now = smtc::now_playing(&mut manager);
            #[cfg(not(windows))]
            let now = None;
            if last.as_ref() != Some(&now) {
                notify(now.clone());
                last = Some(now);
            }
        }
    });
}

#[cfg(windows)]
mod smtc {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    use super::{MediaKey, NowPlaying};

    fn app(s: &Session) -> String {
        s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default()
    }

    /// Lecteur visé : celui qui joue, sinon Spotify, sinon le lecteur courant de Windows.
    /// Renvoie aussi s'il est en train de jouer.
    fn pick(manager: &Manager) -> Option<(Session, bool)> {
        let sessions: Vec<Session> = manager.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
        let is_playing =
            |s: &Session| s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).is_ok_and(|st| st == Status::Playing);
        if let Some(s) = sessions.iter().find(|s| is_playing(s)) {
            return Some((s.clone(), true));
        }
        let spotify = sessions.iter().find(|s| app(s).to_lowercase().contains("spotify")).cloned();
        spotify.or_else(|| manager.GetCurrentSession().ok()).map(|s| (s, false))
    }

    /// Titre / artiste du lecteur visé. Le gestionnaire Windows est gardé entre deux appels.
    pub fn now_playing(manager: &mut Option<Manager>) -> Option<NowPlaying> {
        if manager.is_none() {
            *manager = Manager::RequestAsync().and_then(|op| op.join()).ok();
        }
        let (session, playing) = pick(manager.as_ref()?)?;
        let props = session.TryGetMediaPropertiesAsync().and_then(|op| op.join()).ok()?;
        let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
        let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
        (!title.trim().is_empty()).then(|| NowPlaying { title, artist, playing })
    }

    /// Choisit le lecteur et lui envoie la commande. Renvoie le nom de l'appli pilotée.
    pub fn control(key: MediaKey) -> Result<String, String> {
        let err = |e: windows::core::Error| e.message().to_string();
        let manager = Manager::RequestAsync().and_then(|op| op.join()).map_err(err)?;
        let (target, playing) = pick(&manager).ok_or("aucun lecteur ouvert")?;
        let op = match key {
            // Rien ne joue : on relance (et non « bascule », qui pourrait mettre en pause un lecteur
            // dont l'état est mal rapporté) ; quelque chose joue : on met en pause.
            MediaKey::PlayPause if playing => target.TryPauseAsync(),
            MediaKey::PlayPause => target.TryPlayAsync(),
            MediaKey::Next => target.TrySkipNextAsync(),
            MediaKey::Previous => target.TrySkipPreviousAsync(),
            _ => return Err("commande non gérée".into()),
        };
        let accepted = op.and_then(|op| op.join()).map_err(err)?;
        if accepted { Ok(app(&target)) } else { Err(format!("{} a refusé la commande", app(&target))) }
    }
}

/// Appuie puis relâche la touche multimédia (secours, et volume).
#[cfg(windows)]
fn send_key(key: MediaKey) {
    const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    // SAFETY : appel Win32 sans pointeur ; simule l'appui puis le relâchement d'une touche.
    unsafe {
        keybd_event(key.vk(), 0, KEYEVENTF_EXTENDEDKEY, 0);
        keybd_event(key.vk(), 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
    }
}

#[cfg(not(windows))]
fn send_key(_key: MediaKey) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiants_uniques_et_relus() {
        for k in MediaKey::ALL {
            let parsed: MediaKey = serde_json::from_value(serde_json::json!(k.id())).unwrap();
            assert_eq!(parsed, k);
        }
    }
}
