//! soomfon-editor — éditeur graphique de la configuration soomfon-deck.
//!
//! Lit et écrit `%APPDATA%\soomfon-deck\config.toml`. Chaque modification est enregistrée
//! automatiquement (après une courte pause) ; le démon la recharge aussitôt et le boîtier
//! se met à jour en direct. Les aperçus utilisent exactement le même moteur de dessin que le
//! démon : ce que tu vois ici est ce qui s'affiche sur le boîtier.

#![windows_subsystem = "windows"] // pas de fenêtre console

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use slint::{Color, ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use soomfon_deck::{
    clock,
    coil::CoilView,
    config::{self, Action, Config, KeyConfig, ObsConfig, Page, SideWidget, StreamerbotConfig, parse_color},
    media::MediaKey,
    layout::{BACK_POS, KEYS, SIDE},
    render::{KeyVisual, PreviewImage, Renderer, list_icons},
    media::NowPlaying,
    plugins::{countdown, hotkey, launch, sound},
    visuals::{SWATCHES, SideState, back_visual, base_visual, side_label, side_visual},
};

slint::include_modules!();

mod autostart;
mod overlay_obs;
mod overlays_ui;

// ── Types d'action proposés ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Empty,
    Scene,
    Record,
    RecordPause,
    Stream,
    Media,
    Mute,
    Source,
    Replay,
    Timer,
    Folder,
    ScenesFolder,
    MusicKey,
    Streamerbot,
    Coil,
    Hotkey,
    Launch,
    Sound,
    Countdown,
    Decor,
}

const KINDS: [(Kind, &str); 20] = [
    (Kind::Empty, "— Touche vide —"),
    (Kind::Scene, "OBS · Changer de scène"),
    (Kind::Record, "OBS · Enregistrer (démarrer / arrêter)"),
    (Kind::RecordPause, "OBS · Pause de l'enregistrement"),
    (Kind::Stream, "OBS · Live (démarrer / arrêter)"),
    (Kind::Media, "OBS · Jouer un média (meme, son…)"),
    (Kind::Mute, "OBS · Couper / rétablir un son"),
    (Kind::Source, "OBS · Afficher / masquer une source"),
    (Kind::Replay, "OBS · Sauvegarder le replay"),
    (Kind::Timer, "Chrono du live"),
    (Kind::Folder, "Ouvrir un dossier"),
    (Kind::ScenesFolder, "Dossier automatique des scènes OBS"),
    (Kind::MusicKey, "Musique · Lecture, pistes, volume"),
    (Kind::Streamerbot, "Streamer.bot · Lancer une action"),
    (Kind::Coil, "Coil · Pause entre deux taffes"),
    (Kind::Hotkey, "Clavier · Raccourci / macro"),
    (Kind::Launch, "Lancer une appli, un site, un fichier"),
    (Kind::Sound, "Soundboard · Jouer un son"),
    (Kind::Countdown, "Compte à rebours"),
    (Kind::Decor, "Décoration (aucune action)"),
];

/// Durées de pause proposées pour Coil (comme dans l'appli : 15 s, 30 s ou perso).
const COIL_DURATIONS: [u32; 7] = [10, 15, 20, 30, 45, 60, 90];

fn kind_of(a: &Action) -> Kind {
    match a {
        Action::ObsScene { .. } => Kind::Scene,
        Action::ObsRecord => Kind::Record,
        Action::ObsRecordPause => Kind::RecordPause,
        Action::ObsStream => Kind::Stream,
        Action::ObsMedia { .. } => Kind::Media,
        Action::ObsMute { .. } => Kind::Mute,
        Action::ObsSource { .. } => Kind::Source,
        Action::ObsReplaySave => Kind::Replay,
        Action::StreamTimer => Kind::Timer,
        Action::Folder { .. } => Kind::Folder,
        Action::ObsScenesFolder => Kind::ScenesFolder,
        Action::Media { .. } => Kind::MusicKey,
        Action::Streamerbot { .. } => Kind::Streamerbot,
        Action::Coil { .. } => Kind::Coil,
        Action::Hotkey { .. } => Kind::Hotkey,
        Action::Launch { .. } => Kind::Launch,
        Action::Sound { .. } => Kind::Sound,
        Action::Countdown { .. } => Kind::Countdown,
        Action::None => Kind::Decor,
    }
}

fn param_kind(k: Kind) -> &'static str {
    match k {
        Kind::Scene => "scene",
        Kind::Source => "source",
        Kind::Media => "media",
        Kind::Mute => "input",
        Kind::Folder => "folder",
        Kind::MusicKey => "mediakey",
        Kind::Streamerbot => "sbaction",
        Kind::Coil => "coil",
        Kind::Hotkey => "hotkey",
        Kind::Launch => "launch",
        Kind::Sound => "sound",
        Kind::Countdown => "countdown",
        _ => "",
    }
}

/// Icône proposée par défaut pour une action.
fn default_icon(a: &Action) -> Option<String> {
    let icon = match a {
        Action::ObsScene { .. } => "obs/scene.svg",
        Action::ObsRecord => "obs/record.svg",
        Action::ObsRecordPause => "obs/record-pause.svg",
        Action::ObsStream => "obs/live.svg",
        Action::ObsMedia { .. } => "misc/meme.svg",
        Action::ObsMute { .. } => "obs/mic.svg",
        Action::ObsSource { .. } => "obs/source-show.svg",
        Action::ObsReplaySave => "obs/replay-buffer.svg",
        Action::StreamTimer => "misc/timer.svg",
        Action::Folder { .. } => "nav/folder.svg",
        Action::ObsScenesFolder => "obs/scene.svg",
        Action::Media { key } => key.default_look().1,
        Action::Streamerbot { .. } => "apps/streamerbot.png",
        Action::Coil { .. } => "misc/coil.svg",
        Action::Hotkey { .. } => "misc/keyboard.svg",
        Action::Launch { .. } => "misc/launch.svg",
        Action::Sound { .. } => "misc/soundboard.svg",
        Action::Countdown { .. } => "misc/hourglass.svg",
        Action::None => return None,
    };
    Some(icon.to_string())
}

