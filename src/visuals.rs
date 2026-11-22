//! Apparence « de base » d'une touche, à partir de sa config seule (sans état OBS).
//! Partagé par le démon (qui ajoute ensuite l'état OBS) et l'éditeur (aperçus).

use crate::{
    app::format_duration,
    clock::LocalTime,
    config::{Action, KeyConfig, SideWidget, parse_color},
    coil::CoilView,
    media::NowPlaying,
    plugins::countdown,
    render::{Dial, KeyVisual, TextLine},
};

// Palette commune.
pub const DEFAULT_BG: [u8; 3] = [34, 38, 49];
pub const ACTIVE_RED: [u8; 3] = [198, 40, 40];
pub const PAUSED_AMBER: [u8; 3] = [184, 120, 0];
pub const SCENE_BG: [u8; 3] = [31, 63, 122];
pub const BACK_BG: [u8; 3] = [60, 64, 74];

/// Couleurs proposées dans l'éditeur.
pub const SWATCHES: [&str; 10] = [
    "#222631", "#3c404a", "#1f3f7a", "#12636b", "#1f5f3a", "#7a5a12", "#b87800", "#7a1f2b", "#c62828", "#5a2a7a",
];

/// Apparence d'une touche configurée, hors état OBS.
pub fn base_visual(key: &KeyConfig) -> KeyVisual {
    let custom_bg = key.color.as_deref().and_then(parse_color);
    let default_bg = match key.action {
        Action::ObsScene { .. } => SCENE_BG,
        _ => DEFAULT_BG,
    };
    KeyVisual {
        bg: custom_bg.unwrap_or(default_bg),
        icon: key.icon.clone(),
        label: key.label.clone(),
        big_text: matches!(key.action, Action::StreamTimer).then(|| "--:--".to_string()),
        highlight: false,
        dim: false,
        lines: Vec::new(),
        dial: None,
    }
}

/// Touche « compte à rebours » : au repos, l'apparence choisie dans l'éditeur ; en marche, un anneau
/// qui se vide et le temps restant (rouge dans les 10 dernières secondes) ; à la fin, « FINI ».
pub fn countdown_visual(idle: KeyVisual, view: countdown::View, seconds: u32) -> KeyVisual {
    const WHITE: [u8; 3] = [255, 255, 255];
    const GREY: [u8; 3] = [150, 165, 195];
    const RING: [u8; 3] = [58, 116, 255];
    const RING_LATE: [u8; 3] = [255, 92, 80];
    match view {
        countdown::View::Idle => {
            let mut v = idle;
            if v.label.is_none() {
                v.label = Some(countdown::label(seconds));
            }
            v
        }
        countdown::View::Running { left, progress } => {
            let late = left <= 10;
            let text = format_duration(left);
            let size = if text.len() > 5 { 17 } else { 24 };
            KeyVisual {
                bg: if late { [52, 16, 20] } else { [12, 20, 40] },
                dial: Some(Dial {
                    lit: ((1.0 - progress) * 60.0).round() as u8,
                    color: if late { RING_LATE } else { RING },
                    embers: 0,
                    phase: 0,
                }),
                lines: vec![TextLine::new(text, size, WHITE, 1), TextLine::new("restant", 10, GREY, 1)],
                ..KeyVisual::blank()
            }
        }
        countdown::View::Done => KeyVisual {
            bg: ACTIVE_RED,
            lines: vec![TextLine::new("FINI", 20, WHITE, 1), TextLine::new(countdown::label(seconds), 10, [255, 210, 205], 1)],
            ..KeyVisual::blank()
        },
    }
}

/// Touche Retour des dossiers.
pub fn back_visual() -> KeyVisual {
    KeyVisual { bg: BACK_BG, icon: Some("nav/back.svg".into()), label: Some("Retour".into()), ..Default::default() }
}

/// Ce qu'il faut savoir pour dessiner l'écran latéral (état réel dans le démon, exemple dans l'éditeur).
pub struct SideState<'a> {
    pub music: Option<&'a NowPlaying>,
    /// Durée du live en secondes (`None` = hors live).
    pub stream_elapsed: Option<u64>,
    pub scene: Option<&'a str>,
    pub obs_connected: bool,
    pub time: LocalTime,
    pub coil: CoilView,
}

/// Libellé d'un contenu de l'écran latéral (éditeur).
pub fn side_label(widget: SideWidget) -> &'static str {
    match widget {
        SideWidget::Music => "Musique en cours",
        SideWidget::Clock => "Heure et date",
        SideWidget::Logo => "Logo Blackat",
        SideWidget::StreamTimer => "Chrono du live",
        SideWidget::Coil => "Coil (pause vape)",
        SideWidget::Scene => "Scène OBS active",
        SideWidget::None => "Vide",
    }
}

