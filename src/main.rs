//! soomfon-deck — démon de contrôle du SOOMFON Stream Controller (15 touches).
//!
//! Lancement : `soomfon-deck.exe` (sans fenêtre, démarré avec Windows). Une seule instance à la fois.
//! Journal : `%APPDATA%\soomfon-deck\soomfon-deck.log`.
//! Config : `%APPDATA%\soomfon-deck\config.toml` (relue automatiquement à chaque modification).

// Pas de fenêtre console : le démon tourne en arrière-plan.
#![windows_subsystem = "windows"]

use std::time::Duration;

use soomfon_deck::{
    app::{App, AppEvent},
    config, device, logger, media, obs, streamerbot, watcher,
};
use tokio::time::{MissedTickBehavior, interval};

fn main() {
    logger::init(&config::app_dir().join("soomfon-deck.log"));
    // Les plantages internes vont dans le journal (la tâche concernée est relancée).
    std::panic::set_hook(Box::new(|info| log::error!("Plantage interne : {info}")));

    // `soomfon-deck.exe --media play_pause|next|previous|…` : exécute une commande musique et quitte.
    let args: Vec<String> = std::env::args().collect();
    if let Some(id) = args.iter().position(|a| a == "--media").and_then(|i| args.get(i + 1)) {
        match soomfon_deck::media::MediaKey::ALL.into_iter().find(|k| k.id() == id) {
            Some(key) => soomfon_deck::media::run(key),
            None => log::warn!("--media : commande inconnue « {id} »"),
        }
        return;
    }

    // Une seule instance : deux démons se disputeraient le boîtier.
    if !single_instance() {
        log::info!("soomfon-deck tourne déjà : cette 2e copie s'arrête");
        return;
    }

    // Un seul thread : l'appli passe l'essentiel de son temps à dormir en attendant un événement.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("création du runtime");
    let local = tokio::task::LocalSet::new();
    if let Err(e) = local.block_on(&runtime, run()) {
        log::error!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    log::info!("soomfon-deck {} démarre", env!("CARGO_PKG_VERSION"));
    warn_if_official_app_running();

    let config_path = config::config_path();
    let cfg = config::load_or_create(&config_path)?;
    log::info!("Config : {}", config_path.display());

    let icons_dir = cfg.icons_dir.clone().unwrap_or_else(config::find_icons_dir);
    log::info!("Icônes : {}", icons_dir.display());

    let (events_tx, mut events) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let obs_tx = obs::spawn(cfg.obs.clone(), events_tx.clone());
    let sb_tx = streamerbot::spawn(cfg.streamerbot.clone(), events_tx.clone());

    let mut app = App::new(cfg, icons_dir, obs_tx, sb_tx);
    let music_tx = events_tx.clone();
    media::watch_now_playing(app.music_flag(), move |now| {
        let _ = music_tx.send(AppEvent::Music(now));
    });
    let device = device::spawn(app.initial_frame(), app.brightness(), events_tx.clone());
    app.attach_device(device);

    let _config_watcher = watcher::spawn(config_path, events_tx.clone());

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    // Le chrono n'existe que lorsqu'il est utile (live actif + touche chrono à l'écran).
    let mut ticker: Option<tokio::time::Interval> = None;

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                log::info!("Arrêt demandé");
                break;
            }
            Some(event) = events.recv() => app.handle(event),
            _ = async {
                match ticker.as_mut() {
                    Some(t) => { t.tick().await; }
                    None => std::future::pending::<()>().await,
                }
            } => app.tick(),
        }

        match (app.needs_tick(), ticker.is_some()) {
            (true, false) => {
                let mut t = interval(Duration::from_secs(1));
                t.set_missed_tick_behavior(MissedTickBehavior::Skip);
                ticker = Some(t);
            }
            (false, true) => ticker = None,
            _ => {}
        }
    }

    if let Some(device) = app.take_device() {
        device.shutdown().await;
    }
    log::info!("soomfon-deck arrêté");
    Ok(())
}

/// Le logiciel officiel se bat avec nous pour l'affichage : on prévient s'il tourne.
fn warn_if_official_app_running() {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Stream Controller.exe", "/NH"])
        .output();
    if let Ok(out) = output {
        if String::from_utf8_lossy(&out.stdout).contains("Stream Controller.exe") {
            log::warn!("Le logiciel officiel « Stream Controller » tourne : ferme-le pour éviter les conflits d'affichage");
        }
    }
}

/// Réserve un verrou nommé au niveau de la session Windows. Faux si une autre copie le détient.
#[cfg(windows)]
fn single_instance() -> bool {
    use windows::{
        Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        Win32::System::Threading::CreateMutexW,
        core::w,
    };
    // SAFETY : création d'un mutex nommé ; le handle reste ouvert jusqu'à la fin du processus.
    unsafe {
        let handle = CreateMutexW(None, true, w!("soomfon-deck-daemon"));
        handle.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn single_instance() -> bool {
    true
}
