//! Couche matérielle : connexion au boîtier, lecture des appuis, envoi des images.
//!
//! - La lecture tourne dans sa propre tâche (une seule lecture USB en attente en permanence).
//! - L'écriture reçoit la « frame » voulue (15 images) via un canal `watch` et n'envoie que les
//!   touches qui diffèrent de ce qui est déjà affiché. Si une nouvelle frame arrive en cours
//!   d'envoi, on repart de la nouvelle cible sans finir l'ancienne.
//! - Débranchement → on attend le rebranchement (notification Windows), puis redessin complet.

use std::{rc::Rc, sync::Arc, time::Duration};

use async_hid::{DeviceEvent, HidBackend};
use futures_lite::StreamExt;
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    types::DeviceInput,
};
use tokio::{
    sync::{mpsc::UnboundedSender, oneshot, watch},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, sleep},
};

use crate::{
    app::AppEvent,
    layout::{DEVICE_KEY_COUNT, SLOTS, image_index, pos_from_raw},
    render::KeyImage,
};

/// Les 18 images à afficher : 15 touches (positions logiques) puis les 3 cases de l'écran latéral.
pub type Frame = [KeyImage; SLOTS];

/// SOOMFON 15 touches (famille Mirabox HSV293S) : interface HID vendeur 0xFFA0 / usage 1.
static QUERIES: [DeviceQuery; 1] = [DeviceQuery::new(0xFFA0, 1, 0x5548, 0x6670)];

/// Poignée gardée par la boucle principale.
pub struct DeviceHandle {
    pub frame: watch::Sender<Frame>,
    pub brightness: watch::Sender<u8>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl DeviceHandle {
    /// Éteint proprement les écrans avant de quitter.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(3), self.task).await;
    }
}

pub fn spawn(initial: Frame, brightness: u8, events: UnboundedSender<AppEvent>) -> DeviceHandle {
    let (frame, frame_rx) = watch::channel(initial);
    let (brightness_tx, brightness_rx) = watch::channel(brightness);
    let (shutdown, shutdown_rx) = watch::channel(false);
    // Superviseur : si la tâche plante (bug d'une bibliothèque…), on la relance.
    let task = tokio::task::spawn_local(async move {
        loop {
            let worker = tokio::task::spawn_local(run(
                frame_rx.clone(),
                brightness_rx.clone(),
                shutdown_rx.clone(),
                events.clone(),
            ));
            match worker.await {
                Err(e) if e.is_panic() && !*shutdown_rx.borrow() => {
                    log::error!("La tâche boîtier a planté, redémarrage dans 2 s");
                    sleep(Duration::from_secs(2)).await;
                }
                _ => return,
            }
        }
    });
    DeviceHandle { frame, brightness: brightness_tx, shutdown, task }
}

#[derive(PartialEq)]
enum SessionEnd {
    Lost,
    Shutdown,
}