/// Case de l'écran latéral.
pub fn side_visual(widget: SideWidget, s: &SideState) -> KeyVisual {
    const BG: [u8; 3] = [10, 14, 26];
    const WHITE: [u8; 3] = [255, 255, 255];
    const GREY: [u8; 3] = [140, 155, 185];
    let mut v = KeyVisual { bg: BG, ..KeyVisual::blank() };
    match widget {
        SideWidget::None => return KeyVisual::blank(),
        SideWidget::Logo => {
            v.bg = [5, 7, 13];
            v.icon = Some("misc/blackat.svg".into());
        }
        SideWidget::Clock => {
            v.lines = vec![TextLine::new(s.time.hhmm(), 25, WHITE, 1), TextLine::new(s.time.short_date(), 11, GREY, 1)];
        }
        SideWidget::Music => match s.music {
            Some(m) => {
                v.icon = Some(if m.playing { "misc/music.svg" } else { "misc/play-pause.svg" }.into());
                v.lines.push(TextLine::new(m.title.clone(), 12, if m.playing { WHITE } else { GREY }, 2));
                if !m.artist.trim().is_empty() {
                    v.lines.push(TextLine::new(m.artist.clone(), 10, GREY, 1));
                }
            }
            None => {
                v.icon = Some("misc/music.svg".into());
                v.lines.push(TextLine::new("Pas de musique", 11, GREY, 2));
            }
        },
        SideWidget::StreamTimer => match s.stream_elapsed {
            Some(secs) => {
                v.bg = ACTIVE_RED;
                v.lines = vec![TextLine::new("EN LIVE", 11, WHITE, 1), TextLine::new(format_duration(secs), 20, WHITE, 1)];
            }
            None => {
                v.icon = Some("misc/timer.svg".into());
                v.lines.push(TextLine::new("Hors live", 11, GREY, 1));
            }
        },
        SideWidget::Coil => return coil_visual(&s.coil, None),
        SideWidget::Scene => {
            v.icon = Some("obs/scene.svg".into());
            match (s.scene, s.obs_connected) {
                (Some(scene), true) => v.lines.push(TextLine::new(scene, 12, WHITE, 2)),
                _ => {
                    v.lines.push(TextLine::new("OBS fermé", 11, GREY, 1));
                    v.dim = true;
                }
            }
        }
    }
    v
}

// ── Coil ────────────────────────────────────────────────────────────────────

/// Résistance chaude juste après une taffe (orange) → refroidie (cyan), comme dans l'appli.
pub const COIL_HOT: [u8; 3] = [255, 122, 26];
pub const COIL_COOL: [u8; 3] = [63, 216, 255];
/// Étape intermédiaire du refroidissement (évite le vert-gris d'un mélange direct orange/cyan).
const COIL_MID: [u8; 3] = [176, 92, 255];
const COIL_BG: [u8; 3] = [8, 11, 20];

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Coil : cadran de la pause. `idle` = apparence au repos (touche : icône + libellé ;
/// `None` = écran latéral : cadran « PRÊT » et taffes du jour).
pub fn coil_visual(c: &CoilView, idle: Option<(&str, Option<&str>)>) -> KeyVisual {
    const WHITE: [u8; 3] = [255, 255, 255];
    const GREY: [u8; 3] = [140, 155, 185];
    let mut v = KeyVisual { bg: COIL_BG, ..KeyVisual::blank() };
    let taffes = |n: u32| if n > 1 { format!("{n} taffes") } else { format!("{n} taffe") };
    match c.remaining {
        Some(left) => {
            let color = if c.heat > 0.5 {
                mix(COIL_MID, COIL_HOT, (c.heat - 0.5) * 2.0)
            } else {
                mix(COIL_COOL, COIL_MID, c.heat * 2.0)
            };
            v.dial = Some(Dial {
                lit: (c.progress * 60.0).round() as u8,
                color,
                embers: if c.heat > 0.35 { (c.heat * 255.0) as u8 } else { 0 },
                phase: (c.phase % 251) as u8,
            });
            if c.waiting {
                v.lines = vec![TextLine::new("ATTENDS", 12, [255, 110, 100], 1), TextLine::new(format!("{left}"), 22, WHITE, 1)];
            } else {
                v.lines = vec![TextLine::new(format!("{left}"), 27, WHITE, 1), TextLine::new("pause", 10, GREY, 1)];
            }
        }
        None => match idle {
            Some((label, icon)) if !c.ready => {
                v.bg = [20, 26, 40];
                v.icon = Some(icon.unwrap_or("misc/coil.svg").to_string());
                v.label = Some(label.to_string());
            }
            _ => {
                if c.ready {
                    v.bg = [10, 38, 50];
                }
                v.dial = Some(Dial { lit: 60, color: COIL_COOL, embers: 0, phase: 0 });
                v.lines = vec![TextLine::new("PRÊT", 15, if c.ready { WHITE } else { COIL_COOL }, 1), TextLine::new(taffes(c.today), 10, GREY, 1)];
            }
        },
    }
    v
}
