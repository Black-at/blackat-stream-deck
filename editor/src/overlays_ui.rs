//! Onglet Overlays de l'éditeur : relie l'interface (global Slint `OV`), les réglages
//! (`soomfon_deck::overlays`) et OBS (`overlay_obs`).
//!
//! Chaque réglage modifié est enregistré dans `shared/studio.js` (après une courte pause),
//! puis l'aperçu OBS est relancé et, si l'option est active, les sources OBS qui affichent
//! la version Studio sont rafraîchies.

use std::{
    cell::RefCell,
    collections::VecDeque,
    net::{SocketAddr, TcpStream},
    os::windows::process::CommandExt,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use slint::{Color, ComponentHandle, Image, ModelRc, SharedString, Timer, TimerMode, VecModel};
use soomfon_deck::{
    config::{ObsConfig, parse_color},
    overlays::{
        self, Item, Studio,
        schema::{self, Field, Group, Kind, Slot},
    },
};

use crate::{
    FieldRow, MainWindow, OV, OverlayRow,
    overlay_obs::{Cmd, Msg, ObsWorker, SourceInfo},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RESOLUTIONS: [(&str, u32, u32); 4] =
    [("1920 × 1080", 1920, 1080), ("2560 × 1440", 2560, 1440), ("3840 × 2160", 3840, 2160), ("Personnalisée", 0, 0)];

struct OvState {
    dir: PathBuf,
    studio_ok: bool,
    studio: Studio,
    items: Vec<Item>,
    /// None = thème commun.
    sel: Option<usize>,
    /// Dernier overlay choisi (sert d'aperçu quand on règle le thème commun).
    last_overlay: usize,
    group: usize,
    res: (u32, u32),
    sources: Vec<SourceInfo>,
    obs_ok: Option<bool>,
    /// Réglages affichés (dans le groupe courant), pour retrouver un champ par son index.
    shown: Vec<&'static Field>,
}

impl OvState {
    fn item(&self) -> Option<&Item> {
        self.sel.and_then(|i| self.items.get(i))
    }

    fn preview_item(&self) -> Option<&Item> {
        self.item().or_else(|| self.items.get(self.last_overlay)).or_else(|| self.items.first())
    }

    fn fields(&self) -> &'static [Field] {
        match self.item() {
            None => schema::GLOBAL,
            Some(item) => schema::fields_for(&item.section),
        }
    }

    fn slot(&mut self) -> Slot<'_> {
        match self.sel.and_then(|i| self.items.get(i)).map(|it| it.id.clone()) {
            None => (&mut self.studio.global).into(),
            Some(id) => self.studio.entry_mut(&id).into(),
        }
    }

    fn reload_items(&mut self) {
        self.items = overlays::list(&self.dir, &self.studio);
    }

    fn url(&self, item: &Item) -> String {
        overlays::file_url(&item.file)
    }

    /// URL de la version d'origine (dossier `overlays` à côté du dossier Studio).
    fn original_url(&self, item: &Item) -> Option<String> {
        let original = self.dir.parent()?.join("overlays").join(format!("{}.html", item.id));
        Some(overlays::file_url(&original))
    }
}

#[derive(Clone)]
pub struct OvCtx {
    st: Rc<RefCell<OvState>>,
    ui: slint::Weak<MainWindow>,
    obs: Rc<ObsWorker>,
    inbox: Arc<Mutex<VecDeque<Msg>>>,
    save_timer: Rc<Timer>,
    preview_timer: Rc<Timer>,
}

impl OvCtx {
    fn with(&self, f: impl FnOnce(&MainWindow, &mut OvState)) {
        if let Some(ui) = self.ui.upgrade() {
            f(&ui, &mut self.st.borrow_mut());
        }
    }

    /// Fermeture de l'éditeur : enregistre un réglage en attente, puis remet OBS comme avant
    /// (mode studio, scène d'aperçu supprimée). Attend au plus 3 s.
    pub fn shutdown(&self) {
        if self.save_timer.running() {
            self.save_timer.stop();
            let st = self.st.borrow();
            let _ = st.studio.save(&st.dir);
        }
        self.obs.cleanup_blocking(Duration::from_secs(3));
    }
}

fn status(ui: &MainWindow, text: &str, error: bool) {
    ui.set_status(text.into());
    ui.set_status_error(error);
}

fn strings(v: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.iter().map(SharedString::from).collect::<Vec<_>>()))
}

