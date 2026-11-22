//! Coil sur le Stream Deck : pause imposée entre deux taffes de cigarette électronique, pour que
//! le coton se ré-imbibe et que la résistance dure (même principe que l'appli iPhone Coil).
//!
//! Un appui sur une touche Coil = « j'ai tiré » : la pause démarre. Appuyer pendant la pause ne
//! la relance pas, la touche affiche « ATTENDS ». À la fin, un petit son, puis « PRÊT ».
//! Le nombre de taffes du jour est gardé dans `%APPDATA%\soomfon-deck\coil.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::time::{Duration, Instant};

use crate::clock;

/// Durée du « PRÊT » en surbrillance après la pause, et de l'avertissement « ATTENDS ».
const READY_FLASH: Duration = Duration::from_secs(4);
const WAIT_FLASH: Duration = Duration::from_millis(1500);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    day: String,
    count: u32,
}

/// Ce qu'il faut pour dessiner Coil à un instant donné.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoilView {
    /// Secondes restantes (arrondies au-dessus) ; `None` hors pause.
    pub remaining: Option<u32>,
    /// Avancement de la pause, 0 (taffe à l'instant) → 1 (fin).
    pub progress: f32,
    /// Chaleur de la résistance, 1 (taffe à l'instant) → 0 (refroidie).
    pub heat: f32,
    pub today: u32,
    /// Appui pendant la pause : « ATTENDS ».
    pub waiting: bool,
    /// Pause tout juste terminée : « PRÊT » en surbrillance.
    pub ready: bool,
    /// Compteur de secondes pour animer les braises.
    pub phase: u32,
}

pub struct Coil {
    path: PathBuf,
    started: Option<Instant>,
    duration: Duration,
    day: String,
    today: u32,
    wait_until: Option<Instant>,
    ready_until: Option<Instant>,
}

pub enum Press {
    Started,
    /// Pause en cours : on rappelle d'attendre.
    Wait,
}

fn today_key() -> String {
    let t = clock::now();
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

impl Coil {
    pub fn load(path: PathBuf) -> Self {
        let saved: Saved =
            std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut coil = Self {
            path,
            started: None,
            duration: Duration::from_secs(15),
            day: saved.day,
            today: saved.count,
            wait_until: None,
            ready_until: None,
        };
        coil.rollover();
        coil
    }

    /// Nouveau jour : le compteur repart de zéro.
    fn rollover(&mut self) {
        let key = today_key();
        if self.day != key {
            self.day = key;
            self.today = 0;
        }
    }

    fn save(&self) {
        let saved = Saved { day: self.day.clone(), count: self.today };
        if let Ok(text) = serde_json::to_string(&saved) {
            let _ = std::fs::write(&self.path, text);
        }
    }

    pub fn press(&mut self, seconds: u32, now: Instant) -> Press {
        if self.started.is_some_and(|s| now < s + self.duration) {
            self.wait_until = Some(now + WAIT_FLASH);
            return Press::Wait;
        }
        self.rollover();
        self.today += 1;
        self.save();
        self.started = Some(now);
        self.duration = Duration::from_secs(seconds.max(1) as u64);
        self.ready_until = None;
        self.wait_until = None;
        Press::Started
    }

    /// À appeler à chaque battement : vrai à l'instant où la pause se termine.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.started {
            Some(s) if now >= s + self.duration => {
                self.started = None;
                self.ready_until = Some(now + READY_FLASH);
                true
            }
            _ => false,
        }
    }

    /// Faut-il un battement (pause en cours ou message à effacer) ?
    pub fn active(&self, now: Instant) -> bool {
        self.started.is_some() || self.ready_until.is_some_and(|t| now < t) || self.wait_until.is_some_and(|t| now < t)
    }

    pub fn view(&self, now: Instant) -> CoilView {
        let (remaining, progress) = match self.started {
            Some(s) => {
                let elapsed = now.saturating_duration_since(s).as_secs_f32();
                let total = self.duration.as_secs_f32();
                let left = (total - elapsed).max(0.0);
                (Some(left.ceil() as u32), (elapsed / total).clamp(0.0, 1.0))
            }
            None => (None, 1.0),
        };
        CoilView {
            remaining,
            progress,
            heat: if remaining.is_some() { 1.0 - progress } else { 0.0 },
            today: if self.day == today_key() { self.today } else { 0 },
            waiting: remaining.is_some() && self.wait_until.is_some_and(|t| now < t),
            ready: remaining.is_none() && self.ready_until.is_some_and(|t| now < t),
            phase: self.started.map(|s| now.saturating_duration_since(s).as_secs() as u32).unwrap_or(0),
        }
    }
}

/// Petit son de fin de pause (son de notification de Windows), sans bloquer.
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
    fn pause_puis_pret() {
        let dir = std::env::temp_dir().join(format!("coil-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Coil::load(dir.join("coil.json"));
        let t0 = Instant::now();
        let before = c.view(t0).today;
        assert!(matches!(c.press(15, t0), Press::Started));
        let v = c.view(t0 + Duration::from_millis(3200));
        assert_eq!(v.remaining, Some(12));
        assert!(v.heat > 0.7);
        assert_eq!(v.today, before + 1);
        // Appui pendant la pause : pas de nouvelle taffe comptée.
        assert!(matches!(c.press(15, t0 + Duration::from_secs(5)), Press::Wait));
        assert!(c.view(t0 + Duration::from_secs(5)).waiting);
        assert_eq!(c.view(t0 + Duration::from_secs(5)).today, before + 1);
        // Fin de la pause.
        assert!(!c.tick(t0 + Duration::from_secs(14)));
        assert!(c.tick(t0 + Duration::from_secs(15)));
        let end = c.view(t0 + Duration::from_secs(16));
        assert_eq!(end.remaining, None);
        assert!(end.ready);
        // Le compteur est gardé sur disque.
        assert_eq!(Coil::load(dir.join("coil.json")).view(t0).today, before + 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
