//! Cœur de l'application : état, navigation (racine → dossier → Retour) et apparence des touches.
//!
//! Tout passe par une seule boucle d'événements : appuis, nouvelles d'OBS, config rechargée,
//! battement du chrono. Après chaque événement, on recalcule les 15 touches de la page
//! courante et les 3 cases de l'écran latéral ; seules celles qui changent partent en USB
//! (voir device.rs).

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::{sync::mpsc::UnboundedSender, time::Instant};

use crate::{
    clock,
    coil::{self, Coil, Press},
    config::{Action, Config, KeyConfig, SideWidget},
    device::{DeviceHandle, Frame},
    layout::{BACK_POS, KEYS, SLOTS},
    media::{self, NowPlaying},
    obs::{ObsCommand, ObsUpdate, Snapshot, WatchList},
    plugins::{
        countdown::{self, Countdowns},
        hotkey, launch, sound,
    },
    streamerbot::SbCommand,
    render::{KeyImage, KeyVisual, Renderer},
    visuals::{ACTIVE_RED, PAUSED_AMBER, SideState, back_visual, base_visual, coil_visual, countdown_visual, side_visual},
};

/// Tout ce qui peut réveiller la boucle principale.
#[derive(Debug)]
pub enum AppEvent {
    Key(usize),
    DeviceConnected,
    DeviceDisconnected,
    Obs(ObsUpdate),
    ConfigReloaded(Box<Config>),
    /// Streamer.bot joignable ou non.
    Streamerbot(bool),
    /// Musique en cours (écran latéral).
    Music(Option<NowPlaying>),
}

/// Page affichée.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PageRef {
    Root,
    /// Dossier défini dans la config (index de page).
    Folder(usize),
    /// Dossier généré à partir des scènes OBS.
    Scenes,
}

/// État d'OBS tel que connu de l'app.
#[derive(Debug, Default)]
struct ObsState {
    connected: bool,
    program_scene: Option<String>,
    scenes: Vec<String>,
    record_active: bool,
    record_paused: bool,
    stream_active: bool,
    stream_started: Option<Instant>,
    replay_active: bool,
    mutes: HashMap<String, bool>,
    sources: HashMap<(String, String), bool>,
}

pub struct App {
    cfg: Config,
    renderer: Renderer,
    icons_dir: std::path::PathBuf,
    page: PageRef,
    obs: ObsState,
    device: Option<DeviceHandle>,
    obs_tx: UnboundedSender<ObsCommand>,
    sb_tx: UnboundedSender<SbCommand>,
    /// Streamer.bot joignable (les touches Streamer.bot sont grisées sinon).
    sb_ok: bool,
    music: Option<NowPlaying>,
    /// Vrai si une case de l'écran latéral affiche la musique (le suivi ne tourne qu'alors).
    music_wanted: Arc<AtomicBool>,
    coil: Coil,
    countdowns: Countdowns,
}

impl App {
    pub fn new(
        cfg: Config,
        icons_dir: std::path::PathBuf,
        obs_tx: UnboundedSender<ObsCommand>,
        sb_tx: UnboundedSender<SbCommand>,
    ) -> Self {
        let renderer = Renderer::new(icons_dir.clone(), cfg.jpeg_quality);
        let music_wanted = Arc::new(AtomicBool::new(cfg.side.contains(&SideWidget::Music)));
        let app = Self {
            cfg,
            renderer,
            icons_dir,
            page: PageRef::Root,
            obs: ObsState::default(),
            device: None,
            obs_tx,
            sb_tx,
            sb_ok: false,
            music: None,
            music_wanted,
            coil: Coil::load(crate::config::app_dir().join("coil.json")),
            countdowns: Countdowns::default(),
        };
        app.send_watch_list();
        app
    }

    /// Calcule la première frame (avant que la tâche boîtier existe).
    pub fn initial_frame(&mut self) -> Frame {
        self.frame()
    }

    pub fn attach_device(&mut self, device: DeviceHandle) {
        self.device = Some(device);
    }

    pub fn take_device(&mut self) -> Option<DeviceHandle> {
        self.device.take()
    }

    pub fn brightness(&self) -> u8 {
        self.cfg.brightness
    }

    /// Drapeau lu par le suivi de la musique (media::watch_now_playing).
    pub fn music_flag(&self) -> Arc<AtomicBool> {
        self.music_wanted.clone()
    }