// ── Affichage ─────────────────────────────────────────────────────────────────

fn refresh_all(ui: &MainWindow, st: &mut OvState) {
    let ov = ui.global::<OV>();
    ov.set_studio_ok(st.studio_ok);
    ov.set_studio_dir(format!("Attendu : {}", st.dir.display()).into());
    let rows: Vec<OverlayRow> = st
        .items
        .iter()
        .map(|i| OverlayRow { name: i.name.as_str().into(), file: format!("{}.html", i.id).into(), enabled: i.enabled })
        .collect();
    ov.set_items(ModelRc::new(VecModel::from(rows)));
    ov.set_selected(st.sel.map(|i| i as i32).unwrap_or(-1));
    match st.item() {
        None => {
            ov.set_title("Thème commun".into());
            ov.set_subtitle(format!("S'applique aux {} overlays · aperçu : {}", st.items.len(),
                st.preview_item().map(|i| i.name.as_str()).unwrap_or("—")).into());
            ov.set_enabled(true);
            ov.set_name_edit("".into());
        }
        Some(item) => {
            ov.set_title(item.name.as_str().into());
            ov.set_subtitle(format!("{}.html", item.id).into());
            ov.set_enabled(item.enabled);
            ov.set_name_edit(item.name.as_str().into());
        }
    }
    let groups: Vec<String> = Group::ALL.iter().map(|g| g.label().to_string()).collect();
    ov.set_groups(strings(&groups));
    ov.set_group(st.group as i32);
    ov.set_res_options(strings(&RESOLUTIONS.iter().map(|(l, _, _)| l.to_string()).collect::<Vec<_>>()));
    refresh_fields(ui, st);
    refresh_checks(ui, st);
}

fn refresh_fields(ui: &MainWindow, st: &mut OvState) {
    let group = Group::ALL[st.group.min(Group::ALL.len() - 1)];
    st.shown = st.fields().iter().filter(|f| f.group == group).collect();
    let shown = st.shown.clone();
    let slot = st.slot();
    let rows: Vec<FieldRow> = shown
        .iter()
        .map(|field| {
            let (value, modified) = slot.get(field);
            let number = value.parse::<f32>().unwrap_or(0.0);
            let (kind, min, max, step, unit) = match field.kind {
                Kind::Color => ("color", 0.0, 0.0, 0.0, ""),
                Kind::Px(a, b, s) => ("slider", a, b, s, "px"),
                Kind::Ms(a, b, s) => ("slider", a, b, s, "ms"),
                Kind::Number(a, b, s) => ("slider", a, b, s, ""),
                Kind::Percent(a, b) => ("slider", a, b, 1.0, "%"),
                Kind::Shadow(m) | Kind::Blur(m) => ("slider", 0.0, m, 1.0, "px"),
                Kind::Text | Kind::Lines => ("text", 0.0, 0.0, 0.0, ""),
                Kind::Select(_) | Kind::Font(_) => ("select", 0.0, 0.0, 0.0, ""),
                Kind::Toggle => ("toggle", 0.0, 0.0, 0.0, ""),
            };
            let options = schema::option_labels(field.kind);
            let option_index = schema::option_index(field.kind, &value).map(|i| i as i32).unwrap_or(-1);
            let swatch = parse_color(&value).map(|[r, g, b]| Color::from_rgb_u8(r, g, b)).unwrap_or_default();
            FieldRow {
                kind: kind.into(),
                label: field.label.into(),
                value: value.as_str().into(),
                number,
                min,
                max,
                step,
                unit: unit.into(),
                options: strings(&options),
                option_index,
                modified,
                swatch,
            }
        })
        .collect();
    ui.global::<OV>().set_fields(ModelRc::new(VecModel::from(rows)));
}

fn streamerbot_reachable() -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], 8080)), Duration::from_millis(150)).is_ok()
}

