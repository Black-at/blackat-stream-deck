//! Rechargement à chaud du fichier de config.
//!
//! Windows signale les modifications (ReadDirectoryChangesW via `notify`) : aucun polling.
//! Les éditeurs écrivent souvent en plusieurs fois, d'où un anti-rebond de 300 ms.

use std::{path::PathBuf, time::Duration};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::{app::AppEvent, config};

/// Lance la surveillance. Le `RecommendedWatcher` renvoyé doit rester vivant.
pub fn spawn(path: PathBuf, events: UnboundedSender<AppEvent>) -> Option<RecommendedWatcher> {
    let (tx, mut rx) = unbounded_channel::<()>();
    let file_name = path.file_name()?.to_os_string();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let touches_config = event.paths.iter().any(|p| p.file_name() == Some(file_name.as_os_str()));
            if touches_config && (event.kind.is_modify() || event.kind.is_create()) {
                let _ = tx.send(());
            }
        }
    })
    .inspect_err(|e| log::warn!("Surveillance de la config impossible : {e}"))
    .ok()?;

    let dir = path.parent()?.to_path_buf();
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .inspect_err(|e| log::warn!("Surveillance de {} impossible : {e}", dir.display()))
        .ok()?;

    tokio::task::spawn_local(async move {
        while rx.recv().await.is_some() {
            // Anti-rebond : on attend que les écritures se calment.
            tokio::time::sleep(Duration::from_millis(300)).await;
            while rx.try_recv().is_ok() {}
            match config::load(&path) {
                Ok(cfg) => {
                    let _ = events.send(AppEvent::ConfigReloaded(Box::new(cfg)));
                }
                Err(e) => log::error!("Config non rechargée, l'ancienne reste active :\n{e}"),
            }
        }
    });

    Some(watcher)
}
