//! Plugin « Micro Windows » : coupe / rétablit le micro par défaut de Windows. Toutes les applis
//! (Discord, OBS, jeu…) l'entendent coupé en même temps, contrairement au muet d'une seule appli.
//!
//! L'état est relu toutes les secondes par un petit fil, seulement quand une touche ou l'écran
//! latéral l'affiche : un micro coupé depuis Windows ou le casque se voit aussi sur le boîtier.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

const UNKNOWN: u8 = 0;
const LIVE: u8 = 1;
const MUTED: u8 = 2;

#[derive(Clone)]
pub struct Mic {
    state: Arc<AtomicU8>,
    wanted: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

impl Default for Mic {
    fn default() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(UNKNOWN)),
            wanted: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Mic {
    /// `Some(true)` = coupé, `Some(false)` = actif, `None` = pas de micro (ou pas encore lu).
    pub fn muted(&self) -> Option<bool> {
        match self.state.load(Ordering::Relaxed) {
            LIVE => Some(false),
            MUTED => Some(true),
            _ => None,
        }
    }

    /// Indique si l'état est affiché quelque part ; démarre le suivi au premier besoin.
    pub fn set_wanted(&self, wanted: bool) {
        self.wanted.store(wanted, Ordering::Relaxed);
        if wanted && !self.started.swap(true, Ordering::Relaxed) {
            let (state, wanted) = (self.state.clone(), self.wanted.clone());
            std::thread::Builder::new()
                .name("micro".into())
                .spawn(move || {
                    loop {
                        if wanted.load(Ordering::Relaxed) {
                            state.store(encode(endpoint::get()), Ordering::Relaxed);
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                })
                .ok();
        }
    }

    /// Bascule coupé / actif (dans un fil à part : Windows peut mettre un instant à répondre).
    pub fn toggle(&self) {
        let state = self.state.clone();
        std::thread::spawn(move || match endpoint::toggle() {
            Ok(muted) => {
                state.store(encode(Ok(muted)), Ordering::Relaxed);
                log::info!("Micro Windows {}", if muted { "coupé" } else { "rétabli" });
            }
            Err(e) => log::warn!("Micro Windows : {e}"),
        });
    }
}

fn encode(r: Result<bool, String>) -> u8 {
    match r {
        Ok(true) => MUTED,
        Ok(false) => LIVE,
        Err(_) => UNKNOWN,
    }
}

#[cfg(windows)]
mod endpoint {
    use windows::Win32::{
        Media::Audio::{Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole},
        System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
    };

    fn volume() -> Result<IAudioEndpointVolume, String> {
        let err = |e: windows::core::Error| e.message().to_string();
        // SAFETY : appels COM standard ; CoInitializeEx peut être appelé plusieurs fois sur un fil.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            let mic = devices.GetDefaultAudioEndpoint(eCapture, eConsole).map_err(|_| "aucun micro dans Windows".to_string())?;
            mic.Activate(CLSCTX_ALL, None).map_err(err)
        }
    }

    pub fn get() -> Result<bool, String> {
        let v = volume()?;
        // SAFETY : interface valide obtenue ci-dessus.
        unsafe { v.GetMute().map(|b| b.as_bool()).map_err(|e| e.message().to_string()) }
    }

    pub fn toggle() -> Result<bool, String> {
        let v = volume()?;
        // SAFETY : interface valide obtenue ci-dessus ; pas de contexte d'événement.
        unsafe {
            let muted = v.GetMute().map_err(|e| e.message().to_string())?.as_bool();
            v.SetMute(!muted, std::ptr::null()).map_err(|e| e.message().to_string())?;
            Ok(!muted)
        }
    }
}

#[cfg(not(windows))]
mod endpoint {
    pub fn get() -> Result<bool, String> {
        Err("Windows seulement".into())
    }
    pub fn toggle() -> Result<bool, String> {
        Err("Windows seulement".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etat_inconnu_au_depart() {
        let m = Mic::default();
        assert_eq!(m.muted(), None);
        assert_eq!(encode(Ok(true)), MUTED);
        assert_eq!(encode(Err("x".into())), UNKNOWN);
    }

    /// Lit vraiment l'état du micro de ce PC (sans le changer) : `cargo test -- --ignored micro_reel`.
    #[test]
    #[ignore]
    fn micro_reel() {
        println!("micro coupé : {:?}", endpoint::get());
        assert!(endpoint::get().is_ok());
    }
}