/// Texte proposé par défaut pour une action.
fn default_label(a: &Action) -> Option<String> {
    let label = match a {
        Action::ObsScene { scene } => scene.clone(),
        Action::ObsRecord => "REC".into(),
        Action::ObsRecordPause => "Pause REC".into(),
        Action::ObsStream => "LIVE".into(),
        Action::ObsMedia { input } | Action::ObsMute { input } => input.clone(),
        Action::ObsSource { source, .. } => source.clone(),
        Action::ObsReplaySave => "Replay".into(),
        Action::Folder { page } => page.clone(),
        Action::ObsScenesFolder => "Scènes".into(),
        Action::Media { key } => key.default_look().0.to_string(),
        Action::Streamerbot { action } => action.clone(),
        Action::Coil { seconds } => format!("{seconds} s"),
        Action::Hotkey { keys } => {
            let pretty = hotkey::pretty(keys);
            if pretty.is_empty() || pretty.chars().count() > 14 { "Raccourci".into() } else { pretty }
        }
        Action::Launch { target, .. } if target.trim().is_empty() => "Lancer".into(),
        Action::Launch { target, .. } => launch::short_name(target),
        Action::Sound { file, .. } if file.trim().is_empty() => "Son".into(),
        Action::Sound { file, .. } => launch::short_name(file),
        Action::Countdown { seconds } => countdown::label(*seconds),
        Action::StreamTimer | Action::None => return None,
    };
    (!label.is_empty()).then_some(label)
}

// ── État ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct ObsLists {
    connected: bool,
    error: Option<String>,
    scenes: Vec<String>,
    inputs: Vec<String>,
    media: Vec<String>,
    sources: HashMap<String, Vec<String>>,
    /// Actions Streamer.bot (« Groupe · Nom » affiché, nom réel à côté).
    sb_actions: Vec<String>,
    sb_error: Option<String>,
}

struct State {
    cfg: Config,
    path: PathBuf,
    icons_dir: PathBuf,
    renderer: Renderer,
    /// Index 0 = « (aucune) ».
    icons: Vec<String>,
    page: usize,
    sel: Option<usize>,
    /// Case de l'écran latéral sélectionnée (exclusif avec `sel`).
    side_sel: Option<usize>,
    obs: ObsLists,
    /// Actions proposées pour la page courante (pas de dossier dans un dossier).
    kinds_view: Vec<Kind>,
    /// Options actuellement affichées dans les listes (pour retrouver le choix par index).
    scene_opts: Vec<String>,
    input_opts: Vec<String>,
    source_opts: Vec<String>,
    sb_opts: Vec<String>,
}

impl State {
    fn is_folder(&self) -> bool {
        self.page > 0
    }

    fn key_index(&self) -> Option<usize> {
        let pos = self.sel?;
        self.cfg.pages[self.page].keys.iter().position(|k| k.pos == pos)
    }

    fn key(&self) -> Option<&KeyConfig> {
        self.key_index().map(|i| &self.cfg.pages[self.page].keys[i])
    }

    fn key_mut(&mut self) -> Option<&mut KeyConfig> {
        let i = self.key_index()?;
        Some(&mut self.cfg.pages[self.page].keys[i])
    }

    fn folder_ids(&self) -> Vec<String> {
        self.cfg.pages.iter().skip(1).map(|p| p.id.clone()).collect()
    }

    /// Remplace l'action de la touche sélectionnée en mettant à jour icône et texte
    /// s'ils étaient encore ceux proposés par défaut.
    fn set_action(&mut self, new: Action) {
        let Some(key) = self.key_mut() else { return };
        let old = std::mem::replace(&mut key.action, new);
        if key.icon.is_none() || key.icon == default_icon(&old) {
            key.icon = default_icon(&key.action);
        }
        if key.label.is_none() || key.label == default_label(&old) {
            key.label = default_label(&key.action);
        }
    }
}

#[derive(Clone)]
struct Ctx {
    st: Rc<RefCell<State>>,
    ui: slint::Weak<MainWindow>,
    save_timer: Rc<Timer>,
    obs_shared: Arc<Mutex<Option<ObsLists>>>,
    /// Une modification attend d'être enregistrée.
    dirty: Rc<Cell<bool>>,
}

impl Ctx {
    fn with(&self, f: impl FnOnce(&MainWindow, &mut State)) {
        if let Some(ui) = self.ui.upgrade() {
            f(&ui, &mut self.st.borrow_mut());
        }
    }

    /// Enregistre après une courte pause (regroupe les frappes au clavier).
    fn schedule_save(&self) {
        self.dirty.set(true);
        let ctx = self.clone();
        self.save_timer.start(TimerMode::SingleShot, Duration::from_millis(400), move || ctx.save_now());
        if let Some(ui) = self.ui.upgrade() {
            set_status(&ui, "Enregistrement…", false);
        }
    }

    fn save_now(&self) {
        self.save_timer.stop();
        self.dirty.set(false);
        self.with(|ui, st| match config::save(&st.path, &st.cfg) {
            Ok(()) => set_status(ui, "Enregistré — le boîtier est à jour", false),
            Err(e) => set_status(ui, &format!("Non enregistré : {}", e.lines().next().unwrap_or("")), true),
        });
    }
}

/// Flocons pseudo-aléatoires (même rendu que le lanceur, en plus discret).
fn flakes(count: usize) -> Vec<Flake> {
    let mut seed: u32 = 0x5EED_B1AC;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    (0..count)
        .map(|_| {
            let depth = rnd();
            Flake {
                x: rnd(),
                speed: 0.03 + depth * 0.06,
                phase: rnd(),
                size: 1.2 + depth * 2.5,
                drift: 0.3 + rnd() * 1.0,
                opacity: 0.12 + depth * 0.35,
            }
        })
        .collect()
}

fn set_status(ui: &MainWindow, text: &str, error: bool) {
    ui.set_status(text.into());
    ui.set_status_error(error);
}

// ── Conversion d'images ───────────────────────────────────────────────────────

fn to_image(rgb: PreviewImage) -> Image {
    let (w, h) = rgb.dimensions();
    Image::from_rgb8(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(rgb.as_raw(), w, h))
}

fn string_model(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.iter().map(SharedString::from).collect::<Vec<_>>()))
}

