//! Pilotage d'OBS pour l'onglet Overlays, dans un thread à part (la fenêtre ne se fige jamais).
//!
//! - Aperçu : une source navigateur « Soomfon · Aperçu overlay » dans une scène dédiée
//!   « Soomfon · Aperçu » (jamais à l'antenne). On lui donne l'URL de l'overlay et la
//!   résolution choisie, puis on demande à OBS une capture : c'est le rendu exact d'OBS.
//! - Sources réelles : liste des sources navigateur, rafraîchissement, taille, et bascule
//!   d'une source sur la copie Studio (uniquement sur demande explicite).

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::Engine;
use obws::{
    Client,
    requests::{
        inputs::{Create, InputId, SetSettings},
        scenes::SceneId,
        sources::{SourceId, TakeScreenshot},
    },
};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use soomfon_deck::config::ObsConfig;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const PREVIEW_SCENE: &str = "Soomfon · Aperçu";
pub const PREVIEW_INPUT: &str = "Soomfon · Aperçu overlay";

/// Largeur de la capture affichée dans l'éditeur.
const SHOT_WIDTH: u32 = 1280;
/// Temps laissé à la page pour se charger et jouer son animation d'entrée.
const SETTLE: Duration = Duration::from_millis(2800);

pub enum Cmd {
    /// Affiche `url` dans la source d'aperçu à la taille donnée, puis capture.
    Preview { url: String, width: u32, height: u32 },
    /// Relit la liste des sources navigateur.
    Sources,
    /// Rafraîchit les sources (hors aperçu) qui affichent l'un de ces fichiers.
    Refresh { files: Vec<String> },
    /// Fait pointer une source sur la copie Studio (en gardant ses options d'URL).
    UseStudio { input: String, studio_url: String },
    /// Donne cette taille aux sources qui affichent ce fichier.
    SetSize { file: String, width: u32, height: u32 },
    /// Remet OBS comme avant l'édition (mode studio, scène d'aperçu) et supprime la scène
    /// d'aperçu. `done` est prévenu quand c'est fini (fermeture de l'éditeur).
    Cleanup { done: Option<std::sync::mpsc::Sender<()>> },
}

/// État d'OBS avant que l'éditeur ne passe en mode studio, pour le restaurer ensuite.
struct Session {
    studio_before: bool,
    preview_before: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub name: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
}

impl SourceInfo {
    /// URL sans les options après « ? ».
    pub fn file(&self) -> &str {
        self.url.split('?').next().unwrap_or(&self.url)
    }
}

pub enum Msg {
    Connected(Result<(), String>),
    Preview(Result<SharedPixelBuffer<Rgba8Pixel>, String>),
    Sources(Vec<SourceInfo>),
    Info(String),
    Error(String),
}

pub struct ObsWorker {
    tx: UnboundedSender<Cmd>,
}

impl ObsWorker {
    /// `notify` est appelé (depuis le thread OBS) chaque fois qu'un message attend dans `inbox`.
    pub fn spawn(settings: ObsConfig, inbox: Arc<Mutex<VecDeque<Msg>>>, notify: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = unbounded_channel();
        std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let post = move |m: Msg| {
                inbox.lock().unwrap().push_back(m);
                notify();
            };
            rt.block_on(run(settings, rx, post));
        });
        Self { tx }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    /// Nettoyage en attendant qu'il soit fini (au plus `timeout`) : utilisé à la fermeture.
    pub fn cleanup_blocking(&self, timeout: Duration) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        self.send(Cmd::Cleanup { done: Some(tx) });
        rx.recv_timeout(timeout).is_ok()
    }
}

