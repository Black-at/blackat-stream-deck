//! Plugin « Soundboard » : la touche joue un son (mp3, wav, ogg, m4a, wma…).
//!
//! Lecture par le lecteur intégré à Windows (MCI) : aucune bibliothèque audio en plus, rien ne
//! tourne quand aucun son ne joue. Plusieurs sons peuvent se superposer ; appuyer sur une touche
//! dont le son joue encore l'arrête. Le son sort sur la sortie audio par défaut de Windows
//! (à capturer dans OBS avec « Capture audio du bureau » ou une capture d'application).

use std::sync::{
    OnceLock,
    mpsc::{Sender, channel},
};

/// Extensions proposées dans l'éditeur.
pub const EXTENSIONS: [&str; 7] = ["mp3", "wav", "ogg", "m4a", "wma", "aac", "flac"];

/// Volume par défaut d'une touche (0 à 100).
pub const DEFAULT_VOLUME: u8 = 80;

struct Play {
    file: String,
    volume: u8,
}

/// Les commandes MCI doivent toutes partir du même fil : un seul fil « lecteur », démarré
/// au premier son et endormi le reste du temps.
fn player() -> &'static Sender<Play> {
    static PLAYER: OnceLock<Sender<Play>> = OnceLock::new();
    PLAYER.get_or_init(|| {
        let (tx, rx) = channel::<Play>();
        std::thread::Builder::new()
            .name("soundboard".into())
            .spawn(move || {
                while let Ok(p) = rx.recv() {
                    toggle(&p.file, p.volume);
                }
            })
            .expect("fil soundboard");
        tx
    })
}

/// Joue le son (ou l'arrête s'il joue encore).
pub fn play(file: &str, volume: u8) {
    let file = file.trim().trim_matches('"').to_string();
    if file.is_empty() {
        log::warn!("Touche soundboard sans fichier");
        return;
    }
    if !std::path::Path::new(&file).is_file() {
        log::warn!("Soundboard : fichier introuvable « {file} »");
        return;
    }
    let _ = player().send(Play { file, volume: volume.min(100) });
}

/// Nom court (alias MCI) propre à un fichier.
fn alias(file: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file.to_lowercase().hash(&mut h);
    format!("sb{:x}", h.finish())
}

#[cfg(windows)]
fn mci(cmd: &str) -> Result<String, String> {
    use windows::{Win32::Media::Multimedia::mciSendStringW, core::HSTRING};
    let mut buf = [0u16; 128];
    // SAFETY : commande et tampon valides pendant l'appel ; pas de fenêtre de rappel.
    let err = unsafe { mciSendStringW(&HSTRING::from(cmd), Some(&mut buf), None) };
    if err == 0 {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Ok(String::from_utf16_lossy(&buf[..end]))
    } else {
        Err(format!("erreur MCI {err}"))
    }
}

#[cfg(windows)]
fn toggle(file: &str, volume: u8) {
    let a = alias(file);
    if mci(&format!("status {a} mode")).is_ok_and(|m| m == "playing") {
        let _ = mci(&format!("close {a}"));
        log::info!("Soundboard : arrêt de {file}");
        return;
    }
    let _ = mci(&format!("close {a}"));
    let result = mci(&format!("open \"{file}\" type mpegvideo alias {a}"))
        .and_then(|_| mci(&format!("setaudio {a} volume to {}", volume as u32 * 10)))
        .and_then(|_| mci(&format!("play {a} from 0")));
    match result {
        Ok(_) => log::info!("Soundboard : {file} (volume {volume})"),
        Err(e) => {
            let _ = mci(&format!("close {a}"));
            log::warn!("Soundboard : impossible de jouer « {file} » ({e})");
        }
    }
}

#[cfg(not(windows))]
fn toggle(_file: &str, _volume: u8) {}

/// Petit son système (fin d'un compte à rebours).
#[cfg(windows)]
pub fn chime() {
    use windows::{
        Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT},
        core::HSTRING,
    };
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let file = HSTRING::from(format!(r"{windir}\Media\Windows Notify System Generic.wav"));
    // SAFETY : chaîne valide le temps de l'appel ; lecture asynchrone gérée par Windows.
    unsafe {
        let _ = PlaySoundW(&file, None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
    }
}

#[cfg(not(windows))]
pub fn chime() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_stable_et_insensible_a_la_casse() {
        assert_eq!(alias(r"C:\Sons\Bruh.mp3"), alias(r"c:\sons\bruh.MP3"));
        assert_ne!(alias(r"C:\Sons\a.mp3"), alias(r"C:\Sons\b.mp3"));
        assert!(alias("x").chars().all(|c| c.is_ascii_alphanumeric()));
    }

    /// Joue vraiment un son de Windows : `cargo test -- --ignored son_windows`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn son_windows() {
        let windir = std::env::var("WINDIR").unwrap();
        let file = format!(r"{windir}\Media\Alarm01.wav");
        toggle(&file, 50);
        assert_eq!(mci(&format!("status {} mode", alias(&file))).unwrap(), "playing");
        toggle(&file, 50);
        assert!(mci(&format!("status {} mode", alias(&file))).is_err());
    }
}