async fn run(
    mut frame_rx: watch::Receiver<Frame>,
    mut brightness_rx: watch::Receiver<u8>,
    mut shutdown_rx: watch::Receiver<bool>,
    events: UnboundedSender<AppEvent>,
) {
    // Notifications de branchement / débranchement (sans interroger l'USB en boucle).
    // On utilise directement async-hid : le `DeviceWatcher` de mirajazz plante (unwrap) si le
    // périphérique disparaît pendant qu'il l'interroge. Ici, un événement ne fait que nous
    // réveiller ; c'est `connect()` qui vérifie, avec gestion d'erreur.
    let backend = HidBackend::default();
    let mut plug_events = match backend.watch() {
        Ok(stream) => Some(stream),
        Err(e) => {
            log::warn!("Surveillance USB indisponible ({e:?}) : nouvelle tentative toutes les 5 s");
            None
        }
    };
    let mut warned_missing = false;
    // Nombre d'échecs consécutifs alors que le boîtier semblait présent : on réessaie vite.
    let mut quick_retries = 0u32;
    // Vrai quand on vient d'être réveillé par un branchement : le firmware démarre encore.
    let mut just_plugged = false;

    loop {
        if *shutdown_rx.borrow() {
            return;
        }

        match connect().await {
            Ok(Some(device)) => {
                warned_missing = false;
                if just_plugged {
                    // Écrire trop tôt après le branchement bloque ou échoue (0x8007001F).
                    sleep(Duration::from_millis(1500)).await;
                }
                let device = Rc::new(device);
                let _ = events.send(AppEvent::DeviceConnected);
                let started = Instant::now();
                let end = session(&device, &mut frame_rx, &mut brightness_rx, &mut shutdown_rx, &events).await;
                let _ = events.send(AppEvent::DeviceDisconnected);
                if end == SessionEnd::Shutdown {
                    if let Err(e) = device.shutdown().await {
                        log::debug!("Extinction du boîtier : {e:?}");
                    }
                    return;
                }
                log::warn!("Boîtier perdu, en attente de rebranchement…");
                if started.elapsed() > Duration::from_secs(10) {
                    quick_retries = 0; // la session précédente était saine
                }
                quick_retries += 1;
            }
            Ok(None) => {
                quick_retries = 0;
                if !warned_missing {
                    log::warn!("Boîtier SOOMFON non trouvé, en attente de branchement…");
                    warned_missing = true;
                }
            }
            Err(e) => {
                log::warn!("Connexion au boîtier impossible : {e:?}");
                quick_retries += 1;
            }
        }

        // Attente : branchement signalé par Windows, nouvel essai rapide si le boîtier semblait
        // présent mais ne répondait pas, filet de sécurité périodique, ou arrêt.
        just_plugged = quick_retries > 0;
        let fallback = if (1..=10).contains(&quick_retries) {
            Duration::from_secs(1)
        } else if plug_events.is_some() {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(5)
        };
        tokio::select! {
            _ = shutdown_rx.changed() => {}
            _ = sleep(fallback) => {}
            _ = async {
                match plug_events.as_mut() {
                    Some(stream) => loop {
                        match stream.next().await {
                            Some(DeviceEvent::Connected(_)) => break,
                            Some(_) => continue,
                            None => std::future::pending::<()>().await,
                        }
                    },
                    None => std::future::pending::<()>().await,
                }
            } => {
                just_plugged = true;
                quick_retries = quick_retries.max(1);
            }
        }
    }
}

async fn connect() -> Result<Option<Device>, MirajazzError> {
    let Some(info) = list_devices(&QUERIES).await?.into_iter().next() else {
        return Ok(None);
    };
    let device = Device::connect(&info, 1, DEVICE_KEY_COUNT, 0).await?;
    log::info!("Boîtier connecté ({:04X}:{:04X})", device.vid, device.pid);
    Ok(Some(device))
}

