//! Lien avec OBS Studio via obs-websocket (v5).
//!
//! - Réglages (port, mot de passe) lus dans la config d'OBS, sauf si `[obs]` les impose.
//! - Reconnexion automatique avec un délai qui grandit de 1 à 30 s : OBS fermé ne coûte presque rien.
//! - Une fois connecté, l'état est lu une fois, puis tenu à jour uniquement par les événements.

use std::{
    collections::{HashMap, HashSet},
    panic::AssertUnwindSafe,
    path::PathBuf,
    time::Duration,
};

use futures_lite::{FutureExt, StreamExt};
use obws::{
    Client,
    client::ConnectConfig,
    common::MediaAction,
    events::{Event, OutputState},
    requests::{inputs::InputId, scene_items, scenes::SceneId},
};
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    time::{Instant, sleep},
};

use crate::{app::AppEvent, config::ObsConfig};

/// Ce que l'application demande à OBS.
#[derive(Debug, Clone)]
pub enum ObsCommand {
    SetScene(String),
    ToggleRecord,
    ToggleRecordPause,
    ToggleStream,
    RestartMedia(String),
    ToggleMute(String),
    ToggleSource { scene: String, source: String },
    SaveReplay,
    /// Entrées audio et sources dont il faut suivre l'état (issues de la config).
    Watch(WatchList),
    /// Nouveaux paramètres de connexion (config rechargée).
    Settings(ObsConfig),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchList {
    pub inputs: HashSet<String>,
    pub sources: HashSet<(String, String)>,
}

/// Ce qu'OBS signale à l'application.
#[derive(Debug, Clone)]
pub enum ObsUpdate {
    Connected(Snapshot),
    Disconnected,
    ProgramScene(String),
    SceneList(Vec<String>),
    Record { active: bool, paused: bool },
    Stream { active: bool, started: Option<Instant> },
    Mute { input: String, muted: bool },
    Source { scene: String, source: String, enabled: bool },
    ReplayBuffer(bool),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub program_scene: Option<String>,
    pub scenes: Vec<String>,
    pub record_active: bool,
    pub record_paused: bool,
    pub stream_active: bool,
    pub stream_started: Option<Instant>,
    pub replay_active: bool,
    pub mutes: HashMap<String, bool>,
    pub sources: HashMap<(String, String), bool>,
}

pub fn spawn(settings: ObsConfig, events: UnboundedSender<AppEvent>) -> UnboundedSender<ObsCommand> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(run(settings, rx, events));
    tx
}

/// Réglages effectifs de connexion.
struct Target {
    host: String,
    port: u16,
    password: Option<String>,
}

fn resolve_target(cfg: &ObsConfig) -> Target {
    // Lecture de %APPDATA%\obs-studio\plugin_config\obs-websocket\config.json
    #[derive(serde::Deserialize, Default)]
    struct WsConfig {
        server_port: Option<u16>,
        server_password: Option<String>,
        auth_required: Option<bool>,
    }
    let path = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(r"obs-studio\plugin_config\obs-websocket\config.json");
    let ws: WsConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let ws_password = if ws.auth_required.unwrap_or(true) { ws.server_password.filter(|p| !p.is_empty()) } else { None };

    Target {
        host: cfg.host.clone().unwrap_or_else(|| "127.0.0.1".into()),
        port: cfg.port.or(ws.server_port).unwrap_or(4455),
        password: cfg.password.clone().or(ws_password),
    }
}

/// Connexion à OBS avec les réglages effectifs (config, sinon ceux lus dans OBS).
/// `events = false` : pas d'abonnement aux événements (usage ponctuel, ex. l'éditeur).
pub async fn connect(settings: &ObsConfig, events: bool) -> Result<Client, obws::error::Error> {
    let target = resolve_target(settings);
    Client::connect_with_config(ConnectConfig {
        host: target.host.as_str(),
        port: target.port,
        password: target.password.as_deref(),
        event_subscriptions: if events { None } else { Some(obws::requests::EventSubscription::NONE) },
        broadcast_capacity: obws::client::DEFAULT_BROADCAST_CAPACITY,
        connect_timeout: Duration::from_secs(3),
        dangerous: None,
    })
    .await
}