async fn run(settings: ObsConfig, mut rx: UnboundedReceiver<Cmd>, post: impl Fn(Msg)) {
    let mut client: Option<Client> = None;
    let mut session: Option<Session> = None;
    while let Some(first) = rx.recv().await {
        // Regroupe les demandes en attente : seul le dernier aperçu compte.
        let mut cmds = vec![first];
        while let Ok(c) = rx.try_recv() {
            cmds.push(c);
        }
        let last_preview = cmds.iter().rposition(|c| matches!(c, Cmd::Preview { .. }));
        let cmds: Vec<Cmd> = cmds
            .into_iter()
            .enumerate()
            .filter(|(i, c)| !matches!(c, Cmd::Preview { .. }) || Some(*i) == last_preview)
            .map(|(_, c)| c)
            .collect();

        if client.is_none() {
            match soomfon_deck::obs::connect(&settings, false).await {
                Ok(c) => {
                    client = Some(c);
                    post(Msg::Connected(Ok(())));
                }
                Err(e) => {
                    post(Msg::Connected(Err(short(&e.to_string()))));
                    // OBS fermé : rien à nettoyer, on libère quand même une fermeture en attente.
                    for cmd in cmds {
                        if let Cmd::Cleanup { done: Some(d) } = cmd {
                            let _ = d.send(());
                        }
                    }
                    session = None;
                    continue;
                }
            }
        }
        let c = client.as_ref().unwrap();
        let mut lost = false;

        for cmd in cmds {
            let result = match cmd {
                Cmd::Preview { url, width, height } => {
                    let r = preview(c, &mut session, &url, width, height).await;
                    post(Msg::Preview(r.map_err(|e| short(&e))));
                    Ok(())
                }
                Cmd::Cleanup { done } => {
                    let r = cleanup(c, session.take()).await;
                    if let Some(d) = done {
                        let _ = d.send(());
                    }
                    r.map(|()| post(Msg::Info("OBS remis comme avant (aperçu retiré)".into())))
                }
                Cmd::Sources => sources(c).await.map(|s| post(Msg::Sources(s))),
                Cmd::Refresh { files } => refresh(c, &files).await.map(|n| {
                    if n > 0 {
                        post(Msg::Info(format!("{n} source(s) rafraîchie(s) dans OBS")));
                    }
                }),
                Cmd::UseStudio { input, studio_url } => use_studio(c, &input, &studio_url).await.map(|()| {
                    post(Msg::Info(format!("« {input} » affiche maintenant la version Studio")));
                }),
                Cmd::SetSize { file, width, height } => set_size(c, &file, width, height).await.map(|n| {
                    post(Msg::Info(format!("Taille {width} × {height} appliquée à {n} source(s)")));
                }),
            };
            if let Err(e) = result {
                post(Msg::Error(short(&e)));
                lost = true;
                break;
            }
        }
        if lost {
            // Connexion probablement perdue (OBS fermé) : on se reconnectera à la prochaine demande.
            client = None;
        } else if let Ok(s) = sources(c).await {
            // Les sources ont pu changer : on renvoie la liste à jour.
            post(Msg::Sources(s));
        }
    }
}

fn short(e: &str) -> String {
    let e = e.lines().next().unwrap_or(e);
    if e.chars().count() > 90 { format!("{}…", e.chars().take(90).collect::<String>()) } else { e.to_string() }
}

fn err(e: obws::error::Error) -> String {
    e.to_string()
}

async fn ensure_preview_source(c: &Client, settings: &serde_json::Value) -> Result<bool, String> {
    let scenes = c.scenes().list().await.map_err(err)?;
    if !scenes.scenes.iter().any(|s| s.id.name == PREVIEW_SCENE) {
        c.scenes().create(PREVIEW_SCENE).await.map_err(err)?;
    }
    let inputs = c.inputs().list(Some("browser_source")).await.map_err(err)?;
    if inputs.iter().any(|i| i.id.name == PREVIEW_INPUT) {
        c.inputs()
            .set_settings(SetSettings { input: InputId::Name(PREVIEW_INPUT), settings, overlay: Some(true) })
            .await
            .map_err(err)?;
        Ok(false)
    } else {
        c.inputs()
            .create(Create {
                scene: SceneId::Name(PREVIEW_SCENE),
                input: PREVIEW_INPUT,
                kind: "browser_source",
                settings: Some(settings.clone()),
                enabled: Some(true),
            })
            .await
            .map_err(err)?;
        Ok(true)
    }
}

/// OBS met en veille les sources navigateur qui ne sont affichées nulle part : une capture
/// serait vide. On affiche donc la scène d'aperçu dans le côté « Aperçu » du mode studio
/// (jamais à l'antenne), en mémorisant l'état d'avant pour le restaurer au nettoyage.
async fn show_in_studio_preview(c: &Client, session: &mut Option<Session>) -> Result<(), String> {
    if session.is_none() {
        let studio_before = c.ui().studio_mode_enabled().await.map_err(err)?;
        let preview_before = if studio_before {
            c.scenes().current_preview_scene().await.ok().map(|s| s.id.name).filter(|n| n != PREVIEW_SCENE)
        } else {
            None
        };
        if !studio_before {
            c.ui().set_studio_mode_enabled(true).await.map_err(err)?;
        }
        *session = Some(Session { studio_before, preview_before });
    }
    c.scenes().set_current_preview_scene(SceneId::Name(PREVIEW_SCENE)).await.map_err(err)
}