async fn session(
    device: &Rc<Device>,
    frame_rx: &mut watch::Receiver<Frame>,
    brightness_rx: &mut watch::Receiver<u8>,
    shutdown_rx: &mut watch::Receiver<bool>,
    events: &UnboundedSender<AppEvent>,
) -> SessionEnd {
    // Tâche de lecture des appuis.
    let (dead_tx, mut dead_rx) = oneshot::channel::<()>();
    let reader_device = device.clone();
    let reader_events = events.clone();
    let reader = tokio::task::spawn_local(async move {
        let reader = reader_device.get_reader(|_, _| Ok(DeviceInput::NoData));
        loop {
            match reader.raw_read_data(512).await {
                Ok(data) => {
                    // Rapport d'appui : "ACK" … "OK" … index brut à l'octet 9.
                    if data.starts_with(b"ACK") {
                        if let Some(pos) = pos_from_raw(data[9]) {
                            let _ = reader_events.send(AppEvent::Key(pos));
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Lecture USB interrompue : {e:?}");
                    break;
                }
            }
        }
        let _ = dead_tx.send(());
    });

    let end = async {
        let brightness = *brightness_rx.borrow_and_update();
        let init = async {
            device.set_brightness(brightness).await?;
            device.clear_all_button_images().await
        };
        if !guarded(SHORT, init).await {
            return SessionEnd::Lost;
        }

        // Ce qui est réellement affiché sur chaque touche (rien après l'effacement).
        let mut shown: [Option<KeyImage>; SLOTS] = Default::default();
        let mut keep_alive = interval(Duration::from_secs(10));
        keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
        keep_alive.tick().await;

        loop {
            if !guarded(PAINT, paint(device, frame_rx, &mut shown)).await {
                return SessionEnd::Lost;
            }
            tokio::select! {
                _ = &mut dead_rx => return SessionEnd::Lost,
                changed = frame_rx.changed() => {
                    if changed.is_err() {
                        return SessionEnd::Shutdown;
                    }
                }
                changed = brightness_rx.changed() => {
                    if changed.is_ok() {
                        let value = *brightness_rx.borrow_and_update();
                        if !guarded(SHORT, device.set_brightness(value)).await {
                            return SessionEnd::Lost;
                        }
                    }
                }
                _ = keep_alive.tick() => {
                    if !guarded(SHORT, device.keep_alive()).await {
                        return SessionEnd::Lost;
                    }
                }
                _ = shutdown_rx.changed() => return SessionEnd::Shutdown,
            }
        }
    }
    .await;

    reader.abort();
    end
}

/// Délai maximal d'une commande simple (luminosité, keep-alive, effacement).
const SHORT: Duration = Duration::from_secs(3);
/// Délai maximal pour dessiner une page complète (~0,6 s en temps normal).
const PAINT: Duration = Duration::from_secs(8);

/// Exécute une opération USB avec un délai maximal. `false` = échec ou blocage : le boîtier
/// est considéré comme perdu et on se reconnecte, plutôt que de rester figé.
async fn guarded(limit: Duration, op: impl Future<Output = Result<(), MirajazzError>>) -> bool {
    match tokio::time::timeout(limit, op).await {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            log::debug!("Écriture USB échouée : {e:?}");
            false
        }
        Err(_) => {
            log::warn!("Le boîtier ne répond plus (écriture bloquée {limit:?}), reconnexion");
            false
        }
    }
}

/// Nombre de touches envoyées avant chaque commande d'affichage (`STP`).
/// Le `STP` coûte cher (~25 ms) : on le partage entre plusieurs touches, tout en gardant
/// la possibilité de s'interrompre entre deux paquets si la cible change.
const BATCH: usize = 5;

/// Envoie les touches qui diffèrent de ce qui est affiché. S'interrompt pour repartir de la
/// nouvelle cible si la frame change pendant l'envoi.
async fn paint(
    device: &Device,
    frame_rx: &mut watch::Receiver<Frame>,
    shown: &mut [Option<KeyImage>; SLOTS],
) -> Result<(), MirajazzError> {
    'restart: loop {
        let target = frame_rx.borrow_and_update().clone();
        let start = Instant::now();
        let changed: Vec<usize> = (0..SLOTS)
            .filter(|&pos| {
                !shown[pos].as_ref().is_some_and(|s| Arc::ptr_eq(s, &target[pos]) || **s == *target[pos])
            })
            .collect();
        if changed.is_empty() {
            return Ok(());
        }

        let mut sent = 0;
        for batch in changed.chunks(BATCH) {
            for &pos in batch {
                device.write_image(image_index(pos), &target[pos]).await?;
            }
            device.flush().await?;
            for &pos in batch {
                shown[pos] = Some(target[pos].clone());
            }
            sent += batch.len();
            if frame_rx.has_changed().unwrap_or(false) {
                continue 'restart;
            }
        }
        log::debug!("{sent} touche(s) envoyée(s) en {:?}", start.elapsed());
        return Ok(());
    }
}
