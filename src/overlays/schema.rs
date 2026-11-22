//! Réglages proposés dans l'éditeur, et leur traduction en variables CSS / textes.
//!
//! Chaque réglage pointe vers quelque chose qui existe vraiment dans les overlays :
//! une variable CSS utilisée par les feuilles de style, ou une clé de `OVERLAY_CONFIG`.
//! Les valeurs par défaut reproduisent l'état d'origine des fichiers.

use serde_json::{Map, Value};

use super::{Entry, OverlayEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Appearance,
    Typography,
    Position,
    Animation,
    Texts,
}

impl Group {
    pub const ALL: [Group; 5] = [Group::Appearance, Group::Typography, Group::Position, Group::Animation, Group::Texts];

    pub fn label(self) -> &'static str {
        match self {
            Group::Appearance => "Apparence",
            Group::Typography => "Typographie",
            Group::Position => "Position",
            Group::Animation => "Animations",
            Group::Texts => "Textes",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Color,
    /// Pixels (min, max, pas).
    Px(f32, f32, f32),
    /// Millisecondes (min, max, pas).
    Ms(f32, f32, f32),
    /// Nombre sans unité (min, max, pas).
    Number(f32, f32, f32),
    /// Pourcentage 0–100 affiché, 0–1 écrit.
    Percent(f32, f32),
    Text,
    /// Texte sur plusieurs lignes, séparées par « / » dans l'éditeur (titres).
    Lines,
    Select(&'static [(&'static str, &'static str)]),
    Toggle,
    Font(FontRole),
    /// Ombre portée en px (0 = aucune).
    Shadow(f32),
    /// Flou d'arrière-plan en px (0 = aucun).
    Blur(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontRole {
    Display,
    Ui,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// Variable CSS posée sur la page.
    Var(&'static str),
    /// Clé de la section de l'overlay (ou de la racine pour le thème commun).
    Config(&'static str),
    /// Réseau social n° i : type d'icône (true) ou texte (false).
    Social(usize, bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub label: &'static str,
    pub group: Group,
    pub kind: Kind,
    pub target: Target,
    pub default: &'static str,
}

const fn f(label: &'static str, group: Group, kind: Kind, target: Target, default: &'static str) -> Field {
    Field { label, group, kind, target, default }
}

use FontRole::*;
use Group::*;
use Kind::*;
use Target::*;

pub const DISPLAY_FONTS: &[(&str, &str)] = &[
    ("Barlow Condensed", "Barlow Condensed (d'origine)"),
    ("Bebas Neue", "Bebas Neue"),
    ("Oswald", "Oswald"),
    ("Anton", "Anton"),
    ("Teko", "Teko"),
    ("Rajdhani", "Rajdhani"),
    ("Russo One", "Russo One"),
];
pub const UI_FONTS: &[(&str, &str)] = &[
    ("Inter", "Inter (d'origine)"),
    ("Montserrat", "Montserrat"),
    ("Poppins", "Poppins"),
    ("Roboto", "Roboto"),
    ("Barlow", "Barlow"),
];
pub const MONO_FONTS: &[(&str, &str)] =
    &[("JetBrains Mono", "JetBrains Mono (d'origine)"), ("Roboto Mono", "Roboto Mono"), ("Space Mono", "Space Mono")];

const SOCIAL_ICONS: &[(&str, &str)] = &[
    ("", "— aucun —"),
    ("twitch", "Twitch"),
    ("tiktok", "TikTok"),
    ("discord", "Discord"),
    ("x", "X"),
    ("youtube", "YouTube"),
    ("instagram", "Instagram"),
    ("kick", "Kick"),
];
const CORNERS: &[(&str, &str)] = &[
    ("bottom-right", "En bas à droite"),
    ("bottom-left", "En bas à gauche"),
    ("top-right", "En haut à droite"),
    ("top-left", "En haut à gauche"),
];
const ALIGN: &[(&str, &str)] = &[("center", "Centre"), ("flex-start", "Gauche"), ("flex-end", "Droite")];

/// Thème commun à tous les overlays.
pub const GLOBAL: &[Field] = &[
    f("Fond", Appearance, Color, Var("--bg"), "#0A0B0D"),
    f("Texte", Appearance, Color, Var("--text"), "#ECEAE6"),
    f("Texte secondaire", Appearance, Color, Var("--muted"), "#8B9097"),
    f("Filets et bordures", Appearance, Color, Var("--line"), "#2B2F35"),
    f("Accent principal (subs, compteurs)", Appearance, Color, Var("--sunset"), "#E8855E"),
    f("Accent dons et bits", Appearance, Color, Var("--gold"), "#D6A84E"),
    f("Accent « live »", Appearance, Color, Var("--signal"), "#D9393E"),
    f("Photos de fond", Appearance, Toggle, Config("showBackgrounds"), "true"),
    f("Luminosité des photos", Appearance, Percent(10.0, 90.0), Var("--bg-brightness"), "42"),
    f("Police des titres", Typography, Font(Display), Var("--font-display"), "Barlow Condensed"),
    f("Police du texte", Typography, Font(Ui), Var("--font-ui"), "Inter"),
    f("Police des chiffres", Typography, Font(Mono), Var("--font-mono"), "JetBrains Mono"),
    f("Marge gauche / droite", Position, Px(0.0, 240.0, 4.0), Var("--pad-x"), "96"),
    f("Marge haut / bas", Position, Px(0.0, 200.0, 4.0), Var("--pad-y"), "64"),
    f("Durée d'apparition", Animation, Ms(0.0, 2000.0, 50.0), Var("--rise-dur"), "700"),
    f("Décalage entre éléments", Animation, Ms(0.0, 400.0, 10.0), Var("--rise-step"), "90"),
    f("Intervalle entre photos", Animation, Ms(4000.0, 60000.0, 1000.0), Config("backgroundInterval"), "14000"),
    f("Pseudo", Texts, Text, Config("streamer"), "Blackat"),
    f("Serveur", Texts, Text, Config("server"), "Révolution RP"),
    f("Jeu", Texts, Text, Config("game"), "GTA V RP"),
    f("Réseau 1 — type", Texts, Select(SOCIAL_ICONS), Social(0, true), "twitch"),
    f("Réseau 1 — texte", Texts, Text, Social(0, false), "twitch.tv/blackat04"),
    f("Réseau 2 — type", Texts, Select(SOCIAL_ICONS), Social(1, true), ""),
    f("Réseau 2 — texte", Texts, Text, Social(1, false), ""),
    f("Réseau 3 — type", Texts, Select(SOCIAL_ICONS), Social(2, true), ""),
    f("Réseau 3 — texte", Texts, Text, Social(2, false), ""),
];

const ACCENT: Field = f("Accent principal (cet overlay)", Appearance, Color, Var("--sunset"), "#E8855E");

const STARTING: &[Field] = &[
    ACCENT,
    f("Taille du titre", Typography, Px(80.0, 320.0, 2.0), Var("--display-size"), "200"),
    f("Durée du compte à rebours (min)", Animation, Number(1.0, 60.0, 1.0), Config("minutes"), "5"),
    f("Surtitre", Texts, Text, Config("kicker"), "Starting soon"),
    f("Titre (lignes séparées par /)", Texts, Lines, Config("title"), "Le live / commence"),
    f("Libellé du compteur", Texts, Text, Config("countLabel"), "Début dans"),
    f("Libellé à la fin", Texts, Text, Config("doneLabel"), "Ça commence"),
];

const BRB: &[Field] = &[
    ACCENT,
    f("Taille du titre", Typography, Px(80.0, 360.0, 2.0), Var("--display-size"), "260"),
    f("Surtitre", Texts, Text, Config("kicker"), "Be right back"),
    f("Titre (lignes séparées par /)", Texts, Lines, Config("title"), "Pause"),
    f("Sous-titre", Texts, Text, Config("subtitle"), "Je reviens dans un instant."),
];

const ENDING: &[Field] = &[
    ACCENT,
    f("Accent dons et bits (cet overlay)", Appearance, Color, Var("--gold"), "#D6A84E"),
    f("Taille du titre", Typography, Px(80.0, 300.0, 2.0), Var("--display-size"), "176"),
    f("Surtitre", Texts, Text, Config("kicker"), "Fin du live"),
    f("Titre (lignes séparées par /)", Texts, Lines, Config("title"), "Merci / d'être passé"),
    f("Sous-titre", Texts, Text, Config("subtitle"), "On se retrouve très vite en ville."),
    f("Titre des soutiens", Texts, Text, Config("supportersLabel"), "Derniers soutiens"),
];

const JUST_CHATTING: &[Field] = &[
    ACCENT,
    f("Teinte du cadre caméra", Appearance, Percent(0.0, 100.0), Var("--cam-tint"), "35"),
    f("Taille du sujet", Typography, Px(40.0, 140.0, 2.0), Var("--display-size"), "72"),
    f("Largeur du cadre caméra", Position, Px(640.0, 1400.0, 10.0), Var("--cam-w"), "1240"),
    f("Écart caméra / colonne", Position, Px(16.0, 160.0, 4.0), Var("--jc-gap"), "56"),
    f("Surtitre", Texts, Text, Config("kicker"), "Just chatting"),
    f("Libellé du sujet", Texts, Text, Config("topicLabel"), "Sujet du moment"),
    f("Sujet", Texts, Text, Config("topic"), "On refait la soirée en ville"),
    f("Libellé caméra", Texts, Text, Config("camLabel"), "Caméra"),
    f("Afficher les derniers soutiens", Texts, Toggle, Config("showSupporters"), "true"),
    f("Titre des soutiens", Texts, Text, Config("supportersLabel"), "Derniers soutiens"),
];

const GAMEPLAY: &[Field] = &[
    ACCENT,
    f("Opacité du bloc", Appearance, Percent(0.0, 100.0), Var("--hud-opacity"), "82"),
    f("Arrondi des coins", Appearance, Px(0.0, 30.0, 1.0), Var("--hud-radius"), "0"),
    f("Flou d'arrière-plan", Appearance, Blur(20.0), Var("--hud-backdrop"), "0"),
    f("Coin de l'écran", Position, Select(CORNERS), Config("corner"), "bottom-right"),
    f("Distance au bord", Position, Px(0.0, 200.0, 2.0), Var("--hud-margin"), "32"),
    f("Largeur", Position, Px(300.0, 700.0, 10.0), Var("--hud-width"), "420"),
    f("Mention après le pseudo", Texts, Text, Config("tag"), "/ live"),
    f("Afficher le dernier follow", Texts, Toggle, Config("showFollow"), "true"),
    f("Afficher le dernier sub", Texts, Toggle, Config("showSub"), "true"),
    f("Afficher le dernier don", Texts, Toggle, Config("showDonation"), "true"),
];

const ALERTS: &[Field] = &[
    f("Accent subs, gifts, raids", Appearance, Color, Var("--sunset"), "#E8855E"),
    f("Accent dons et bits", Appearance, Color, Var("--gold"), "#D6A84E"),
    f("Opacité du panneau", Appearance, Percent(0.0, 100.0), Var("--alert-opacity"), "88"),
    f("Arrondi des coins", Appearance, Px(0.0, 30.0, 1.0), Var("--alert-radius"), "0"),
    f("Ombre portée", Appearance, Shadow(60.0), Var("--alert-filter"), "0"),
    f("Flou d'arrière-plan", Appearance, Blur(20.0), Var("--alert-backdrop"), "0"),
    f("Distance au haut de l'écran", Position, Px(0.0, 900.0, 2.0), Var("--alert-top"), "28"),
    f("Alignement", Position, Select(ALIGN), Var("--alert-align"), "center"),
    f("Marge latérale", Position, Px(0.0, 400.0, 4.0), Var("--alert-side"), "0"),
    f("Largeur minimale", Position, Px(360.0, 900.0, 10.0), Var("--alert-width"), "520"),
    f("Durée d'affichage", Animation, Ms(2000.0, 15000.0, 500.0), Config("duration"), "5000"),
    f("Bonus si message", Animation, Ms(0.0, 10000.0, 500.0), Config("messageBonus"), "2500"),
    f("Pause entre deux alertes", Animation, Ms(0.0, 3000.0, 100.0), Config("gap"), "500"),
    f("Volume des sons", Animation, Percent(0.0, 100.0), Config("volume"), "50"),
];

const GENERIC: &[Field] = &[ACCENT];

/// Réglages d'un overlay selon sa section (les copies héritent de l'original).
pub fn fields_for(section: &str) -> &'static [Field] {
    match section {
        "starting" => STARTING,
        "brb" => BRB,
        "ending" => ENDING,
        "justchatting" => JUST_CHATTING,
        "gameplay" => GAMEPLAY,
        "alerts" => ALERTS,
        _ => GENERIC,
    }
}

// ── Lecture / écriture des valeurs ────────────────────────────────────────────

/// Clé où l'on garde la valeur « éditeur » d'une variable (ignorée par studio-apply.js).
fn ui_key(var: &str) -> String {
    format!("@{var}")
}

fn options(kind: Kind) -> &'static [(&'static str, &'static str)] {
    match kind {
        Select(o) => o,
        Font(Display) => DISPLAY_FONTS,
        Font(Ui) => UI_FONTS,
        Font(Mono) => MONO_FONTS,
        _ => &[],
    }
}

/// Libellés proposés pour un champ à choix.
pub fn option_labels(kind: Kind) -> Vec<String> {
    options(kind).iter().map(|(_, l)| l.to_string()).collect()
}

/// Valeur interne → index dans la liste de choix.
pub fn option_index(kind: Kind, value: &str) -> Option<usize> {
    options(kind).iter().position(|(v, _)| *v == value)
}

/// Index choisi → valeur interne.
pub fn option_value(kind: Kind, index: usize) -> Option<&'static str> {
    options(kind).get(index).map(|(v, _)| *v)
}

/// Valeur CSS écrite pour une variable, à partir de la valeur « éditeur ».
fn css_value(kind: Kind, v: &str) -> String {
    let num = || v.parse::<f32>().unwrap_or(0.0);
    match kind {
        Px(..) => format!("{}px", num().round()),
        Ms(..) => format!("{}ms", num().round()),
        Percent(..) => format!("{}", (num() / 100.0 * 1000.0).round() / 1000.0),
        Font(role) => {
            let fallback = match role {
                Display => "\"Arial Narrow\", sans-serif",
                Ui => "\"Segoe UI\", sans-serif",
                Mono => "Consolas, monospace",
            };
            format!("\"{v}\", {fallback}")
        }
        Shadow(_) if num() <= 0.0 => "none".into(),
        Shadow(_) => format!("drop-shadow(0 {}px {}px rgba(0, 0, 0, 0.6))", (num() / 3.0).round(), num().round()),
        Blur(_) if num() <= 0.0 => "none".into(),
        Blur(_) => format!("blur({}px)", num().round()),
        _ => v.to_string(),
    }
}

/// Valeur JSON écrite dans la config, à partir de la valeur « éditeur ».
fn json_value(kind: Kind, v: &str) -> Value {
    let num = v.parse::<f64>().unwrap_or(0.0);
    match kind {
        Toggle => Value::Bool(v == "true"),
        Ms(..) | Number(..) | Px(..) => serde_json::json!(num.round() as i64),
        Percent(..) => serde_json::json!((num / 100.0 * 100.0).round() / 100.0),
        Lines => Value::Array(
            v.split('/').map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| Value::String(l.into())).collect(),
        ),
        _ => Value::String(v.to_string()),
    }
}

fn display_from_json(kind: Kind, j: &Value) -> String {
    match (kind, j) {
        (Lines, Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" / "),
        (Percent(..), Value::Number(n)) => format!("{}", (n.as_f64().unwrap_or(0.0) * 100.0).round()),
        (_, Value::Bool(b)) => b.to_string(),
        (_, Value::Number(n)) => n.to_string(),
        (_, Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Vue commune sur les réglages du thème global ou d'un overlay.
pub struct Slot<'a> {
    pub vars: &'a mut std::collections::BTreeMap<String, String>,
    pub config: &'a mut Value,
}

impl<'a> From<&'a mut Entry> for Slot<'a> {
    fn from(e: &'a mut Entry) -> Self {
        Slot { vars: &mut e.vars, config: &mut e.config }
    }
}

impl<'a> From<&'a mut OverlayEntry> for Slot<'a> {
    fn from(e: &'a mut OverlayEntry) -> Self {
        Slot { vars: &mut e.vars, config: &mut e.config }
    }
}

fn config_obj(config: &mut Value) -> &mut Map<String, Value> {
    if !config.is_object() {
        *config = Value::Object(Map::new());
    }
    config.as_object_mut().unwrap()
}

/// Les 3 emplacements de réseaux tels que saisis (même incomplets), gardés à part :
/// la liste `socials` lue par les overlays ne contient que les réseaux complets.
const SOCIAL_SLOTS: &str = "@socials";

/// Réseaux sociaux : [(icône, texte)] × 3, depuis la saisie, la config ou les valeurs par défaut.
fn socials(config: &Value) -> Vec<(String, String)> {
    let source = config.get(SOCIAL_SLOTS).or_else(|| config.get("socials"));
    let mut out: Vec<(String, String)> = match source.and_then(|s| s.as_array()) {
        Some(arr) => arr
            .iter()
            .map(|s| {
                (
                    s.get("icon").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    s.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                )
            })
            .collect(),
        None => vec![("twitch".into(), "twitch.tv/blackat04".into())],
    };
    out.resize(3, (String::new(), String::new()));
    out
}

impl Slot<'_> {
    /// Valeur affichée dans l'éditeur, et si elle diffère de la valeur d'origine.
    pub fn get(&self, field: &Field) -> (String, bool) {
        let value = match field.target {
            Var(name) => self.vars.get(&ui_key(name)).cloned(),
            Config(key) => self.config.get(key).map(|j| display_from_json(field.kind, j)),
            Social(i, icon) => self.config.get(SOCIAL_SLOTS).or_else(|| self.config.get("socials")).map(|_| {
                let s = socials(self.config);
                if icon { s[i].0.clone() } else { s[i].1.clone() }
            }),
        };
        match value {
            Some(v) => {
                let modified = v != field.default;
                (v, modified)
            }
            None => (field.default.to_string(), false),
        }
    }

    /// Enregistre une valeur saisie dans l'éditeur.
    pub fn set(&mut self, field: &Field, value: &str) {
        match field.target {
            Var(name) => {
                self.vars.insert(ui_key(name), value.to_string());
                self.vars.insert(name.to_string(), css_value(field.kind, value));
            }
            Config(key) => {
                config_obj(self.config).insert(key.to_string(), json_value(field.kind, value));
            }
            Social(i, icon) => {
                let mut s = socials(self.config);
                if icon {
                    s[i].0 = value.to_string();
                } else {
                    s[i].1 = value.to_string();
                }
                let to_json = |(ic, label): &(String, String)| serde_json::json!({ "icon": ic, "label": label });
                let slots: Vec<Value> = s.iter().map(to_json).collect();
                let shown: Vec<Value> =
                    s.iter().filter(|(ic, label)| !ic.is_empty() && !label.trim().is_empty()).map(to_json).collect();
                let obj = config_obj(self.config);
                obj.insert(SOCIAL_SLOTS.into(), Value::Array(slots));
                obj.insert("socials".into(), Value::Array(shown));
            }
        }
    }

    /// Revient à la valeur d'origine pour ce réglage.
    pub fn reset(&mut self, field: &Field) {
        match field.target {
            Var(name) => {
                self.vars.remove(name);
                self.vars.remove(&ui_key(name));
            }
            Config(key) => {
                config_obj(self.config).remove(key);
            }
            Social(..) => {
                let obj = config_obj(self.config);
                obj.remove("socials");
                obj.remove(SOCIAL_SLOTS);
            }
        }
    }

    /// Revient à l'origine pour tous les réglages.
    pub fn reset_all(&mut self) {
        self.vars.clear();
        *self.config = Value::Object(Map::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlays::Studio;

    #[test]
    fn valeurs_par_defaut_non_modifiees() {
        let mut s = Studio::default();
        let slot: Slot = s.entry_mut("starting").into();
        for field in STARTING {
            let (v, modified) = slot.get(field);
            assert_eq!(v, field.default);
            assert!(!modified);
        }
    }

    #[test]
    fn variables_css_et_config() {
        let mut s = Studio::default();
        let mut slot: Slot = s.entry_mut("alerts").into();
        let size = ALERTS.iter().find(|f| f.target == Var("--alert-top")).unwrap();
        slot.set(size, "120");
        assert_eq!(slot.vars["--alert-top"], "120px");
        assert_eq!(slot.get(size), ("120".into(), true));

        let shadow = ALERTS.iter().find(|f| f.target == Var("--alert-filter")).unwrap();
        slot.set(shadow, "30");
        assert_eq!(slot.vars["--alert-filter"], "drop-shadow(0 10px 30px rgba(0, 0, 0, 0.6))");
        slot.set(shadow, "0");
        assert_eq!(slot.vars["--alert-filter"], "none");

        let opacity = ALERTS.iter().find(|f| f.target == Var("--alert-opacity")).unwrap();
        slot.set(opacity, "75");
        assert_eq!(slot.vars["--alert-opacity"], "0.75");

        let dur = ALERTS.iter().find(|f| f.target == Config("duration")).unwrap();
        slot.set(dur, "7000");
        assert_eq!(slot.config["duration"], 7000);
        slot.reset(dur);
        assert!(slot.config.get("duration").is_none());
    }

    #[test]
    fn titre_sur_plusieurs_lignes_et_reseaux() {
        let mut s = Studio::default();
        let mut slot: Slot = s.entry_mut("starting").into();
        let title = STARTING.iter().find(|f| f.target == Config("title")).unwrap();
        slot.set(title, "On / y / va");
        assert_eq!(slot.config["title"], serde_json::json!(["On", "y", "va"]));
        assert_eq!(slot.get(title).0, "On / y / va");

        let mut g: Slot = (&mut s.global).into();
        let icon2 = GLOBAL.iter().find(|f| f.target == Social(1, true)).unwrap();
        let text2 = GLOBAL.iter().find(|f| f.target == Social(1, false)).unwrap();
        g.set(icon2, "tiktok");
        g.set(text2, "@blackat");
        assert_eq!(
            g.config["socials"],
            serde_json::json!([{ "icon": "twitch", "label": "twitch.tv/blackat04" }, { "icon": "tiktok", "label": "@blackat" }])
        );
    }

    #[test]
    fn police() {
        let mut s = Studio::default();
        let mut g: Slot = (&mut s.global).into();
        let font = GLOBAL.iter().find(|f| f.target == Var("--font-display")).unwrap();
        g.set(font, "Bebas Neue");
        assert_eq!(g.vars["--font-display"], "\"Bebas Neue\", \"Arial Narrow\", sans-serif");
        assert_eq!(option_index(font.kind, "Bebas Neue"), Some(1));
    }
}