async fn cleanup(c: &Client, session: Option<Session>) -> Result<(), String> {
    if let Some(s) = session {
        if !s.studio_before {
            c.ui().set_studio_mode_enabled(false).await.map_err(err)?;
        } else if let Some(previous) = s.preview_before {
            let _ = c.scenes().set_current_preview_scene(SceneId::Name(&previous)).await;
        }
    }
    // Supprime la source puis la scène d'aperçu si elles existent (erreurs ignorées : déjà absentes).
    let _ = c.inputs().remove(InputId::Name(PREVIEW_INPUT)).await;
    let scenes = c.scenes().list().await.map_err(err)?;
    if scenes.scenes.iter().any(|s| s.id.name == PREVIEW_SCENE) {
        c.scenes().remove(SceneId::Name(PREVIEW_SCENE)).await.map_err(err)?;
    }
    Ok(())
}

async fn preview(
    c: &Client,
    session: &mut Option<Session>,
    url: &str,
    width: u32,
    height: u32,
) -> Result<SharedPixelBuffer<Rgba8Pixel>, String> {
    let settings = serde_json::json!({
        "url": url,
        "is_local_file": false,
        "width": width,
        "height": height,
        // Doit continuer à se rendre hors antenne pour pouvoir être capturée.
        "shutdown": false,
        "restart_when_active": false,
        // Les sons éventuels restent dans OBS (cette scène n'est jamais diffusée).
        "reroute_audio": true,
    });
    let created = ensure_preview_source(c, &settings).await?;
    show_in_studio_preview(c, session).await?;
    if !created {
        c.inputs()
            .press_properties_button(InputId::Name(PREVIEW_INPUT), "refreshnocache")
            .await
            .map_err(err)?;
    }
    tokio::time::sleep(SETTLE).await;

    let shot_h = ((SHOT_WIDTH as f64) * (height as f64) / (width.max(1) as f64)).round().max(1.0) as u32;
    let data = c
        .sources()
        .take_screenshot(TakeScreenshot {
            source: SourceId::Name(PREVIEW_INPUT),
            format: "png",
            width: Some(SHOT_WIDTH),
            height: Some(shot_h),
            compression_quality: None,
        })
        .await
        .map_err(err)?;
    decode_png(&data)
}

fn decode_png(data_uri: &str) -> Result<SharedPixelBuffer<Rgba8Pixel>, String> {
    let b64 = data_uri.split_once(',').map(|(_, d)| d).unwrap_or(data_uri);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("capture illisible : {e}"))?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| format!("capture illisible : {e}"))?
        .to_rgba8();
    Ok(SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height()))
}

async fn sources(c: &Client) -> Result<Vec<SourceInfo>, String> {
    let mut out = Vec::new();
    for input in c.inputs().list(Some("browser_source")).await.map_err(err)? {
        if input.id.name == PREVIEW_INPUT {
            continue;
        }
        let s = c.inputs().settings::<serde_json::Value>(InputId::Name(&input.id.name)).await.map_err(err)?.settings;
        out.push(SourceInfo {
            url: s["url"].as_str().unwrap_or("").to_string(),
            width: s["width"].as_u64().unwrap_or(800) as u32,
            height: s["height"].as_u64().unwrap_or(600) as u32,
            name: input.id.name,
        });
    }
    Ok(out)
}

async fn refresh(c: &Client, files: &[String]) -> Result<usize, String> {
    let mut n = 0;
    for s in sources(c).await? {
        if files.iter().any(|f| s.file().eq_ignore_ascii_case(f)) {
            c.inputs().press_properties_button(InputId::Name(&s.name), "refreshnocache").await.map_err(err)?;
            n += 1;
        }
    }
    Ok(n)
}

async fn use_studio(c: &Client, input: &str, studio_url: &str) -> Result<(), String> {
    let current = c.inputs().settings::<serde_json::Value>(InputId::Name(input)).await.map_err(err)?.settings;
    let url = current["url"].as_str().unwrap_or("");
    // On garde les options (?min=5, ?test=1…) de la source.
    let query = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
    let settings = serde_json::json!({ "url": format!("{studio_url}{query}"), "is_local_file": false });
    c.inputs()
        .set_settings(SetSettings { input: InputId::Name(input), settings: &settings, overlay: Some(true) })
        .await
        .map_err(err)
}

async fn set_size(c: &Client, file: &str, width: u32, height: u32) -> Result<usize, String> {
    let settings = serde_json::json!({ "width": width, "height": height });
    let mut n = 0;
    for s in sources(c).await? {
        if s.file().eq_ignore_ascii_case(file) {
            c.inputs()
                .set_settings(SetSettings { input: InputId::Name(&s.name), settings: &settings, overlay: Some(true) })
                .await
                .map_err(err)?;
            n += 1;
        }
    }
    Ok(n)
}
