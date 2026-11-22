//! Pack Blackat : sauvegarde complète de la configuration de stream, pour réinstaller
//! un PC ou changer de machine.
//!
//! ```text
//! Blackat-Pack-AAAA-MM-JJ.zip          (zip normal : s'ouvre avec l'Explorateur Windows)
//! ├─ Blackat Launcher.exe              ← on double-clique ici, puis « Installer »
//! ├─ LISEZMOI.txt
//! ├─ manifest.json                     versions, chemins d'origine, contenu (aucun secret)
//! ├─ app\                              launcher, Stream Deck, éditeur, icônes
//! ├─ installers\                       installeur officiel d'OBS (version identique)
//! └─ data.blackat                      zip chiffré AES-256 : configs, jetons, clé de stream
//!    ├─ obs\  obs-plugins\  streamerbot\  overlays\  soomfon\  media\
//! ```

pub mod backup;
pub mod restore;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Version du format de pack (pour les futures évolutions de la restauration).
pub const FORMAT: u32 = 1;
/// Nom de l'archive chiffrée dans le pack.
pub const DATA_FILE: &str = "data.blackat";
pub const MANIFEST_FILE: &str = "manifest.json";

/// Dossiers de caches recréés automatiquement : jamais sauvegardés.
pub const CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "GrShaderCache",
    "ShaderCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GraphiteDawnCache",
    "Dictionaries",
    "Service Worker",
    "blob_storage",
    "Crashpad",
    "component_crx_cache",
    "extensions_crx_cache",
];

/// Emplacements des données sur un PC.
#[derive(Debug, Clone)]
pub struct Locations {
    pub user_profile: PathBuf,
    pub appdata: PathBuf,
    pub obs: PathBuf,
    pub obs_plugins: PathBuf,
    pub streamerbot: PathBuf,
    pub overlays: PathBuf,
    pub soomfon: PathBuf,
}

impl Locations {
    pub fn current() -> Self {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
        let user_profile = env("USERPROFILE");
        let appdata = env("APPDATA");
        let program_data = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        Self {
            obs: appdata.join("obs-studio"),
            obs_plugins: program_data.join(r"obs-studio\plugins"),
            streamerbot: user_profile.join(r"Documents\Streamer.bot"),
            overlays: user_profile.join(r"Documents\Blackat-Overlays"),
            soomfon: appdata.join("soomfon-deck"),
            user_profile,
            appdata,
        }
    }
}

/// Un fichier utilisé par une scène OBS, hors des dossiers déjà sauvegardés.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaEntry {
    /// Chemin tel qu'écrit dans OBS (barres obliques normalisées en « / »).
    pub original: String,
    /// Chemin dans l'archive chiffrée.
    pub pack: String,
}

/// Description du pack (en clair : ne contient aucun secret).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub created: String,
    /// Dossiers du PC d'origine, pour réécrire les chemins à la restauration.
    pub user_profile: String,
    pub appdata: String,
    pub obs_version: Option<String>,
    /// Nom du fichier dans `installers\` (absent : installation via winget, Internet requis).
    pub obs_installer: Option<String>,
    pub obs_plugins: Vec<String>,
    pub sections: Vec<SectionInfo>,
    pub media: Vec<MediaEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionInfo {
    pub name: String,
    pub files: usize,
    pub bytes: u64,
}

/// Extrait `data.blackat` d'un pack (zip) vers `dest`.
pub fn extract_data(pack: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(pack).map_err(|e| format!("pack illisible : {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("pack invalide : {e}"))?;
    let mut entry = zip.by_name(DATA_FILE).map_err(|_| "data.blackat absent du pack".to_string())?;
    let mut out = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    Ok(())
}

/// Vérifie le mot de passe et l'intégrité de l'archive chiffrée : lit chaque fichier en entier
/// (le contrôle d'authenticité AES échoue si le mot de passe ou les données sont faux).
/// Renvoie (nombre de fichiers, octets).
pub fn verify_data(data: &Path, password: &str) -> Result<(usize, u64), String> {
    let file = std::fs::File::open(data).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("archive chiffrée invalide : {e}"))?;
    let mut bytes = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index_decrypt(i, password.as_bytes())
            .map_err(|_| "mot de passe incorrect".to_string())?;
        bytes += std::io::copy(&mut entry, &mut std::io::sink()).map_err(|_| "mot de passe incorrect ou pack abîmé".to_string())?;
    }
    Ok((zip.len(), bytes))
}

/// Le chemin est-il dans le dossier `root` (comparaison insensible à la casse, « / » ou « \ ») ?
pub fn is_inside(path: &str, root: &Path) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let p = norm(path);
    let r = norm(&root.to_string_lossy());
    !r.is_empty() && (p == r || p.starts_with(&format!("{r}/")))
}

/// Chemin `file:///C:/...%20...` → `C:/... ...` ; chemin Windows → barres obliques « / ».
pub fn local_path(value: &str) -> Option<String> {
    let v = value.trim();
    let raw = if let Some(rest) = v.strip_prefix("file:///") {
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        percent_decode(rest)
    } else {
        v.to_string()
    };
    let b = raw.as_bytes();
    let drive = b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\');
    drive.then(|| raw.replace('\\', "/"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Date et heure UTC « AAAA-MM-JJ HH:MM » sans dépendance.
pub fn now_utc() -> (String, String) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let date = format!("{y:04}-{m:02}-{d:02}");
    (date.clone(), format!("{date} {:02}:{:02} UTC", rem / 3600, (rem / 60) % 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemins_locaux() {
        assert_eq!(
            local_path("file:///C:/Users/A%20B/Documents/x.html?min=5").as_deref(),
            Some("C:/Users/A B/Documents/x.html")
        );
        assert_eq!(local_path(r"C:\Users\A\Videos\a.mp3").as_deref(), Some("C:/Users/A/Videos/a.mp3"));
        assert_eq!(local_path("https://exemple.fr/a.png"), None);
        assert_eq!(local_path("ffmpeg_source"), None);
        assert_eq!(local_path("Capture d'écran %"), None);
    }

    #[test]
    fn dans_un_dossier() {
        let root = Path::new(r"C:\Users\Blackat\Documents\Blackat-Overlays");
        assert!(is_inside("c:/users/blackat/documents/blackat-overlays/studio/brb.html", root));
        assert!(!is_inside("C:/Users/Blackat/Documents/Blackat-Overlays2/x", root));
        assert!(!is_inside("C:/Users/Blackat/Videos/a.mp3", root));
    }
}