async fn run(mut settings: ObsConfig, mut commands: UnboundedReceiver<ObsCommand>, events: UnboundedSender<AppEvent>) {
    let mut watch = WatchList::default();
    let mut delay = Duration::from_secs(1);
    let mut logged_offline = false;

    loop {
        match connect(&settings, true).await {
            Ok(client) => {
                delay = Duration::from_secs(1);
                logged_offline = false;
                log::info!("OBS connecté");
                // Un plantage pendant la session (bibliothèque tierce…) = simple déconnexion.
                let result = AssertUnwindSafe(session(&client, &mut commands, &mut watch, &mut settings, &events))
                    .catch_unwind()
                    .await;
                if result.is_err() {
                    log::error!("La session OBS a planté, reconnexion");
                }
                let _ = events.send(AppEvent::Obs(ObsUpdate::Disconnected));
                log::info!("OBS déconnecté");
            }
            Err(e) => {
                if !logged_offline {
                    log::info!("OBS injoignable ({e}) : nouvelles tentatives en arrière-plan");
                    logged_offline = true;
                }
                // Pendant l'attente, on garde les commandes de configuration, on ignore les autres.
                let wake = sleep(delay);
                tokio::pin!(wake);
                loop {
                    tokio::select! {
                        _ = &mut wake => break,
                        cmd = commands.recv() => match cmd {
                            None => return,
                            Some(ObsCommand::Watch(w)) => watch = w,
                            Some(ObsCommand::Settings(s)) => { settings = s; break; }
                            Some(other) => log::warn!("OBS non connecté, action ignorée : {other:?}"),
                        }
                    }
                }
                delay = (delay * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn session(
    client: &Client,
    commands: &mut UnboundedReceiver<ObsCommand>,
    watch: &mut WatchList,
    settings: &mut ObsConfig,
    events: &UnboundedSender<AppEvent>,
) {
    let mut stream = match client.events() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Flux d'événements OBS indisponible : {e}");
            return;
        }
    };

    // (scène, id d'élément) → nom de source, pour traduire SceneItemEnableStateChanged.
    let mut item_names: HashMap<(String, i64), String> = HashMap::new();
    match snapshot(client, watch, &mut item_names).await {
        Ok(snap) => {
            let _ = events.send(AppEvent::Obs(ObsUpdate::Connected(snap)));
        }
        Err(e) => {
            log::warn!("Lecture de l'état OBS impossible : {e}");
            return;
        }
    }

    loop {
        tokio::select! {
            event = stream.next() => {
                let Some(event) = event else { return };
                log::debug!("Événement OBS : {event:?}");
                if let Some(update) = translate(event, watch, &item_names) {
                    let _ = events.send(AppEvent::Obs(update));
                }
            }
            cmd = commands.recv() => {
                let Some(cmd) = cmd else { return };
                match cmd {
                    ObsCommand::Watch(w) => {
                        if w != *watch {
                            *watch = w;
                            // Relit l'état des éléments suivis.
                            match snapshot(client, watch, &mut item_names).await {
                                Ok(snap) => { let _ = events.send(AppEvent::Obs(ObsUpdate::Connected(snap))); }
                                Err(e) => log::warn!("Relecture de l'état OBS : {e}"),
                            }
                        }
                    }
                    ObsCommand::Settings(s) => {
                        *settings = s;
                        return; // reconnexion avec les nouveaux réglages
                    }
                    other => {
                        if let Err(e) = execute(client, &other).await {
                            log::warn!("Action OBS {other:?} échouée : {e}");
                        }
                    }
                }
            }
        }
    }
}

async fn execute(client: &Client, cmd: &ObsCommand) -> Result<(), obws::error::Error> {
    match cmd {
        ObsCommand::SetScene(scene) => client.scenes().set_current_program_scene(SceneId::Name(scene)).await,
        ObsCommand::ToggleRecord => client.recording().toggle().await.map(drop),
        ObsCommand::ToggleRecordPause => client.recording().toggle_pause().await.map(drop),
        ObsCommand::ToggleStream => client.streaming().toggle().await.map(drop),
        ObsCommand::RestartMedia(input) => {
            client.media_inputs().trigger_action(InputId::Name(input), MediaAction::Restart).await
        }
        ObsCommand::ToggleMute(input) => client.inputs().toggle_mute(InputId::Name(input)).await.map(drop),
        ObsCommand::ToggleSource { scene, source } => {
            let item_id = client
                .scene_items()
                .id(scene_items::Id { scene: SceneId::Name(scene), source, search_offset: None })
                .await?;
            let enabled = client.scene_items().enabled(SceneId::Name(scene), item_id).await?;
            client
                .scene_items()
                .set_enabled(scene_items::SetEnabled { scene: SceneId::Name(scene), item_id, enabled: !enabled })
                .await
        }
        ObsCommand::SaveReplay => client.replay_buffer().save().await,
        ObsCommand::Watch(_) | ObsCommand::Settings(_) => Ok(()),
    }
}

/// Lit l'état complet d'OBS (au branchement, ou quand la liste suivie change).
async fn snapshot(
    client: &Client,
    watch: &WatchList,
    item_names: &mut HashMap<(String, i64), String>,
) -> Result<Snapshot, obws::error::Error> {
    let scenes = client.scenes().list().await?;
    let record = client.recording().status().await?;
    let stream = client.streaming().status().await?;
    // Le replay buffer peut être désactivé dans OBS : ce n'est pas une erreur.
    let replay_active = client.replay_buffer().status().await.unwrap_or(false);

    let mut snap = Snapshot {
        program_scene: scenes.current_program_scene.map(|s| s.name),
        // OBS liste les scènes de bas en haut : on inverse pour suivre l'ordre affiché dans OBS.
        scenes: scenes.scenes.into_iter().rev().map(|s| s.id.name).collect(),
        record_active: record.active,
        record_paused: record.paused,
        stream_active: stream.active,
        stream_started: stream.active.then(|| Instant::now() - stream.duration.unsigned_abs()),
        replay_active,
        ..Default::default()
    };

    for input in &watch.inputs {
        match client.inputs().muted(InputId::Name(input)).await {
            Ok(muted) => {
                snap.mutes.insert(input.clone(), muted);
            }
            Err(e) => log::warn!("Entrée audio « {input} » introuvable dans OBS : {e}"),
        }
    }

    item_names.clear();
    for (scene, source) in &watch.sources {
        let id = client
            .scene_items()
            .id(scene_items::Id { scene: SceneId::Name(scene), source, search_offset: None })
            .await;
        match id {
            Ok(item_id) => {
                let enabled = client.scene_items().enabled(SceneId::Name(scene), item_id).await?;
                item_names.insert((scene.clone(), item_id), source.clone());
                snap.sources.insert((scene.clone(), source.clone()), enabled);
            }
            Err(e) => log::warn!("Source « {source} » introuvable dans la scène « {scene} » : {e}"),
        }
    }

    Ok(snap)
}

fn translate(event: Event, watch: &WatchList, item_names: &HashMap<(String, i64), String>) -> Option<ObsUpdate> {
    Some(match event {
        Event::CurrentProgramSceneChanged { id } => ObsUpdate::ProgramScene(id.name),
        Event::SceneListChanged { mut scenes } => {
            scenes.sort_by_key(|s| std::cmp::Reverse(s.index));
            ObsUpdate::SceneList(scenes.into_iter().map(|s| s.name).collect())
        }
        // Attention : OBS envoie `active: false` quand l'enregistrement passe en pause.
        // On se fie donc à l'état précis plutôt qu'au drapeau `active`.
        Event::RecordStateChanged { state, .. } => match state {
            OutputState::Started | OutputState::Resumed => ObsUpdate::Record { active: true, paused: false },
            OutputState::Paused => ObsUpdate::Record { active: true, paused: true },
            OutputState::Stopping | OutputState::Stopped => ObsUpdate::Record { active: false, paused: false },
            _ => return None,
        },
        Event::StreamStateChanged { active, state } => match state {
            OutputState::Started => ObsUpdate::Stream { active: true, started: Some(Instant::now()) },
            OutputState::Stopped => ObsUpdate::Stream { active: false, started: None },
            _ => {
                let _ = active;
                return None;
            }
        },
        Event::ReplayBufferStateChanged { active, .. } => ObsUpdate::ReplayBuffer(active),
        Event::InputMuteStateChanged { id, muted } => {
            if !watch.inputs.contains(&id.name) {
                return None;
            }
            ObsUpdate::Mute { input: id.name, muted }
        }
        Event::SceneItemEnableStateChanged { scene, item_id, enabled } => {
            let source = item_names.get(&(scene.name.clone(), item_id as i64))?.clone();
            ObsUpdate::Source { scene: scene.name, source, enabled }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(state: OutputState, active: bool) -> Option<(bool, bool)> {
        let ev = Event::RecordStateChanged { active, state, path: None };
        match translate(ev, &WatchList::default(), &HashMap::new()) {
            Some(ObsUpdate::Record { active, paused }) => Some((active, paused)),
            _ => None,
        }
    }

    #[test]
    fn pause_reste_un_enregistrement_actif() {
        // Séquence réelle observée avec OBS 32 : Paused arrive avec active = false.
        assert_eq!(record(OutputState::Started, true), Some((true, false)));
        assert_eq!(record(OutputState::Paused, false), Some((true, true)));
        assert_eq!(record(OutputState::Resumed, true), Some((true, false)));
        assert_eq!(record(OutputState::Stopping, false), Some((false, false)));
        assert_eq!(record(OutputState::Stopped, false), Some((false, false)));
        assert_eq!(record(OutputState::Starting, false), None);
    }
}