fn refresh_checks(ui: &MainWindow, st: &mut OvState) {
    let ov = ui.global::<OV>();
    let Some(item) = st.item().cloned() else {
        ov.set_url("".into());
        ov.set_checks(
            "Le thème commun s'applique à tous les overlays de la copie Studio. Choisis un overlay pour voir ses sources OBS."
                .into(),
        );
        ov.set_can_use_studio(false);
        return;
    };
    let url = st.url(&item);
    ov.set_url(url.as_str().into());

    let mut lines = Vec::new();
    lines.push(if item.file.exists() {
        format!("• Fichier : {}.html présent", item.id)
    } else {
        format!("• Fichier : {}.html INTROUVABLE", item.id)
    });

    let original = st.original_url(&item).unwrap_or_default();
    let mut candidate: Option<String> = None;
    match st.obs_ok {
        Some(false) => lines.push("• OBS : fermé (ouvre OBS pour l'aperçu et les sources)".into()),
        None => lines.push("• OBS : connexion…".into()),
        Some(true) => {
            let mut any = false;
            for s in &st.sources {
                if s.file().eq_ignore_ascii_case(&url) {
                    lines.push(format!("• Source « {} » : version Studio ({} × {})", s.name, s.width, s.height));
                    any = true;
                } else if s.file().eq_ignore_ascii_case(&original) {
                    lines.push(format!("• Source « {} » : version d'origine ({} × {}) — non modifiée par le Studio", s.name, s.width, s.height));
                    candidate.get_or_insert(s.name.clone());
                    any = true;
                }
            }
            if !any {
                lines.push("• Aucune source OBS n'affiche cet overlay (ajoute une source Navigateur avec l'URL ci-dessous)".into());
            }
            lines.push(
                "• Aperçu : OBS est en mode studio pendant l'édition — n'appuie pas sur « Transition » (l'aperçu passerait à l'antenne)"
                    .into(),
            );
        }
    }
    if ["alerts", "gameplay", "justchatting", "ending"].contains(&item.section.as_str()) {
        lines.push(if streamerbot_reachable() {
            "• Streamer.bot : joignable (port 8080)".into()
        } else {
            "• Streamer.bot : fermé (les alertes réelles n'arriveront pas)".into()
        });
    }
    ov.set_checks(lines.join("\n").into());
    ov.set_can_use_studio(candidate.is_some());
    if let Some(name) = candidate {
        ov.set_use_studio_label(format!("Utiliser la version Studio dans « {name} »").into());
    }
}

// ── Actions ───────────────────────────────────────────────────────────────────

/// Options d'URL utiles à l'aperçu (alertes et soutiens fictifs).
fn preview_query(section: &str) -> &'static str {
    match section {
        "alerts" | "ending" | "justchatting" | "gameplay" => "?test=1",
        _ => "",
    }
}

fn request_preview(ctx: &OvCtx, delay: Duration) {
    let c = ctx.clone();
    ctx.preview_timer.start(TimerMode::SingleShot, delay, move || {
        c.with(|ui, st| {
            let Some(item) = st.preview_item().cloned() else { return };
            let ov = ui.global::<OV>();
            ov.set_preview_busy(true);
            ov.set_preview_status("Rendu dans OBS…".into());
            c.obs.send(Cmd::Preview {
                url: format!("{}{}", st.url(&item), preview_query(&item.section)),
                width: st.res.0,
                height: st.res.1,
            });
        });
    });
}