/// Ajoute `current` à la liste s'il n'y figure pas, et renvoie son index (-1 si vide).
fn with_current(mut options: Vec<String>, current: &str) -> (Vec<String>, i32) {
    if current.is_empty() {
        return (options, -1);
    }
    let idx = match options.iter().position(|o| o == current) {
        Some(i) => i,
        None => {
            options.insert(0, current.to_string());
            0
        }
    };
    (options, idx as i32)
}

// ── Rafraîchissements de l'interface ──────────────────────────────────────────

fn refresh_all(ui: &MainWindow, st: &mut State) {
    refresh_pages(ui, st);
    refresh_tiles(ui, st);
    refresh_panel(ui, st);
}

fn refresh_pages(ui: &MainWindow, st: &mut State) {
    let items: Vec<PageItem> = st
        .cfg
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| PageItem {
            name: if i == 0 { "Page racine".into() } else { p.id.as_str().into() },
            folder: i > 0,
        })
        .collect();
    ui.set_pages(ModelRc::new(VecModel::from(items)));
    ui.set_current_page(st.page as i32);
    ui.set_is_folder(st.is_folder());
    ui.set_folder_name(st.cfg.pages[st.page].id.as_str().into());
    let hint = if st.is_folder() {
        format!("Dossier « {} » : 14 touches + Retour (en bas à droite).", st.cfg.pages[st.page].id)
    } else {
        "Page racine. Clique une touche pour la modifier : le boîtier se met à jour en direct.".to_string()
    };
    ui.set_hint(hint.into());
}

fn refresh_tiles(ui: &MainWindow, st: &mut State) {
    let folder = st.is_folder();
    let page = st.cfg.pages[st.page].clone();
    let tiles: Vec<Tile> = (0..KEYS)
        .map(|pos| {
            let (visual, empty, locked) = if folder && pos == BACK_POS {
                (back_visual(), false, true)
            } else if let Some(k) = page.keys.iter().find(|k| k.pos == pos) {
                (base_visual(k), false, false)
            } else {
                (KeyVisual::blank(), true, false)
            };
            Tile { image: to_image(st.renderer.preview(&visual)), empty, locked }
        })
        .collect();
    ui.set_tiles(ModelRc::new(VecModel::from(tiles)));

    // Écran latéral : aperçu avec un exemple de musique et l'heure actuelle.
    let sample = NowPlaying { title: "Titre du morceau".into(), artist: "Artiste".into(), playing: true };
    let scene = st.obs.scenes.first().cloned().unwrap_or_else(|| "Scène".into());
    let state = SideState {
        music: Some(&sample),
        stream_elapsed: None,
        scene: Some(&scene),
        obs_connected: true,
        time: clock::now(),
        // Coil au repos (« PRÊT ») : le compteur réel du jour est celui du démon.
        coil: CoilView { remaining: None, progress: 1.0, heat: 0.0, today: 0, waiting: false, ready: false, phase: 0 },
    };
    let side: Vec<Tile> = (0..SIDE)
        .map(|i| {
            let widget = st.cfg.side.get(i).copied().unwrap_or(SideWidget::None);
            Tile { image: to_image(st.renderer.preview(&side_visual(widget, &state))), empty: false, locked: false }
        })
        .collect();
    ui.set_side_tiles(ModelRc::new(VecModel::from(side)));
}