    /// Faut-il un battement de 1 s ? (chrono du live visible, ou horloge sur l'écran latéral)
    pub fn needs_tick(&self) -> bool {
        let side = |w: SideWidget| self.cfg.side.contains(&w);
        side(SideWidget::Clock)
            || self.coil.active(Instant::now())
            || self.countdowns.active()
            || (self.obs.stream_active
                && (side(SideWidget::StreamTimer)
                    || self.current_keys().iter().any(|k| matches!(k.action, Action::StreamTimer))))
    }

    pub fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(pos) => self.on_key(pos),
            AppEvent::DeviceConnected => log::debug!("Boîtier prêt"),
            AppEvent::DeviceDisconnected => {}
            AppEvent::Obs(update) => self.on_obs(update),
            AppEvent::ConfigReloaded(cfg) => self.on_config(*cfg),
            AppEvent::Streamerbot(ok) => self.sb_ok = ok,
            AppEvent::Music(now) => self.music = now,
        }
        self.redraw();
    }

    /// Battement du chrono (1 Hz).
    pub fn tick(&mut self) {
        if self.coil.tick(Instant::now()) {
            log::info!("Coil : pause terminée");
            coil::chime();
        }
        if self.countdowns.tick(Instant::now()) > 0 {
            log::info!("Compte à rebours terminé");
            coil::chime();
        }
        self.redraw();
    }

    // ── Appuis ──────────────────────────────────────────────────────────────

    fn on_key(&mut self, pos: usize) {
        if self.page != PageRef::Root && pos == BACK_POS {
            log::debug!("Retour à la racine");
            self.page = PageRef::Root;
            return;
        }

        let Some(key) = self.current_keys().into_iter().find(|k| k.pos == pos) else { return };
        log::debug!("Touche {pos} : {:?}", key.action);

        match key.action {
            Action::Folder { page } => {
                if let Some(index) = self.cfg.page_index(&page) {
                    self.page = PageRef::Folder(index);
                }
            }
            Action::ObsScenesFolder => self.page = PageRef::Scenes,
            Action::StreamTimer | Action::None => {}
            Action::ObsScene { scene } => self.send(ObsCommand::SetScene(scene)),
            Action::ObsRecord => self.send(ObsCommand::ToggleRecord),
            Action::ObsRecordPause => self.send(ObsCommand::ToggleRecordPause),
            Action::ObsStream => self.send(ObsCommand::ToggleStream),
            Action::ObsMedia { input } => self.send(ObsCommand::RestartMedia(input)),
            Action::ObsMute { input } => self.send(ObsCommand::ToggleMute(input)),
            Action::ObsSource { scene, source } => self.send(ObsCommand::ToggleSource { scene, source }),
            Action::ObsReplaySave => self.send(ObsCommand::SaveReplay),
            Action::Media { key } => media::press(key),
            Action::Coil { seconds } => match self.coil.press(seconds, Instant::now()) {
                Press::Started => log::info!("Coil : taffe, pause de {seconds} s"),
                Press::Wait => log::debug!("Coil : pause en cours, attends"),
            },
            Action::Streamerbot { action } => {
                if action.trim().is_empty() {
                    log::warn!("Touche Streamer.bot sans action choisie");
                } else {
                    let _ = self.sb_tx.send(SbCommand::Run(action));
                }
            }
            Action::Hotkey { keys } => hotkey::send(&keys),
            Action::Launch { target, args } => launch::open(&target, &args),
            Action::Sound { file, volume } => sound::play(&file, volume),
            Action::Countdown { seconds } => {
                if let Some(page) = self.page_index() {
                    self.countdowns.press((page, pos), seconds, Instant::now());
                }
            }
        }
    }

    /// Index (dans la config) de la page affichée ; `None` pour le dossier automatique des scènes.
    fn page_index(&self) -> Option<usize> {
        match self.page {
            PageRef::Root => Some(0),
            PageRef::Folder(i) => Some(i),
            PageRef::Scenes => None,
        }
    }

    fn send(&self, cmd: ObsCommand) {
        if !self.obs.connected {
            log::warn!("OBS n'est pas connecté : action ignorée");
            return;
        }
        let _ = self.obs_tx.send(cmd);
    }

    // ── OBS ─────────────────────────────────────────────────────────────────

    fn on_obs(&mut self, update: ObsUpdate) {
        let o = &mut self.obs;
        match update {
            ObsUpdate::Connected(snap) => apply_snapshot(o, snap),
            ObsUpdate::Disconnected => *o = ObsState::default(),
            ObsUpdate::ProgramScene(scene) => o.program_scene = Some(scene),
            ObsUpdate::SceneList(scenes) => o.scenes = scenes,
            ObsUpdate::Record { active, paused } => {
                o.record_active = active;
                o.record_paused = active && paused;
            }
            ObsUpdate::Stream { active, started } => {
                o.stream_active = active;
                o.stream_started = started;
            }
            ObsUpdate::Mute { input, muted } => {
                o.mutes.insert(input, muted);
            }
            ObsUpdate::Source { scene, source, enabled } => {
                o.sources.insert((scene, source), enabled);
            }
            ObsUpdate::ReplayBuffer(active) => o.replay_active = active,
        }
    }

    // ── Config ──────────────────────────────────────────────────────────────

    fn on_config(&mut self, cfg: Config) {
        log::info!("Config rechargée");
        let obs_changed = cfg.obs != self.cfg.obs;
        if cfg.streamerbot != self.cfg.streamerbot {
            let _ = self.sb_tx.send(SbCommand::Settings(cfg.streamerbot.clone()));
        }
        let icons_dir = cfg.icons_dir.clone().unwrap_or_else(|| self.icons_dir.clone());
        self.renderer.reset(icons_dir, cfg.jpeg_quality);
        if let Some(device) = &self.device {
            let _ = device.brightness.send(cfg.brightness);
        }
        // La page ouverte a pu disparaître : on revient à la racine dans ce cas.
        if let PageRef::Folder(i) = self.page {
            if i >= cfg.pages.len() {
                self.page = PageRef::Root;
            }
        }
        if obs_changed {
            let _ = self.obs_tx.send(ObsCommand::Settings(cfg.obs.clone()));
        }
        let wants_music = cfg.side.contains(&SideWidget::Music);
        self.music_wanted.store(wants_music, Ordering::Relaxed);
        if !wants_music {
            self.music = None;
        }
        // Décomptes des touches supprimées ou changées : oubliés.
        self.countdowns.retain(|(page, pos)| {
            cfg.pages.get(page).is_some_and(|p| {
                p.keys.iter().any(|k| k.pos == pos && matches!(k.action, Action::Countdown { .. }))
            })
        });
        self.cfg = cfg;
        self.send_watch_list();
    }

    /// Indique à la tâche OBS quelles entrées / sources suivre.
    fn send_watch_list(&self) {
        let mut list = WatchList { inputs: HashSet::new(), sources: HashSet::new() };
        for page in &self.cfg.pages {
            for key in &page.keys {
                match &key.action {
                    Action::ObsMute { input } => {
                        list.inputs.insert(input.clone());
                    }
                    Action::ObsSource { scene, source } => {
                        list.sources.insert((scene.clone(), source.clone()));
                    }
                    _ => {}
                }
            }
        }
        let _ = self.obs_tx.send(ObsCommand::Watch(list));
    }

    // ── Affichage ───────────────────────────────────────────────────────────

    /// Touches de la page courante (le Retour est ajouté à part).
    fn current_keys(&self) -> Vec<KeyConfig> {
        match self.page {
            PageRef::Root => self.cfg.pages[0].keys.clone(),
            PageRef::Folder(i) => self.cfg.pages.get(i).map(|p| p.keys.clone()).unwrap_or_default(),
            PageRef::Scenes => self
                .obs
                .scenes
                .iter()
                .take(KEYS - 1)
                .enumerate()
                .map(|(pos, scene)| KeyConfig {
                    pos,
                    icon: Some("obs/scene.svg".into()),
                    label: Some(scene.clone()),
                    color: None,
                    action: Action::ObsScene { scene: scene.clone() },
                })
                .collect(),
        }
    }

    fn redraw(&mut self) {
        let frame = self.frame();
        if let Some(device) = &self.device {
            device.frame.send_if_modified(|current| {
                let changed = current.iter().zip(frame.iter()).any(|(a, b)| !std::sync::Arc::ptr_eq(a, b));
                if changed {
                    *current = frame;
                }
                changed
            });
        }
    }

    fn frame(&mut self) -> Frame {
        let mut visuals: [KeyVisual; KEYS] = std::array::from_fn(|_| KeyVisual::blank());
        for key in self.current_keys() {
            if key.pos < KEYS {
                visuals[key.pos] = self.visual(&key);
            }
        }
        if self.page != PageRef::Root {
            visuals[BACK_POS] = back_visual();
        }
        let side: Vec<KeyVisual> =
            (0..SLOTS - KEYS).map(|i| self.side_visual(self.cfg.side.get(i).copied().unwrap_or(SideWidget::None))).collect();
        let images: [KeyImage; SLOTS] = std::array::from_fn(|i| {
            if i < KEYS { self.renderer.render(&visuals[i]) } else { self.renderer.render(&side[i - KEYS]) }
        });
        images
    }

    /// Case de l'écran latéral, avec l'état réel.
    fn side_visual(&self, widget: SideWidget) -> KeyVisual {
        let o = &self.obs;
        let state = SideState {
            music: self.music.as_ref(),
            stream_elapsed: o.stream_active.then(|| o.stream_started.map(|t| t.elapsed().as_secs()).unwrap_or(0)),
            scene: o.program_scene.as_deref(),
            obs_connected: o.connected,
            time: clock::now(),
            coil: self.coil.view(Instant::now()),
        };
        side_visual(widget, &state)
    }

    /// Apparence d'une touche selon son action et l'état d'OBS.
    fn visual(&self, key: &KeyConfig) -> KeyVisual {
        let o = &self.obs;
        let mut v = base_visual(key);
        v.dim = key.action.needs_obs() && !o.connected;

        match &key.action {
            Action::ObsScene { scene } => {
                v.highlight = o.program_scene.as_deref() == Some(scene.as_str());
            }
            Action::ObsRecord if o.record_active => {
                v.bg = ACTIVE_RED;
                v.icon = Some("obs/record-stop.svg".into());
            }
            Action::ObsRecordPause if o.record_paused => {
                v.bg = PAUSED_AMBER;
                v.icon = Some("obs/record.svg".into());
                v.label = Some("Reprendre".into());
            }
            Action::ObsStream if o.stream_active => {
                v.bg = ACTIVE_RED;
                v.icon = Some("obs/live-stop.svg".into());
            }
            Action::StreamTimer => {
                if o.stream_active {
                    v.bg = ACTIVE_RED;
                    let elapsed = o.stream_started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                    v.big_text = Some(format_duration(elapsed));
                }
            }
            Action::ObsMute { input } if o.mutes.get(input).copied().unwrap_or(false) => {
                v.bg = ACTIVE_RED;
                v.icon = Some(muted_icon(key.icon.as_deref()));
            }
            // Replay buffer arrêté dans OBS : la sauvegarde est impossible, on grise.
            Action::ObsReplaySave => v.dim |= !o.replay_active,
            // Streamer.bot fermé : l'action ne pourrait pas partir.
            Action::Streamerbot { .. } => v.dim = !self.sb_ok,
            Action::Coil { seconds } => {
                let label = key.label.clone().unwrap_or_else(|| format!("{seconds} s"));
                let mut c = coil_visual(&self.coil.view(Instant::now()), Some((&label, key.icon.as_deref())));
                if c.dial.is_none() {
                    // Au repos : la couleur choisie dans l'éditeur reste celle de la touche.
                    c.bg = v.bg;
                }
                return c;
            }
            Action::Countdown { seconds } => {
                let view = match self.page_index() {
                    Some(page) => self.countdowns.view((page, key.pos), Instant::now()),
                    None => countdown::View::Idle,
                };
                return countdown_visual(v, view, *seconds);
            }
            Action::ObsSource { scene, source } => {
                let enabled = o.sources.get(&(scene.clone(), source.clone())).copied().unwrap_or(true);
                v.highlight = enabled;
                if !enabled {
                    v.icon = Some("obs/source-hide.svg".into());
                }
            }
            _ => {}
        }
        v
    }
}

fn apply_snapshot(o: &mut ObsState, s: Snapshot) {
    *o = ObsState {
        connected: true,
        program_scene: s.program_scene,
        scenes: s.scenes,
        record_active: s.record_active,
        record_paused: s.record_paused,
        stream_active: s.stream_active,
        stream_started: s.stream_started,
        replay_active: s.replay_active,
        mutes: s.mutes,
        sources: s.sources,
    };
}

/// Icône « coupée » correspondant à l'icône configurée.
fn muted_icon(icon: Option<&str>) -> String {
    match icon {
        Some(i) if i.contains("speaker") => "obs/speaker-off.svg".into(),
        _ => "obs/mic-off.svg".into(),
    }
}

/// 75 s → "01:15", 3725 s → "1:02:05".
pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrono() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(75), "01:15");
        assert_eq!(format_duration(3725), "1:02:05");
    }

    #[test]
    fn icone_coupee() {
        assert_eq!(muted_icon(Some("obs/speaker.svg")), "obs/speaker-off.svg");
        assert_eq!(muted_icon(Some("obs/mic.svg")), "obs/mic-off.svg");
        assert_eq!(muted_icon(None), "obs/mic-off.svg");
    }
}
