//! Overlay Studio : bibliothèque d'overlays HTML et réglages sans code.
//!
//! Les overlays vivent dans un dossier « Studio » (copie de `Blackat-Overlays/overlays`).
//! Chaque écran charge `shared/studio.js`, écrit ici, qui contient un objet JSON
//! `window.OVERLAY_STUDIO` : variables CSS et textes par overlay + thème commun.
//! `shared/studio-apply.js` (côté overlay) applique ces réglages au chargement.
//! Les overlays restent des fichiers statiques : ils marchent dans OBS sans Soomfon.

pub mod schema;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Overlays fournis, dans l'ordre d'affichage. Les autres (copies, imports) suivent.
pub const BUILTIN: [(&str, &str, &str); 6] = [
    ("starting", "Starting Soon", "starting"),
    ("brb", "Be Right Back", "brb"),
    ("ending", "Ending", "ending"),
    ("just-chatting", "Just Chatting", "justchatting"),
    ("gameplay", "Gameplay", "gameplay"),
    ("alerts", "Alertes", "alerts"),
];

/// Réglages communs à tous les overlays.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    pub config: Value,
}

/// Réglages d'un overlay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlayEntry {
    pub name: String,
    /// Section de `OVERLAY_CONFIG` lue par l'écran (une copie de « starting » garde « starting »).
    pub section: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    pub config: Value,
}

fn yes() -> bool {
    true
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Studio {
    pub version: u32,
    #[serde(default)]
    pub global: Entry,
    #[serde(default)]
    pub overlays: BTreeMap<String, OverlayEntry>,
}

impl Default for Studio {
    fn default() -> Self {
        Self { version: 1, global: Entry { vars: BTreeMap::new(), config: empty_object() }, overlays: BTreeMap::new() }
    }
}

/// Un overlay de la bibliothèque, tel qu'affiché.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub section: String,
    pub enabled: bool,
    pub file: PathBuf,
}

const JS_PREFIX: &str = "window.OVERLAY_STUDIO = ";
const JS_HEADER: &str =
    "/* Réglages Soomfon Studio — écrit par Soomfon (onglet Overlays). Données JSON : ne pas modifier à la main. */\n";

/// Dossier Studio par défaut : `Documents\Blackat-Overlays\studio`.
pub fn default_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(r"Documents\Blackat-Overlays\studio")
}

fn studio_js(dir: &Path) -> PathBuf {
    dir.join("shared").join("studio.js")
}

impl Studio {
    /// Lit `shared/studio.js`. Fichier absent = réglages vides.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = studio_js(dir);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("lecture de {} : {e}", path.display())),
        };
        let start = text.find(JS_PREFIX).ok_or("studio.js : objet OVERLAY_STUDIO introuvable")? + JS_PREFIX.len();
        let json = text[start..].trim().trim_end_matches(';').trim();
        serde_json::from_str(json).map_err(|e| format!("studio.js illisible : {e}"))
    }

    /// Écrit `shared/studio.js` de façon atomique.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let path = studio_js(dir);
        let tmp = path.with_extension("js.tmp");
        std::fs::write(&tmp, format!("{JS_HEADER}{JS_PREFIX}{json};\n")).map_err(|e| format!("écriture : {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("remplacement de {} : {e}", path.display()))
    }

    /// Entrée d'un overlay (créée avec des valeurs par défaut si absente).
    pub fn entry_mut(&mut self, id: &str) -> &mut OverlayEntry {
        self.overlays.entry(id.to_string()).or_insert_with(|| default_entry(id))
    }
}

fn default_entry(id: &str) -> OverlayEntry {
    let (name, section) = BUILTIN
        .iter()
        .find(|(bid, _, _)| *bid == id)
        .map(|(_, n, s)| (n.to_string(), s.to_string()))
        .unwrap_or_else(|| (id.to_string(), id.to_string()));
    OverlayEntry { name, section, enabled: true, vars: BTreeMap::new(), config: empty_object() }
}

/// Liste les overlays : fichiers `*.html` du dossier + leurs réglages.
pub fn list(dir: &Path, studio: &Studio) -> Vec<Item> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("html")))
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let rank = |id: &str| BUILTIN.iter().position(|(b, _, _)| *b == id).unwrap_or(usize::MAX);
    ids.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    ids.into_iter()
        .map(|id| {
            let entry = studio.overlays.get(&id).cloned().unwrap_or_else(|| default_entry(&id));
            Item { file: dir.join(format!("{id}.html")), name: entry.name, section: entry.section, enabled: entry.enabled, id }
        })
        .collect()
}

