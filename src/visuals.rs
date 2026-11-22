//! Apparence « de base » d'une touche, à partir de sa config seule (sans état OBS).
//! Partagé par le démon (qui ajoute ensuite l'état OBS) et l'éditeur (aperçus).

use crate::{
    app::format_duration,
    clock::LocalTime,
    config::{Action, KeyConfig, SideWidget, parse_color},
    media::NowPlaying,
    plugins::{countdown, system::Stats},
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
            let size = if text.len() > 5 { 15 } else { 21 };
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
    /// Charge du PC (`None` = pas encore mesurée).
    pub system: Option<Stats>,
    /// Micro Windows coupé (`None` = inconnu / pas de micro).
    pub mic_muted: Option<bool>,
    pub record_active: bool,
    pub record_paused: bool,
}

/// Libellé d'un contenu de l'écran latéral (éditeur).
pub fn side_label(widget: SideWidget) -> &'static str {
    match widget {
        SideWidget::Music => "Musique en cours",
        SideWidget::Clock => "Heure et date",
        SideWidget::Logo => "Logo Blackat",
        SideWidget::StreamTimer => "Chrono du live",
        SideWidget::System => "Charge du PC (CPU, RAM)",
        SideWidget::ObsStatus => "État OBS (REC, LIVE)",
        SideWidget::Mic => "Micro Windows (coupé ?)",
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
        SideWidget::System => return system_visual(s.system, [10, 14, 26]),
        SideWidget::Mic => return mic_visual(s.mic_muted, None),
        SideWidget::ObsStatus => {
            if !s.obs_connected {
                v.icon = Some("obs/scene.svg".into());
                v.lines.push(TextLine::new("OBS fermé", 11, GREY, 1));
                v.dim = true;
            } else {
                let dot = |on: bool| if on { "●" } else { "○" };
                let rec_color = if s.record_paused { PAUSED_AMBER } else if s.record_active { [255, 90, 90] } else { GREY };
                let live_color = if s.stream_elapsed.is_some() { [255, 90, 90] } else { GREY };
                if s.record_active || s.stream_elapsed.is_some() {
                    v.bg = [40, 10, 14];
                }
                let rec = if s.record_paused { "PAUSE REC".to_string() } else { format!("{} REC", dot(s.record_active)) };
                v.lines = vec![
                    TextLine::new(rec, 15, rec_color, 1),
                    TextLine::new(format!("{} LIVE", dot(s.stream_elapsed.is_some())), 15, live_color, 1),
                ];
            }
        }
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

/// Charge du PC : processeur en grand, mémoire en dessous (touche « Infos PC » et écran latéral).
pub fn system_visual(stats: Option<Stats>, bg: [u8; 3]) -> KeyVisual {
    const GREY: [u8; 3] = [140, 155, 185];
    let mut v = KeyVisual { bg, ..KeyVisual::blank() };
    match stats {
        Some(st) => {
            // Vert → orange → rouge selon la charge du processeur.
            let color = match st.cpu {
                0..=59 => [126, 226, 168],
                60..=84 => [255, 190, 90],
                _ => [255, 100, 95],
            };
            v.lines = vec![
                TextLine::new("CPU", 10, GREY, 1),
                TextLine::new(format!("{} %", st.cpu), 21, color, 1),
                TextLine::new(format!("RAM {} %", st.ram), 11, [220, 228, 255], 1),
            ];
        }
        None => v.lines = vec![TextLine::new("CPU", 10, GREY, 1), TextLine::new("…", 20, GREY, 1)],
    }
    v
}

/// Micro Windows : rouge et barré quand il est coupé. `idle` = touche configurée (texte, fond).
pub fn mic_visual(muted: Option<bool>, idle: Option<KeyVisual>) -> KeyVisual {
    let mut v = idle.unwrap_or_else(|| KeyVisual { bg: [10, 14, 26], ..KeyVisual::blank() });
    match muted {
        Some(true) => {
            v.bg = ACTIVE_RED;
            v.icon = Some("obs/mic-off.svg".into());
            v.label = Some("Micro coupé".into());
        }
        Some(false) => {
            v.icon = Some("obs/mic.svg".into());
            if v.label.is_none() {
                v.label = Some("Micro actif".into());
            }
        }
        None => {
            v.icon = Some("obs/mic.svg".into());
            v.label = Some("Pas de micro".into());
            v.dim = true;
        }
    }
    v
}

/// Compteur : valeur en grand, nom en dessous ; le fond choisi dans l'éditeur est gardé.
pub fn counter_visual(idle: KeyVisual, name: &str, value: i64) -> KeyVisual {
    let label = idle.label.clone().unwrap_or_else(|| name.to_string());
    let size = match value.to_string().len() {
        0..=3 => 27,
        4 => 22,
        _ => 16,
    };
    KeyVisual {
        bg: idle.bg,
        lines: vec![TextLine::new(value.to_string(), size, [255, 255, 255], 1), TextLine::new(label, 10, [200, 212, 240], 1)],
        ..KeyVisual::blank()
    }
}