fn refresh_panel(ui: &MainWindow, st: &mut State) {
    let folder = st.is_folder();
    ui.set_selected_pos(st.sel.map(|p| p as i32).unwrap_or(-1));
    ui.set_selected_side(st.side_sel.map(|i| i as i32).unwrap_or(-1));
    if let Some(i) = st.side_sel {
        let widget = st.cfg.side.get(i).copied().unwrap_or(SideWidget::None);
        let labels: Vec<String> = SideWidget::ALL.iter().map(|w| side_label(*w).to_string()).collect();
        ui.set_side_options(string_model(&labels));
        ui.set_side_index(SideWidget::ALL.iter().position(|w| *w == widget).map(|p| p as i32).unwrap_or(-1));
    }
    ui.set_selected_locked(folder && st.sel == Some(BACK_POS));

    st.kinds_view = KINDS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !(folder && matches!(k, Kind::Folder | Kind::ScenesFolder)))
        .collect();
    let names: Vec<String> = st
        .kinds_view
        .iter()
        .map(|k| KINDS.iter().find(|(kk, _)| kk == k).unwrap().1.to_string())
        .collect();
    ui.set_action_names(string_model(&names));

    let key = st.key().cloned();
    ui.set_selected_empty(key.is_none());
    let kind = key.as_ref().map(|k| kind_of(&k.action)).unwrap_or(Kind::Empty);
    ui.set_action_index(st.kinds_view.iter().position(|k| *k == kind).unwrap_or(0) as i32);
    ui.set_param_kind(param_kind(kind).into());

    // Paramètres de l'action.
    let (scene, input, source, page_target) = match key.as_ref().map(|k| &k.action) {
        Some(Action::ObsScene { scene }) => (scene.clone(), String::new(), String::new(), String::new()),
        Some(Action::ObsSource { scene, source }) => (scene.clone(), String::new(), source.clone(), String::new()),
        Some(Action::ObsMedia { input } | Action::ObsMute { input }) => {
            (String::new(), input.clone(), String::new(), String::new())
        }
        Some(Action::Folder { page }) => (String::new(), String::new(), String::new(), page.clone()),
        _ => Default::default(),
    };

    let (scene_opts, scene_idx) = with_current(st.obs.scenes.clone(), &scene);
    ui.set_scene_options(string_model(&scene_opts));
    ui.set_scene_index(scene_idx);
    ui.set_scene_text(scene.as_str().into());
    st.scene_opts = scene_opts;

    let base_inputs = if kind == Kind::Media && !st.obs.media.is_empty() {
        st.obs.media.clone()
    } else {
        st.obs.inputs.clone()
    };
    let (input_opts, input_idx) = with_current(base_inputs, &input);
    ui.set_input_options(string_model(&input_opts));
    ui.set_input_index(input_idx);
    ui.set_input_text(input.as_str().into());
    st.input_opts = input_opts;

    let base_sources = st.obs.sources.get(&scene).cloned().unwrap_or_default();
    let (source_opts, source_idx) = with_current(base_sources, &source);
    ui.set_source_options(string_model(&source_opts));
    ui.set_source_index(source_idx);
    ui.set_source_text(source.as_str().into());
    st.source_opts = source_opts;

    let coil_labels: Vec<String> = COIL_DURATIONS.iter().map(|s| format!("{s} secondes")).collect();
    ui.set_coil_options(string_model(&coil_labels));
    let coil_secs = match key.as_ref().map(|k| &k.action) {
        Some(Action::Coil { seconds }) => Some(*seconds),
        _ => None,
    };
    ui.set_coil_index(coil_secs.and_then(|s| COIL_DURATIONS.iter().position(|x| *x == s)).map(|i| i as i32).unwrap_or(-1));

    let media_labels: Vec<String> = MediaKey::ALL.iter().map(|k| k.label().to_string()).collect();
    ui.set_mediakey_options(string_model(&media_labels));
    let media_key = match key.as_ref().map(|k| &k.action) {
        Some(Action::Media { key }) => Some(*key),
        _ => None,
    };
    ui.set_mediakey_index(media_key.and_then(|k| MediaKey::ALL.iter().position(|x| *x == k)).map(|i| i as i32).unwrap_or(-1));

    let sb_action = match key.as_ref().map(|k| &k.action) {
        Some(Action::Streamerbot { action }) => action.clone(),
        _ => String::new(),
    };
    let (sb_opts, sb_idx) = with_current(st.obs.sb_actions.clone(), &sb_action);
    ui.set_sb_options(string_model(&sb_opts));
    ui.set_sb_index(sb_idx);
    ui.set_sb_text(sb_action.as_str().into());
    st.sb_opts = sb_opts;
    let sb_hint = match (&st.obs.sb_error, st.obs.sb_actions.len()) {
        (Some(_), _) => "Streamer.bot fermé : tape le nom exact de l'action, ou ouvre Streamer.bot puis « Actualiser ».".to_string(),
        (None, 0) => "Aucune action dans Streamer.bot pour l'instant.".to_string(),
        (None, n) => format!("{n} action(s) lue(s) dans Streamer.bot. Ex. : un clip, un marqueur, un message dans le chat."),
    };
    ui.set_sb_hint(sb_hint.into());

    // Plugins : raccourci, lancer, soundboard, compte à rebours.
    let action = key.as_ref().map(|k| k.action.clone());
    let keys = match &action {
        Some(Action::Hotkey { keys }) => keys.clone(),
        _ => String::new(),
    };
    ui.set_hotkey_text(keys.as_str().into());
    let (hint, hint_error) = match hotkey::parse(&keys) {
        _ if keys.trim().is_empty() => ("Exemples : ctrl+shift+m · f13 · win+shift+s · macro : ctrl+c, ctrl+v".to_string(), false),
        Ok(_) => (format!("✓  {}", hotkey::pretty(&keys)), false),
        Err(e) => (format!("⚠  {e}"), true),
    };
    ui.set_hotkey_hint(hint.into());
    ui.set_hotkey_error(hint_error);
    let (target, args) = match &action {
        Some(Action::Launch { target, args }) => (target.clone(), args.clone()),
        _ => Default::default(),
    };
    ui.set_launch_text(target.as_str().into());
    ui.set_launch_args(args.as_str().into());
    let (file, volume) = match &action {
        Some(Action::Sound { file, volume }) => (file.clone(), *volume),
        _ => (String::new(), sound::DEFAULT_VOLUME),
    };
    ui.set_sound_text(file.as_str().into());
    ui.set_sound_volume(volume as f32);
    let countdown_labels: Vec<String> = countdown::DURATIONS.iter().map(|s| countdown::label(*s)).collect();
    ui.set_countdown_options(string_model(&countdown_labels));
    let countdown_secs = match &action {
        Some(Action::Countdown { seconds }) => Some(*seconds),
        _ => None,
    };
    ui.set_countdown_index(
        countdown_secs.and_then(|s| countdown::DURATIONS.iter().position(|x| *x == s)).map(|i| i as i32).unwrap_or(-1),
    );

    let folders = st.folder_ids();
    ui.set_folder_index(folders.iter().position(|f| *f == page_target).map(|i| i as i32).unwrap_or(-1));
    ui.set_folder_options(string_model(&folders));

    // Apparence.
    let label = key.as_ref().and_then(|k| k.label.clone()).unwrap_or_default();
    ui.set_key_label(label.as_str().into());
    let color = key.as_ref().and_then(|k| k.color.clone()).unwrap_or_default();
    ui.set_key_color(color.as_str().into());

    let icon = key.as_ref().and_then(|k| k.icon.clone());
    let icon_idx = match &icon {
        None => 0,
        Some(path) => match st.icons.iter().position(|i| i == path) {
            Some(i) => i,
            None => {
                st.icons.push(path.clone());
                st.icons.len() - 1
            }
        },
    };
    ui.set_icons(string_model(&st.icons));
    ui.set_icon_index(icon_idx as i32);
    refresh_icon_preview(ui, st, icon);
}

fn refresh_icon_preview(ui: &MainWindow, st: &mut State, icon: Option<String>) {
    let visual = KeyVisual { bg: [13, 15, 20], icon, ..Default::default() };
    ui.set_icon_preview(to_image(st.renderer.preview(&visual)));
}

fn refresh_obs_status(ui: &MainWindow, st: &State) {
    let text = if st.obs.connected {
        format!("OBS connecté · {} scène(s) · {} source(s)", st.obs.scenes.len(), st.obs.inputs.len())
    } else if st.obs.error.is_some() {
        // Message court : le détail technique (en anglais) n'aide pas l'utilisateur.
        "OBS fermé · noms à saisir à la main".to_string()
    } else {
        "Connexion à OBS…".to_string()
    };
    ui.set_obs_status(text.into());
    ui.set_obs_ok(st.obs.connected);
}

// ── OBS (thread de fond) ──────────────────────────────────────────────────────