/// Enregistre (après une courte pause), puis aperçu + rafraîchissement OBS éventuel.
fn schedule_save(ctx: &OvCtx) {
    let c = ctx.clone();
    ctx.save_timer.start(TimerMode::SingleShot, Duration::from_millis(350), move || {
        let mut files = Vec::new();
        let mut auto = false;
        c.with(|ui, st| {
            match st.studio.save(&st.dir) {
                Ok(()) => status(ui, "Overlay enregistré", false),
                Err(e) => {
                    status(ui, &format!("Non enregistré : {e}"), true);
                    return;
                }
            }
            auto = ui.global::<OV>().get_auto_refresh();
            files = match st.item() {
                Some(item) => vec![st.url(item)],
                None => st.items.iter().map(|i| st.url(i)).collect(),
            };
        });
        if auto && !files.is_empty() {
            c.obs.send(Cmd::Refresh { files });
        }
        request_preview(&c, Duration::from_millis(50));
    });
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    let Ok(mut child) = std::process::Command::new("clip")
        .stdin(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    else {
        return false;
    };
    // clip.exe lit l'UTF-16 avec BOM : les accents passent correctement.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
    let ok = child.stdin.take().map(|mut s| s.write_all(&bytes).is_ok()).unwrap_or(false);
    ok && child.wait().map(|s| s.success()).unwrap_or(false)
}

fn drain_obs(ui: &MainWindow, st: &mut OvState, msgs: Vec<Msg>) {
    let ov = ui.global::<OV>();
    for m in msgs {
        match m {
            Msg::Connected(Ok(())) => st.obs_ok = Some(true),
            Msg::Connected(Err(e)) => {
                st.obs_ok = Some(false);
                ov.set_preview_busy(false);
                ov.set_preview_status(format!("OBS est fermé ou injoignable — ouvre OBS puis « Actualiser l'aperçu ».\n({e})").into());
            }
            Msg::Preview(Ok(buffer)) => {
                ov.set_preview(Image::from_rgba8(buffer));
                ov.set_has_preview(true);
                ov.set_preview_busy(false);
                ov.set_preview_status("".into());
            }
            Msg::Preview(Err(e)) => {
                ov.set_preview_busy(false);
                ov.set_preview_status(format!("Aperçu impossible : {e}").into());
            }
            Msg::Sources(list) => st.sources = list,
            Msg::Info(text) => status(ui, &text, false),
            Msg::Error(e) => {
                st.obs_ok = Some(false);
                ov.set_preview_busy(false);
                status(ui, &format!("OBS : {e}"), true);
            }
        }
    }
    refresh_checks(ui, st);
}

// ── Mise en place ─────────────────────────────────────────────────────────────

pub fn init(ui: &MainWindow, obs_settings: ObsConfig) -> OvCtx {
    let dir = overlays::default_dir();
    let studio_ok = dir.join("shared").join("studio-apply.js").exists();
    let (studio, load_error) = match Studio::load(&dir) {
        Ok(s) => (s, None),
        Err(e) => (Studio::default(), Some(e)),
    };
    let mut st = OvState {
        items: overlays::list(&dir, &studio),
        dir,
        studio_ok,
        studio,
        sel: Some(0),
        last_overlay: 0,
        group: 0,
        res: (1920, 1080),
        sources: Vec::new(),
        obs_ok: None,
        shown: Vec::new(),
    };
    if st.items.is_empty() {
        st.sel = None;
    }

    let inbox: Arc<Mutex<VecDeque<Msg>>> = Arc::new(Mutex::new(VecDeque::new()));
    let weak = ui.as_weak();
    let obs = ObsWorker::spawn(obs_settings, inbox.clone(), move || {
        let _ = weak.upgrade_in_event_loop(|ui| ui.global::<OV>().invoke_obs_event());
    });

    let ctx = OvCtx {
        st: Rc::new(RefCell::new(st)),
        ui: ui.as_weak(),
        obs: Rc::new(obs),
        inbox,
        save_timer: Rc::new(Timer::default()),
        preview_timer: Rc::new(Timer::default()),
    };
    {
        let ov = ui.global::<OV>();
        ov.set_preview_status("Ouvre cet onglet avec OBS lancé pour voir l'aperçu.".into());
        ov.set_res_index(0);
        refresh_all(ui, &mut ctx.st.borrow_mut());
        if let Some(e) = load_error {
            status(ui, &format!("Réglages Studio illisibles, valeurs d'origine utilisées : {e}"), true);
        }
    }
    install(ui, &ctx);
    ctx
}

fn install(ui: &MainWindow, ctx: &OvCtx) {
    let ov = ui.global::<OV>();

    // Arrivée sur l'onglet : liste des sources + aperçu.
    let c = ctx.clone();
    ui.on_section_changed(move |section| {
        if section == 1 {
            c.obs.send(Cmd::Sources);
            request_preview(&c, Duration::from_millis(10));
        } else {
            // On quitte l'onglet : OBS revient comme avant, la scène d'aperçu disparaît.
            c.preview_timer.stop();
            c.obs.send(Cmd::Cleanup { done: None });
            if let Some(ui) = c.ui.upgrade() {
                let ov = ui.global::<OV>();
                ov.set_has_preview(false);
                ov.set_preview_status("Aperçu en pause (onglet quitté).".into());
            }
        }
    });

    let c = ctx.clone();
    ov.on_obs_event(move || {
        let msgs: Vec<Msg> = c.inbox.lock().unwrap().drain(..).collect();
        c.with(|ui, st| drain_obs(ui, st, msgs));
    });

    // Bibliothèque
    let c = ctx.clone();
    ov.on_select(move |i| {
        c.with(|ui, st| {
            st.sel = (i >= 0).then_some(i as usize).filter(|&i| i < st.items.len());
            if let Some(i) = st.sel {
                st.last_overlay = i;
            }
            st.group = 0;
            ui.global::<OV>().set_confirm_delete(false);
            refresh_all(ui, st);
        });
        request_preview(&c, Duration::from_millis(10));
    });

    let c = ctx.clone();
    ov.on_group_chosen(move |g| {
        c.with(|ui, st| {
            st.group = g.max(0) as usize;
            ui.global::<OV>().set_group(g);
            refresh_fields(ui, st);
        })
    });

    // Réglages
    let c = ctx.clone();
    ov.on_field_text(move |i, text| {
        let mut saved = false;
        c.with(|ui, st| {
            let Some(field) = st.shown.get(i as usize).copied() else { return };
            if field.kind == Kind::Color {
                let t = text.trim();
                if parse_color(t).is_none() {
                    status(ui, "Couleur invalide : format #rrggbb, ex. #E8855E", true);
                    return;
                }
                st.slot().set(field, &t.to_uppercase());
                refresh_fields(ui, st);
            } else {
                // Pas de rafraîchissement de la liste : on garde le curseur dans le champ.
                st.slot().set(field, text.as_str());
            }
            saved = true;
        });
        if saved {
            schedule_save(&c);
        }
    });

    let c = ctx.clone();
    ov.on_field_number(move |i, v| {
        c.with(|ui, st| {
            let Some(field) = st.shown.get(i as usize).copied() else { return };
            st.slot().set(field, &format!("{}", v.round()));
            refresh_fields(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_field_option(move |i, o| {
        c.with(|ui, st| {
            let Some(field) = st.shown.get(i as usize).copied() else { return };
            if let Some(value) = schema::option_value(field.kind, o.max(0) as usize) {
                st.slot().set(field, value);
            }
            refresh_fields(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_field_toggle(move |i, on| {
        c.with(|ui, st| {
            let Some(field) = st.shown.get(i as usize).copied() else { return };
            st.slot().set(field, if on { "true" } else { "false" });
            refresh_fields(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_field_reset(move |i| {
        c.with(|ui, st| {
            let Some(field) = st.shown.get(i as usize).copied() else { return };
            st.slot().reset(field);
            refresh_fields(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_reset_all(move || {
        c.with(|ui, st| {
            st.slot().reset_all();
            refresh_fields(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_toggle_enabled(move |on| {
        c.with(|ui, st| {
            let Some(id) = st.item().map(|i| i.id.clone()) else { return };
            st.studio.entry_mut(&id).enabled = on;
            st.reload_items();
            refresh_all(ui, st);
        });
        schedule_save(&c);
    });

    let c = ctx.clone();
    ov.on_rename(move |name| {
        let mut ok = false;
        c.with(|ui, st| {
            let Some(id) = st.item().map(|i| i.id.clone()) else { return };
            let name = name.trim();
            if name.is_empty() {
                status(ui, "Le nom ne peut pas être vide", true);
                return;
            }
            st.studio.entry_mut(&id).name = name.to_string();
            st.reload_items();
            refresh_all(ui, st);
            ok = true;
        });
        if ok {
            schedule_save(&c);
        }
    });

    let c = ctx.clone();
    ov.on_duplicate(move || {
        let mut ok = false;
        c.with(|ui, st| {
            let Some(id) = st.item().map(|i| i.id.clone()) else { return };
            match overlays::duplicate(&st.dir, &mut st.studio, &id) {
                Ok(new_id) => {
                    st.reload_items();
                    st.sel = st.items.iter().position(|i| i.id == new_id);
                    refresh_all(ui, st);
                    status(ui, &format!("Copie créée : {new_id}.html"), false);
                    ok = true;
                }
                Err(e) => status(ui, &format!("Duplication impossible : {e}"), true),
            }
        });
        if ok {
            schedule_save(&c);
        }
    });

    let c = ctx.clone();
    ov.on_delete_ask(move || {
        if let Some(ui) = c.ui.upgrade() {
            ui.global::<OV>().set_confirm_delete(true);
        }
    });

    let c = ctx.clone();
    ov.on_delete_confirm(move || {
        let mut ok = false;
        c.with(|ui, st| {
            ui.global::<OV>().set_confirm_delete(false);
            let Some(item) = st.item().cloned() else { return };
            match overlays::delete(&st.dir, &mut st.studio, &item.id) {
                Ok(()) => {
                    st.reload_items();
                    st.sel = None;
                    st.last_overlay = 0;
                    refresh_all(ui, st);
                    status(ui, &format!("« {} » supprimé de la copie Studio", item.name), false);
                    ok = true;
                }
                Err(e) => status(ui, &format!("Suppression impossible : {e}"), true),
            }
        });
        if ok {
            schedule_save(&c);
        }
    });

    let c = ctx.clone();
    ov.on_import(move || {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Importer un overlay (fichier .html)")
            .add_filter("Overlay HTML", &["html", "htm"])
            .pick_file()
        else {
            return;
        };
        let mut ok = false;
        c.with(|ui, st| match overlays::import(&st.dir, &mut st.studio, &file) {
            Ok(id) => {
                st.reload_items();
                st.sel = st.items.iter().position(|i| i.id == id);
                refresh_all(ui, st);
                status(ui, &format!("Overlay importé : {id}.html"), false);
                ok = true;
            }
            Err(e) => status(ui, &format!("Import impossible : {e}"), true),
        });
        if ok {
            schedule_save(&c);
        }
    });

    let c = ctx.clone();
    ov.on_export(move || {
        let Some(dest) = rfd::FileDialog::new().set_title("Exporter l'overlay dans…").pick_folder() else { return };
        c.with(|ui, st| {
            let Some(id) = st.item().map(|i| i.id.clone()) else { return };
            match overlays::export(&st.dir, &st.studio, &id, &dest) {
                Ok(out) => {
                    status(ui, &format!("Exporté dans {}", out.display()), false);
                    let _ = std::process::Command::new("explorer").arg(&out).spawn();
                }
                Err(e) => status(ui, &format!("Export impossible : {e}"), true),
            }
        });
    });

    let c = ctx.clone();
    ov.on_open_folder(move || {
        c.with(|_, st| {
            let _ = std::process::Command::new("explorer").arg(&st.dir).spawn();
        })
    });

    // Aperçu
    let c = ctx.clone();
    ov.on_refresh_preview(move || {
        c.obs.send(Cmd::Sources);
        request_preview(&c, Duration::from_millis(10));
    });

    let c = ctx.clone();
    ov.on_res_chosen(move |i| {
        let mut changed = false;
        c.with(|ui, st| {
            if let Some((_, w, h)) = RESOLUTIONS.get(i.max(0) as usize) {
                if *w > 0 {
                    st.res = (*w, *h);
                    changed = true;
                } else {
                    let ov = ui.global::<OV>();
                    ov.set_custom_w(st.res.0.to_string().into());
                    ov.set_custom_h(st.res.1.to_string().into());
                }
            }
        });
        if changed {
            request_preview(&c, Duration::from_millis(10));
        }
    });

    let c = ctx.clone();
    ov.on_custom_size(move |w, h| {
        let mut ok = false;
        c.with(|ui, st| {
            match (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
                (Ok(w), Ok(h)) if (320..=7680).contains(&w) && (240..=4320).contains(&h) => {
                    st.res = (w, h);
                    ok = true;
                }
                _ => status(ui, "Dimensions invalides (largeur 320–7680, hauteur 240–4320)", true),
            }
        });
        if ok {
            request_preview(&c, Duration::from_millis(10));
        }
    });

    // OBS
    let c = ctx.clone();
    ov.on_copy_url(move || {
        c.with(|ui, st| {
            let Some(item) = st.item() else { return };
            let url = st.url(item);
            if copy_to_clipboard(&url) {
                status(ui, "URL copiée — colle-la dans une source Navigateur d'OBS (champ URL)", false);
            } else {
                status(ui, "Copie impossible", true);
            }
        })
    });

    let c = ctx.clone();
    ov.on_refresh_obs(move || {
        let files: Vec<String> = {
            let st = c.st.borrow();
            match st.item() {
                Some(item) => vec![st.url(item)],
                None => st.items.iter().map(|i| st.url(i)).collect(),
            }
        };
        c.obs.send(Cmd::Refresh { files });
    });

    let c = ctx.clone();
    ov.on_use_studio(move || {
        let cmd = {
            let st = c.st.borrow();
            let Some(item) = st.item() else { return };
            let original = st.original_url(item).unwrap_or_default();
            let Some(source) = st.sources.iter().find(|s| s.file().eq_ignore_ascii_case(&original)) else { return };
            Cmd::UseStudio { input: source.name.clone(), studio_url: st.url(item) }
        };
        c.obs.send(cmd);
    });

    let c = ctx.clone();
    ov.on_apply_size(move || {
        let cmd = {
            let st = c.st.borrow();
            let Some(item) = st.item() else { return };
            Cmd::SetSize { file: st.url(item), width: st.res.0, height: st.res.1 }
        };
        c.obs.send(cmd);
    });
}