/// URL `file:///` utilisable dans une source navigateur OBS.
pub fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let encoded: String = p
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            '?' => "%3F".to_string(),
            c => c.to_string(),
        })
        .collect();
    format!("file:///{encoded}")
}

fn unique_id(dir: &Path, base: &str) -> String {
    let base: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let base = if base.trim_matches('-').is_empty() { "overlay".to_string() } else { base };
    if !dir.join(format!("{base}.html")).exists() {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !dir.join(format!("{id}.html")).exists()).unwrap()
}

/// Duplique un overlay (fichier + réglages). Renvoie l'id de la copie.
pub fn duplicate(dir: &Path, studio: &mut Studio, id: &str) -> Result<String, String> {
    let new_id = unique_id(dir, &format!("{id}-copie"));
    std::fs::copy(dir.join(format!("{id}.html")), dir.join(format!("{new_id}.html")))
        .map_err(|e| format!("copie du fichier : {e}"))?;
    let mut entry = studio.overlays.get(id).cloned().unwrap_or_else(|| default_entry(id));
    entry.name = format!("{} (copie)", entry.name);
    studio.overlays.insert(new_id.clone(), entry);
    Ok(new_id)
}

/// Supprime un overlay de la copie Studio (fichier + réglages).
pub fn delete(dir: &Path, studio: &mut Studio, id: &str) -> Result<(), String> {
    std::fs::remove_file(dir.join(format!("{id}.html"))).map_err(|e| format!("suppression : {e}"))?;
    studio.overlays.remove(id);
    Ok(())
}

/// Exporte un overlay dans `dest/<id>/` : son fichier HTML + `reglages.json`.
pub fn export(dir: &Path, studio: &Studio, id: &str, dest: &Path) -> Result<PathBuf, String> {
    let out = dest.join(id);
    std::fs::create_dir_all(&out).map_err(|e| format!("création de {} : {e}", out.display()))?;
    std::fs::copy(dir.join(format!("{id}.html")), out.join(format!("{id}.html"))).map_err(|e| format!("copie : {e}"))?;
    let entry = studio.overlays.get(id).cloned().unwrap_or_else(|| default_entry(id));
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    std::fs::write(out.join("reglages.json"), json).map_err(|e| format!("écriture des réglages : {e}"))?;
    Ok(out)
}

/// Importe un fichier HTML d'overlay (et `reglages.json` s'il est à côté). Renvoie le nouvel id.
pub fn import(dir: &Path, studio: &mut Studio, html: &Path) -> Result<String, String> {
    let stem = html.file_stem().map(|s| s.to_string_lossy().to_string()).ok_or("fichier sans nom")?;
    let id = unique_id(dir, &stem);
    std::fs::copy(html, dir.join(format!("{id}.html"))).map_err(|e| format!("copie : {e}"))?;
    let settings = html.with_file_name("reglages.json");
    let entry = std::fs::read_to_string(&settings)
        .ok()
        .and_then(|t| serde_json::from_str::<OverlayEntry>(&t).ok())
        .unwrap_or_else(|| default_entry(&stem));
    studio.overlays.insert(id.clone(), entry);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("soomfon-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("shared")).unwrap();
        d
    }

    #[test]
    fn aller_retour_studio_js() {
        let dir = temp_dir("rt");
        let mut s = Studio::default();
        s.entry_mut("starting").vars.insert("--sunset".into(), "#ff0000".into());
        s.global.vars.insert("--bg".into(), "#000000".into());
        s.save(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("shared/studio.js")).unwrap();
        assert!(text.contains("window.OVERLAY_STUDIO = {"));
        assert_eq!(Studio::load(&dir).unwrap(), s);
    }

    #[test]
    fn bibliotheque_dupliquer_supprimer() {
        let dir = temp_dir("lib");
        std::fs::write(dir.join("starting.html"), "<html></html>").unwrap();
        std::fs::write(dir.join("zzz.html"), "<html></html>").unwrap();
        let mut s = Studio::default();
        let copy = duplicate(&dir, &mut s, "starting").unwrap();
        assert_eq!(copy, "starting-copie");
        assert_eq!(s.overlays[&copy].section, "starting");
        let ids: Vec<String> = list(&dir, &s).into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec!["starting", "starting-copie", "zzz"]);
        delete(&dir, &mut s, &copy).unwrap();
        assert!(!dir.join("starting-copie.html").exists());
        assert!(!s.overlays.contains_key(&copy));
    }

    #[test]
    fn url_fichier() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\A B\studio\starting.html")),
            "file:///C:/Users/A%20B/studio/starting.html"
        );
    }
}