fn fetch_obs(
    settings: ObsConfig,
    sb: StreamerbotConfig,
    shared: Arc<Mutex<Option<ObsLists>>>,
    ui: slint::Weak<MainWindow>,
) {
    std::thread::spawn(move || {
        let lists = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(async {
                let mut lists = load_obs_lists(&settings).await;
                match load_sb_actions(&sb).await {
                    Ok(actions) => lists.sb_actions = actions,
                    Err(e) => lists.sb_error = Some(e),
                }
                lists
            }),
            Err(e) => ObsLists { error: Some(e.to_string()), ..Default::default() },
        };
        *shared.lock().unwrap() = Some(lists);
        let _ = ui.upgrade_in_event_loop(|ui| ui.invoke_obs_loaded());
    });
}

async fn load_obs_lists(settings: &ObsConfig) -> ObsLists {
    use obws::requests::scenes::SceneId;

    let client = match soomfon_deck::obs::connect(settings, false).await {
        Ok(c) => c,
        Err(e) => return ObsLists { error: Some(short_error(&e.to_string())), ..Default::default() },
    };
    let mut lists = ObsLists { connected: true, ..Default::default() };

    if let Ok(scenes) = client.scenes().list().await {
        lists.scenes = scenes.scenes.into_iter().rev().map(|s| s.id.name).collect();
    }
    if let Ok(inputs) = client.inputs().list(None).await {
        for input in inputs {
            let kind = input.unversioned_kind.as_str();
            if ["ffmpeg_source", "vlc_source"].contains(&kind) {
                lists.media.push(input.id.name.clone());
            }
            lists.inputs.push(input.id.name);
        }
        lists.inputs.sort();
        lists.media.sort();
    }
    for scene in lists.scenes.clone() {
        if let Ok(items) = client.scene_items().list(SceneId::Name(&scene)).await {
            lists.sources.insert(scene, items.into_iter().map(|i| i.source_name).collect());
        }
    }
    lists
}

async fn load_sb_actions(cfg: &StreamerbotConfig) -> Result<Vec<String>, String> {
    let target = soomfon_deck::streamerbot::resolve(cfg);
    let mut client = soomfon_deck::streamerbot::Client::connect(&target).await?;
    let actions = client.actions().await;
    client.close().await;
    Ok(actions?.into_iter().map(|a| a.name).collect())
}

fn short_error(e: &str) -> String {
    let e = e.lines().next().unwrap_or(e);
    if e.chars().count() > 60 { format!("{}…", e.chars().take(60).collect::<String>()) } else { e.to_string() }
}

// ── Programme principal ───────────────────────────────────────────────────────

fn main() -> Result<(), slint::PlatformError> {
    let path = config::config_path();
    let (cfg, load_error) = match config::load_or_create(&path) {
        Ok(cfg) => (cfg, None),
        Err(e) => (config::parse(config::DEFAULT_CONFIG).expect("exemple valide"), Some(e)),
    };
    let icons_dir = cfg.icons_dir.clone().unwrap_or_else(config::find_icons_dir);
    let mut icons = vec!["(aucune)".to_string()];
    icons.extend(list_icons(&icons_dir));

    let ui = MainWindow::new()?;
    let st = State {
        renderer: Renderer::new(icons_dir.clone(), cfg.jpeg_quality),
        cfg,
        path: path.clone(),
        icons_dir,
        icons,
        page: 0,
        sel: None,
        side_sel: None,
        obs: ObsLists::default(),
        kinds_view: Vec::new(),
        scene_opts: Vec::new(),
        input_opts: Vec::new(),
        source_opts: Vec::new(),
        sb_opts: Vec::new(),
    };
    let ctx = Ctx {
        st: Rc::new(RefCell::new(st)),
        ui: ui.as_weak(),
        save_timer: Rc::new(Timer::default()),
        obs_shared: Arc::new(Mutex::new(None)),
        dirty: Rc::new(Cell::new(false)),
    };
    ui.set_flakes(ModelRc::new(VecModel::from(flakes(45))));

    {
        let mut st = ctx.st.borrow_mut();
        ui.set_brightness(st.cfg.brightness as f32);
        let colors: Vec<Color> = SWATCHES
            .iter()
            .map(|s| parse_color(s).map(|[r, g, b]| Color::from_rgb_u8(r, g, b)).unwrap_or_default())
            .collect();
        ui.set_swatch_colors(ModelRc::new(VecModel::from(colors)));
        let values: Vec<String> = SWATCHES.iter().map(|s| s.to_string()).collect();
        ui.set_swatch_values(string_model(&values));
        refresh_all(&ui, &mut st);
        refresh_obs_status(&ui, &st);
        match &load_error {
            Some(e) => set_status(&ui, &format!("Config illisible, exemple chargé : {}", short_error(e)), true),
            None => set_status(&ui, &format!("Config : {}", path.display()), false),
        }
    }

    install_callbacks(&ui, &ctx);
    // Onglet Overlays (reste vivant jusqu'à la fermeture de la fenêtre).
    let overlays = overlays_ui::init(&ui, ctx.st.borrow().cfg.obs.clone());

    // Fermeture : enregistre ce qui attend (Stream Deck + overlays), remet OBS comme avant.
    let shutdown = {
        let deck = ctx.clone();
        let overlays = overlays.clone();
        move || {
            if deck.dirty.get() {
                deck.save_now();
            }
            overlays.shutdown();
        }
    };
    let on_close = shutdown.clone();
    ui.on_close_window(move || {
        on_close();
        let _ = slint::quit_event_loop();
    });
    // Fermeture par la barre des tâches (Alt+F4, clic droit → Fermer).
    let on_request = shutdown.clone();
    ui.window().on_close_requested(move || {
        on_request();
        slint::CloseRequestResponse::HideWindow
    });

    // Aide au test : SOOMFON_EDITOR_QUIT_AFTER=<s> ferme proprement l'éditeur après s secondes.
    if let Some(secs) = std::env::var("SOOMFON_EDITOR_QUIT_AFTER").ok().and_then(|v| v.parse::<u64>().ok()) {
        let weak = ui.as_weak();
        Timer::single_shot(Duration::from_secs(secs), move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_close_window();
            }
        });
    }

    // Aide au développement : SOOMFON_EDITOR_TAB=1 ouvre directement l'onglet Overlays.
    if std::env::var("SOOMFON_EDITOR_TAB").as_deref() == Ok("1") {
        ui.set_section(1);
        ui.invoke_section_changed(1);
    }

    // Aide au développement : SOOMFON_EDITOR_SELECT=<page>:<pos> présélectionne une touche.
    if let Ok(v) = std::env::var("SOOMFON_EDITOR_SELECT") {
        if let Some((page, pos)) = v.split_once(':').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))) {
            ui.invoke_select_page(page);
            ui.invoke_select_key(pos);
        }
    }
    // SOOMFON_EDITOR_SNAPSHOT=<fichier.png> : capture la fenêtre après 2 s puis ferme (vérifier le style).
    if let Ok(out) = std::env::var("SOOMFON_EDITOR_SNAPSHOT") {
        let weak = ui.as_weak();
        Timer::single_shot(Duration::from_millis(2000), move || {
            let Some(ui) = weak.upgrade() else { return };
            match ui.window().take_snapshot() {
                Ok(buf) => {
                    let img = image::RgbaImage::from_raw(buf.width(), buf.height(), buf.as_bytes().to_vec());
                    if let Some(img) = img {
                        let _ = img.save(&out);
                    }
                }
                Err(e) => eprintln!("capture impossible : {e}"),
            }
            ui.invoke_close_window();
        });
    }
    // SOOMFON_EDITOR_SIDE=<0..2> présélectionne une case de l'écran latéral.
    if let Some(i) = std::env::var("SOOMFON_EDITOR_SIDE").ok().and_then(|v| v.parse().ok()) {
        ui.invoke_select_side(i);
    }
    {
        let cfg = &ctx.st.borrow().cfg;
        fetch_obs(cfg.obs.clone(), cfg.streamerbot.clone(), ctx.obs_shared.clone(), ui.as_weak());
    }

    ui.run()
}

fn install_callbacks(ui: &MainWindow, ctx: &Ctx) {
    // Fenêtre sans bordure : déplacement, réduction, fermeture.
    let c = ctx.clone();
    ui.on_drag(move |dx, dy| {
        if let Some(ui) = c.ui.upgrade() {
            let w = ui.window();
            let s = w.scale_factor();
            let p = w.position();
            w.set_position(slint::PhysicalPosition::new(p.x + (dx * s) as i32, p.y + (dy * s) as i32));
        }
    });
    let c = ctx.clone();
    ui.on_minimize_window(move || {
        if let Some(ui) = c.ui.upgrade() {
            ui.window().set_minimized(true);
        }
    });
    let c = ctx.clone();
    // La fermeture (bouton ×) est branchée dans main(), une fois l'onglet Overlays prêt.
    let _ = c;

    // Pages
    let c = ctx.clone();
    ui.on_select_page(move |i| {
        c.with(|ui, st| {
            st.page = (i.max(0) as usize).min(st.cfg.pages.len() - 1);
            st.sel = None;
            refresh_all(ui, st);
        })
    });

    let c = ctx.clone();
    ui.on_add_folder(move || {
        c.with(|ui, st| {
            let mut n = st.cfg.pages.len();
            let id = loop {
                let candidate = format!("dossier {n}");
                if st.cfg.page_index(&candidate).is_none() {
                    break candidate;
                }
                n += 1;
            };
            st.cfg.pages.push(Page { id, keys: Vec::new() });
            st.page = st.cfg.pages.len() - 1;
            st.sel = None;
            refresh_all(ui, st);
        });
        c.schedule_save();
    });

    let c = ctx.clone();
    ui.on_delete_folder(move || {
        c.with(|ui, st| {
            if !st.is_folder() {
                return;
            }
            let id = st.cfg.pages.remove(st.page).id;
            st.cfg.pages[0].keys.retain(|k| k.action != Action::Folder { page: id.clone() });
            st.page = 0;
            st.sel = None;
            refresh_all(ui, st);
        });
        c.schedule_save();
    });

    let c = ctx.clone();
    ui.on_rename_folder(move |name| {
        let mut ok = false;
        c.with(|ui, st| {
            let new = name.trim().to_string();
            let old = st.cfg.pages[st.page].id.clone();
            if !st.is_folder() || new == old {
                return;
            }
            if new.is_empty() || st.cfg.page_index(&new).is_some() {
                set_status(ui, "Nom de dossier vide ou déjà utilisé", true);
                return;
            }
            st.cfg.pages[st.page].id = new.clone();
            for key in &mut st.cfg.pages[0].keys {
                if key.action == (Action::Folder { page: old.clone() }) {
                    if key.label.as_deref() == Some(old.as_str()) {
                        key.label = Some(new.clone());
                    }
                    key.action = Action::Folder { page: new.clone() };
                }
            }
            refresh_all(ui, st);
            ok = true;
        });
        if ok {
            c.schedule_save();
        }
    });

    // Sélection d'une touche
    let c = ctx.clone();
    ui.on_select_key(move |pos| {
        c.with(|ui, st| {
            st.sel = Some(pos as usize);
            st.side_sel = None;
            refresh_panel(ui, st);
        })
    });

    // Écran latéral : sélection d'une case, puis choix de son contenu.
    let c = ctx.clone();
    ui.on_select_side(move |i| {
        c.with(|ui, st| {
            st.side_sel = Some(i as usize);
            st.sel = None;
            refresh_panel(ui, st);
        })
    });
    let c = ctx.clone();
    ui.on_side_chosen(move |idx| {
        let mut changed = false;
        c.with(|ui, st| {
            let (Some(i), Some(widget)) = (st.side_sel, SideWidget::ALL.get(idx as usize).copied()) else { return };
            while st.cfg.side.len() < SIDE {
                st.cfg.side.push(SideWidget::None);
            }
            if st.cfg.side[i] != widget {
                st.cfg.side[i] = widget;
                changed = true;
                refresh_tiles(ui, st);
            }
        });
        if changed {
            c.schedule_save();
        }
    });

    // Action
    let c = ctx.clone();
    ui.on_action_chosen(move |idx| {
        c.with(|ui, st| {
            let (Some(pos), Some(kind)) = (st.sel, st.kinds_view.get(idx as usize).copied()) else { return };
            if kind == Kind::Empty {
                st.cfg.pages[st.page].keys.retain(|k| k.pos != pos);
            } else {
                let first = |v: &Vec<String>| v.first().cloned().unwrap_or_default();
                let action = match kind {
                    Kind::Scene => Action::ObsScene { scene: first(&st.obs.scenes) },
                    Kind::Record => Action::ObsRecord,
                    Kind::RecordPause => Action::ObsRecordPause,
                    Kind::Stream => Action::ObsStream,
                    Kind::Media => Action::ObsMedia {
                        input: first(if st.obs.media.is_empty() { &st.obs.inputs } else { &st.obs.media }),
                    },
                    Kind::Mute => Action::ObsMute { input: first(&st.obs.inputs) },
                    Kind::Source => {
                        let scene = first(&st.obs.scenes);
                        let source = st.obs.sources.get(&scene).and_then(|s| s.first().cloned()).unwrap_or_default();
                        Action::ObsSource { scene, source }
                    }
                    Kind::Replay => Action::ObsReplaySave,
                    Kind::Timer => Action::StreamTimer,
                    Kind::Folder => {
                        if st.cfg.pages.len() < 2 {
                            st.cfg.pages.push(Page { id: "dossier 1".into(), keys: Vec::new() });
                            refresh_pages(ui, st);
                        }
                        Action::Folder { page: st.cfg.pages[1].id.clone() }
                    }
                    Kind::ScenesFolder => Action::ObsScenesFolder,
                    Kind::MusicKey => Action::Media { key: MediaKey::PlayPause },
                    Kind::Streamerbot => Action::Streamerbot { action: first(&st.obs.sb_actions) },
                    Kind::Coil => Action::Coil { seconds: 15 },
                    Kind::Hotkey => Action::Hotkey { keys: String::new() },
                    Kind::Launch => Action::Launch { target: String::new(), args: String::new() },
                    Kind::Sound => Action::Sound { file: String::new(), volume: sound::DEFAULT_VOLUME },
                    Kind::Countdown => Action::Countdown { seconds: 300 },
                    Kind::Decor | Kind::Empty => Action::None,
                };
                if st.key_index().is_none() {
                    st.cfg.pages[st.page].keys.push(KeyConfig {
                        pos,
                        icon: None,
                        label: None,
                        color: None,
                        action: Action::None,
                    });
                }
                st.set_action(action);
            }
            refresh_tiles(ui, st);
            refresh_panel(ui, st);
        });
        c.schedule_save();
    });

    // Paramètres
    fn update_param(c: &Ctx, f: impl FnOnce(&mut State) -> Option<Action>) {
        c.with(|ui, st| {
            if let Some(action) = f(st) {
                st.set_action(action);
                refresh_tiles(ui, st);
                refresh_panel(ui, st);
            }
        });
        c.schedule_save();
    }

    let c = ctx.clone();
    ui.on_scene_chosen(move |i| {
        update_param(&c, |st| {
            let scene = st.scene_opts.get(i as usize)?.clone();
            match st.key()?.action.clone() {
                Action::ObsScene { .. } => Some(Action::ObsScene { scene }),
                Action::ObsSource { .. } => {
                    let source = st.obs.sources.get(&scene).and_then(|s| s.first().cloned()).unwrap_or_default();
                    Some(Action::ObsSource { scene, source })
                }
                _ => None,
            }
        })
    });
    let c = ctx.clone();
    ui.on_scene_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::ObsScene { .. } => Some(Action::ObsScene { scene: t.to_string() }),
            Action::ObsSource { source, .. } => Some(Action::ObsSource { scene: t.to_string(), source }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_source_chosen(move |i| {
        update_param(&c, |st| {
            let source = st.source_opts.get(i as usize)?.clone();
            match st.key()?.action.clone() {
                Action::ObsSource { scene, .. } => Some(Action::ObsSource { scene, source }),
                _ => None,
            }
        })
    });
    let c = ctx.clone();
    ui.on_source_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::ObsSource { scene, .. } => Some(Action::ObsSource { scene, source: t.to_string() }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_input_chosen(move |i| {
        update_param(&c, |st| {
            let input = st.input_opts.get(i as usize)?.clone();
            match st.key()?.action.clone() {
                Action::ObsMedia { .. } => Some(Action::ObsMedia { input }),
                Action::ObsMute { .. } => Some(Action::ObsMute { input }),
                _ => None,
            }
        })
    });
    let c = ctx.clone();
    ui.on_input_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::ObsMedia { .. } => Some(Action::ObsMedia { input: t.to_string() }),
            Action::ObsMute { .. } => Some(Action::ObsMute { input: t.to_string() }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_coil_chosen(move |i| update_param(&c, |_| Some(Action::Coil { seconds: *COIL_DURATIONS.get(i.max(0) as usize)? })));
    let c = ctx.clone();
    ui.on_mediakey_chosen(move |i| {
        update_param(&c, |_| Some(Action::Media { key: *MediaKey::ALL.get(i.max(0) as usize)? }))
    });
    let c = ctx.clone();
    ui.on_sb_chosen(move |i| {
        update_param(&c, |st| Some(Action::Streamerbot { action: st.sb_opts.get(i as usize)?.clone() }))
    });
    let c = ctx.clone();
    ui.on_sb_typed(move |t| update_param(&c, |_| Some(Action::Streamerbot { action: t.to_string() })));

    // Plugins
    let c = ctx.clone();
    ui.on_hotkey_typed(move |t| update_param(&c, |_| Some(Action::Hotkey { keys: t.to_string() })));
    let c = ctx.clone();
    ui.on_launch_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Launch { args, .. } => Some(Action::Launch { target: t.to_string(), args }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_launch_args_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Launch { target, .. } => Some(Action::Launch { target, args: t.to_string() }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_launch_browse(move || {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Choisir le programme ou le fichier à ouvrir")
            .add_filter("Programmes", &["exe", "lnk", "bat", "cmd", "url"])
            .add_filter("Tous les fichiers", &["*"])
            .pick_file()
        else {
            return;
        };
        let target = file.to_string_lossy().into_owned();
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Launch { args, .. } => Some(Action::Launch { target, args }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_sound_typed(move |t| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Sound { volume, .. } => Some(Action::Sound { file: t.to_string(), volume }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_sound_browse(move || {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Choisir le son à jouer")
            .add_filter("Sons", &sound::EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        let path = file.to_string_lossy().into_owned();
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Sound { volume, .. } => Some(Action::Sound { file: path, volume }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_sound_volume_released(move |v| {
        update_param(&c, |st| match st.key()?.action.clone() {
            Action::Sound { file, .. } => Some(Action::Sound { file, volume: v.round().clamp(0.0, 100.0) as u8 }),
            _ => None,
        })
    });
    let c = ctx.clone();
    ui.on_sound_test(move || {
        c.with(|ui, st| match st.key().map(|k| k.action.clone()) {
            Some(Action::Sound { file, volume }) if std::path::Path::new(file.trim()).is_file() => {
                sound::play(&file, volume);
                set_status(ui, "Lecture du son (clique encore pour l'arrêter).", false);
            }
            Some(Action::Sound { .. }) => set_status(ui, "Choisis d'abord un fichier son qui existe.", true),
            _ => {}
        })
    });
    let c = ctx.clone();
    ui.on_countdown_chosen(move |i| {
        update_param(&c, |_| Some(Action::Countdown { seconds: *countdown::DURATIONS.get(i.max(0) as usize)? }))
    });
    let c = ctx.clone();
    ui.on_folder_chosen(move |i| {
        update_param(&c, |st| {
            let page = st.folder_ids().get(i as usize)?.clone();
            Some(Action::Folder { page })
        })
    });

    let c = ctx.clone();
    ui.on_open_folder(move || {
        c.with(|ui, st| {
            if let Some(Action::Folder { page }) = st.key().map(|k| k.action.clone()) {
                if let Some(i) = st.cfg.page_index(&page) {
                    st.page = i;
                    st.sel = None;
                    refresh_all(ui, st);
                }
            }
        })
    });

    // Apparence
    let c = ctx.clone();
    ui.on_label_edited(move |t| {
        c.with(|ui, st| {
            if let Some(key) = st.key_mut() {
                key.label = (!t.trim().is_empty()).then(|| t.to_string());
            }
            refresh_tiles(ui, st);
        });
        c.schedule_save();
    });

    let c = ctx.clone();
    ui.on_color_edited(move |t| {
        let t = t.trim().to_string();
        let mut valid = true;
        c.with(|ui, st| {
            if !t.is_empty() && parse_color(&t).is_none() {
                valid = false;
                if t.len() >= 7 {
                    set_status(ui, "Couleur invalide : format #rrggbb, ex. #7a1f2b", true);
                }
                return;
            }
            if let Some(key) = st.key_mut() {
                key.color = (!t.is_empty()).then(|| t.to_lowercase());
            }
            refresh_tiles(ui, st);
        });
        if valid {
            c.schedule_save();
        }
    });

    let c = ctx.clone();
    ui.on_icon_chosen(move |i| {
        c.with(|ui, st| {
            let icon = if i <= 0 { None } else { st.icons.get(i as usize).cloned() };
            if let Some(key) = st.key_mut() {
                key.icon = icon.clone();
            }
            refresh_tiles(ui, st);
            refresh_icon_preview(ui, st, icon);
        });
        c.schedule_save();
    });

    let c = ctx.clone();
    ui.on_clear_key(move || {
        c.with(|ui, st| {
            if let Some(pos) = st.sel {
                st.cfg.pages[st.page].keys.retain(|k| k.pos != pos);
            }
            refresh_tiles(ui, st);
            refresh_panel(ui, st);
        });
        c.schedule_save();
    });

    // Barre du haut
    let c = ctx.clone();
    ui.on_brightness_released(move |v| {
        c.with(|_, st| st.cfg.brightness = v.round().clamp(0.0, 100.0) as u8);
        c.schedule_save();
    });

    let c = ctx.clone();
    ui.on_refresh_obs(move || {
        c.with(|ui, st| {
            // Rescanne aussi les icônes (si tu en as ajouté).
            st.icons = std::iter::once("(aucune)".to_string()).chain(list_icons(&st.icons_dir)).collect();
            st.renderer.reset(st.icons_dir.clone(), st.cfg.jpeg_quality);
            st.obs = ObsLists::default();
            refresh_obs_status(ui, st);
            refresh_all(ui, st);
            fetch_obs(st.cfg.obs.clone(), st.cfg.streamerbot.clone(), c.obs_shared.clone(), c.ui.clone());
        })
    });

    // ── Démarrer avec Windows ──
    ui.set_autostart(autostart::enabled());
    ui.set_official_autostart(autostart::official_enabled());
    let c = ctx.clone();
    ui.on_autostart_toggled(move |on| {
        c.with(|ui, _| {
            match autostart::set(on) {
                Ok(()) if on => set_status(ui, "Le Stream Deck démarrera tout seul à l'ouverture de ta session Windows.", false),
                Ok(()) => set_status(ui, "Le Stream Deck ne démarrera plus avec Windows.", false),
                Err(e) => set_status(ui, &format!("Démarrage automatique : {e}"), true),
            }
            ui.set_autostart(autostart::enabled());
        })
    });
    let c = ctx.clone();
    ui.on_disable_official(move || {
        c.with(|ui, _| {
            match autostart::disable_official() {
                Ok(()) => set_status(
                    ui,
                    "Le logiciel officiel ne démarrera plus avec Windows (réactivable dans Gestionnaire des tâches → Applications de démarrage).",
                    false,
                ),
                Err(e) => set_status(ui, &format!("Logiciel officiel : {e}"), true),
            }
            ui.set_official_autostart(autostart::official_enabled());
        })
    });

    let c = ctx.clone();
    ui.on_open_icons_folder(move || {
        c.with(|ui, st| {
            let dir = st.icons_dir.join("perso");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            set_status(
                ui,
                "Dépose tes images (PNG, JPG, SVG) dans ce dossier, puis clique « Actualiser OBS » pour les voir.",
                false,
            );
        })
    });

    let c = ctx.clone();
    ui.on_obs_loaded(move || {
        let lists = c.obs_shared.lock().unwrap().take();
        c.with(|ui, st| {
            if let Some(lists) = lists {
                st.obs = lists;
            }
            refresh_obs_status(ui, st);
            refresh_panel(ui, st);
        })
    });
}
