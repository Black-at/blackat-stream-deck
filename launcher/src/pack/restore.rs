//! Restauration d'un pack Blackat (bouton « Installer » du launcher, lancé depuis le pack extrait).
//!
//! Ordre : vérification du mot de passe → mise de côté de l'existant (copies `.bak-…`) →
//! installation d'OBS → copie des configs → réécriture des chemins de l'ancien PC →
//! périphériques audio introuvables mis sur « Par défaut » → programmes Blackat + raccourci →
//! contrôle des fichiers utilisés par les scènes.
//!
//! Mode « bac à sable » (tests) : tout est restauré sous un dossier racine qui simule un autre
//! PC ; rien n'est installé ni lancé, le vrai profil Windows n'est pas touché.

use std::{
    io,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

use zip::ZipArchive;

use super::{
    DATA_FILE, Locations, MANIFEST_FILE, Manifest, backup::Progress, backup::is_running, local_path, now_utc, verify_data,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const STEPS: [&str; 10] = [
    "Vérification du pack",
    "Mise de côté de l'existant",
    "Installation d'OBS",
    "Configuration OBS",
    "Plugins OBS",
    "Streamer.bot",
    "Overlays et médias",
    "Stream Deck (Blackat)",
    "Chemins et audio",
    "Contrôle final",
];

pub struct Options {
    /// Dossier du pack extrait (contient manifest.json, data.blackat, app\, installers\).
    pub pack_dir: PathBuf,
    pub password: String,
    /// Racine d'un PC simulé (tests) : rien n'est installé, lancé ni créé hors de ce dossier.
    pub sandbox: Option<PathBuf>,
}

pub struct Outcome {
    pub warnings: Vec<String>,
    pub missing: Vec<String>,
    pub backups: Vec<PathBuf>,
    /// Launcher installé (pour « Lancer Blackat »).
    pub launcher: PathBuf,
}

/// Emplacements cibles : vrai PC, ou PC simulé sous `root`.
pub struct Targets {
    pub loc: Locations,
    pub local_appdata: PathBuf,
    pub desktop: PathBuf,
}

impl Targets {
    fn real() -> Self {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
        let loc = Locations::current();
        let desktop = loc.user_profile.join("Desktop");
        Self { local_appdata: env("LOCALAPPDATA"), desktop, loc }
    }

    fn sandbox(root: &Path) -> Self {
        let user = root.join(r"Users\NouveauPC");
        let appdata = user.join(r"AppData\Roaming");
        Self {
            loc: Locations {
                obs: appdata.join("obs-studio"),
                obs_plugins: root.join(r"ProgramData\obs-studio\plugins"),
                streamerbot: user.join(r"Documents\Streamer.bot"),
                overlays: user.join(r"Documents\Blackat-Overlays"),
                soomfon: appdata.join("soomfon-deck"),
                user_profile: user.clone(),
                appdata,
            },
            local_appdata: user.join(r"AppData\Local"),
            desktop: user.join("Desktop"),
        }
    }
}

// ── Réécriture des chemins ────────────────────────────────────────────────────

/// Remplace le profil de l'ancien PC par le nouveau dans un texte (JSON, INI…), sous toutes
/// ses écritures : `C:\Users\A`, `C:\\Users\\A` (JSON), `C:/Users/A`, `file:///C:/Users/A`.
/// Insensible à la casse sur l'ancien chemin. Renvoie le nombre de remplacements.
pub fn rewrite_profile(text: &str, old: &str, new: &str) -> (String, usize) {
    let old = old.trim_end_matches(['\\', '/']);
    let new = new.trim_end_matches(['\\', '/']);
    if old.is_empty() || old.eq_ignore_ascii_case(new) {
        return (text.to_string(), 0);
    }
    let variants = [
        (old.replace('/', "\\").replace('\\', "\\\\"), new.replace('/', "\\").replace('\\', "\\\\")),
        (old.replace('/', "\\"), new.replace('/', "\\")),
        (old.replace('\\', "/"), new.replace('\\', "/")),
    ];
    let mut out = text.to_string();
    let mut count = 0;
    for (from, to) in variants {
        let (replaced, n) = replace_ignore_case(&out, &from, &to);
        out = replaced;
        count += n;
    }
    (out, count)
}

fn replace_ignore_case(text: &str, from: &str, to: &str) -> (String, usize) {
    if from.is_empty() {
        return (text.to_string(), 0);
    }
    let lower = text.to_lowercase();
    let needle = from.to_lowercase();
    // Les chemins sont en ASCII pour la partie comparée : les index restent alignés si
    // la mise en minuscules ne change pas la longueur (on vérifie, sinon on renonce).
    if lower.len() != text.len() {
        return (text.replace(from, to), text.matches(from).count());
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut count = 0;
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&needle) {
        let start = i + pos;
        let end = start + needle.len();
        // Évite « C:\Users\Blackat2 » quand l'ancien profil est « C:\Users\Blackat ».
        let next = text[end..].chars().next();
        if next.is_none_or(|c| matches!(c, '\\' | '/' | '"' | '\'' | ';' | ',' | '?' | '#' | '\r' | '\n' | ' ')) {
            out.push_str(&text[last..start]);
            out.push_str(to);
            last = end;
            count += 1;
        }
        i = end;
    }
    out.push_str(&text[last..]);
    (out, count)
}

/// Périphériques audio inconnus sur ce PC → « default ». Concerne `"device_id": "…"` (JSON)
/// et `…DeviceId=…` (INI). Renvoie le texte et les identifiants remplacés.
pub fn fix_audio_devices(text: &str, known: &[String]) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut replaced = Vec::new();
    let is_known = |id: &str| id.eq_ignore_ascii_case("default") || id.is_empty() || known.iter().any(|k| k.eq_ignore_ascii_case(id));
    for line in text.split_inclusive('\n') {
        let mut l = line.to_string();
        // JSON : "device_id": "{0.0.1.00000000}.{…}"
        if let Some(p) = l.find("\"device_id\"") {
            if let Some(q1) = l[p + 11..].find('"').map(|x| p + 11 + x) {
                if let Some(q2) = l[q1 + 1..].find('"').map(|x| q1 + 1 + x) {
                    let id = l[q1 + 1..q2].to_string();
                    if !is_known(&id) {
                        l.replace_range(q1 + 1..q2, "default");
                        replaced.push(id);
                    }
                }
            }
        }
        // INI : MonitoringDeviceId=…
        if let Some(eq) = l.find('=') {
            let key = l[..eq].trim();
            if key.ends_with("DeviceId") {
                let id = l[eq + 1..].trim().to_string();
                if !is_known(&id) {
                    let ending = if l.ends_with("\r\n") { "\r\n" } else if l.ends_with('\n') { "\n" } else { "" };
                    l = format!("{}=default{ending}", &l[..eq]);
                    replaced.push(id);
                }
            }
        }
        out.push_str(&l);
    }
    (out, replaced)
}

/// Identifiants des périphériques audio présents (format OBS : `{0.0.x.00000000}.{GUID}`).
fn audio_endpoints() -> Vec<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-PnpDevice -Class AudioEndpoint -PresentOnly | ForEach-Object { $_.InstanceId }"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    out.map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|l| l.trim().rsplit('\\').next().map(str::to_string))
            .filter(|s| s.starts_with('{'))
            .collect()
    })
    .unwrap_or_default()
}

// ── Extraction ────────────────────────────────────────────────────────────────

struct Data {
    zip: ZipArchive<std::fs::File>,
    password: String,
}

impl Data {
    /// Extrait toutes les entrées commençant par `prefix/` vers `dest`. Renvoie le nombre de fichiers.
    fn extract(&mut self, prefix: &str, dest: &Path) -> Result<usize, String> {
        let pre = format!("{prefix}/");
        let mut n = 0;
        for i in 0..self.zip.len() {
            let name = match self.zip.name_for_index(i) { Some(Ok(s)) => s.into_owned(), _ => continue };
            let Some(rel) = name.strip_prefix(&pre) else { continue };
            // Refuse tout chemin qui sortirait du dossier cible (« .. », absolu).
            if rel.split('/').any(|p| p == ".." || p.contains(':')) || rel.starts_with('/') {
                continue;
            }
            let target = dest.join(rel.replace('/', "\\"));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
            }
            let mut entry = self.zip.by_index_decrypt(i, self.password.as_bytes()).map_err(|_| "mot de passe incorrect".to_string())?;
            let mut out = std::fs::File::create(&target).map_err(|e| format!("{} : {e}", target.display()))?;
            io::copy(&mut entry, &mut out).map_err(|e| format!("{} : {e}", target.display()))?;
            n += 1;
        }
        Ok(n)
    }

    fn extract_file(&mut self, name: &str, dest: &Path) -> Result<bool, String> {
        let Some(i) = self.zip.index_for_name(name) else { return Ok(false) };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut entry = self.zip.by_index_decrypt(i, self.password.as_bytes()).map_err(|_| "mot de passe incorrect".to_string())?;
        let mut out = std::fs::File::create(dest).map_err(|e| format!("{} : {e}", dest.display()))?;
        io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

/// Renomme un dossier existant en `<dossier>.bak-AAAA-MM-JJ-HHMM` (rien n'est supprimé).
fn set_aside(dir: &Path, stamp: &str, backups: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let mut bak = PathBuf::from(format!("{}.bak-{stamp}", dir.display()));
    let mut n = 2;
    while bak.exists() {
        bak = PathBuf::from(format!("{}.bak-{stamp}-{n}", dir.display()));
        n += 1;
    }
    std::fs::rename(dir, &bak).map_err(|e| format!("mise de côté de {} impossible : {e}", dir.display()))?;
    backups.push(bak);
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> io::Result<usize> {
    let mut n = 0;
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)?.flatten() {
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            n += copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
            n += 1;
        }
    }
    Ok(n)
}

fn obs_exe() -> PathBuf {
    std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_default().join(r"obs-studio\bin\64bit\obs64.exe")
}

/// Lance l'installeur OBS du pack en silencieux, avec la demande d'autorisation Windows.
fn install_obs(installer: &Path) -> Result<(), String> {
    let sig = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("(Get-AuthenticodeSignature -LiteralPath '{}').Status", installer.display())])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if sig != "Valid" {
        return Err(format!("signature de l'installeur OBS invalide ({sig}) : installation annulée"));
    }
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Start-Process -FilePath '{}' -ArgumentList '/S' -Verb RunAs -Wait", installer.display()),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() || !obs_exe().exists() {
        return Err("OBS n'a pas été installé (autorisation refusée ?)".into());
    }
    Ok(())
}

fn install_obs_winget() -> Result<(), String> {
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            "OBSProject.OBSStudio",
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("winget introuvable : {e}"))?;
    if status.success() && obs_exe().exists() { Ok(()) } else { Err("installation d'OBS par winget échouée".into()) }
}

/// Raccourci « Blackat » sur le Bureau, avec l'icône du logo.
fn desktop_shortcut(desktop: &Path, target: &Path, icon: &Path) -> Result<(), String> {
    let lnk = desktop.join("Blackat.lnk");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='Lance OBS, Streamer.bot et le Stream Deck';$s.Save()",
        lnk.display(),
        target.display(),
        target.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        icon.display()
    );
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok { Ok(()) } else { Err("raccourci du Bureau non créé".into()) }
}

/// Applique `f` à chaque fichier texte (.json / .ini) d'un dossier.
fn for_text_files(dir: &Path, f: &mut dyn FnMut(&Path, String) -> Option<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name != "obs-browser" {
                for_text_files(&p, f);
            }
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json") || e.eq_ignore_ascii_case("ini")) {
            if let Ok(bytes) = std::fs::read(&p) {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if let Some(new) = f(&p, text) {
                    let _ = std::fs::write(&p, new);
                }
            }
        }
    }
}

const ICON: &[u8] = include_bytes!("../../ui/blackat.ico");

pub fn run(opts: &Options, progress: &dyn Progress) -> Result<Outcome, String> {
    let sandboxed = opts.sandbox.is_some();
    let t = match &opts.sandbox {
        Some(root) => Targets::sandbox(root),
        None => Targets::real(),
    };
    let mut warnings = Vec::new();
    let mut backups = Vec::new();

    // 0. Vérification
    progress.step(0, 1, "mot de passe…");
    let manifest: Manifest = std::fs::read_to_string(opts.pack_dir.join(MANIFEST_FILE))
        .map_err(|_| "manifest.json introuvable : lance le launcher depuis le pack extrait".to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| format!("manifest.json illisible : {e}")))?;
    let data_path = opts.pack_dir.join(DATA_FILE);
    let (files, _) = verify_data(&data_path, &opts.password).inspect_err(|_| progress.step(0, 3, "refusé"))?;
    if !sandboxed && (is_running("obs64.exe") || is_running("Streamer.bot.exe")) {
        progress.step(0, 3, "OBS ou Streamer.bot ouvert");
        return Err("ferme OBS et Streamer.bot avant d'installer".into());
    }
    let mut data = Data {
        zip: ZipArchive::new(std::fs::File::open(&data_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?,
        password: opts.password.clone(),
    };
    progress.step(0, 2, &format!("mot de passe OK · {files} fichiers"));

    // 1. Mise de côté de l'existant
    progress.step(1, 1, "copies .bak…");
    let stamp = now_utc().1.replace([' ', ':'], "-").replace("-UTC", "");
    for dir in [&t.loc.obs, &t.loc.streamerbot, &t.loc.overlays, &t.loc.soomfon] {
        set_aside(dir, &stamp, &mut backups)?;
    }
    progress.step(1, 2, &if backups.is_empty() { "rien à mettre de côté".to_string() } else { format!("{} dossier(s) en .bak", backups.len()) });

    // 2. OBS
    progress.step(2, 1, "…");
    if sandboxed {
        progress.step(2, 2, "ignoré (PC simulé)");
    } else if obs_exe().exists() {
        progress.step(2, 2, "déjà installé");
    } else {
        let installer = manifest.obs_installer.as_ref().map(|n| opts.pack_dir.join("installers").join(n));
        progress.step(2, 1, "autorise l'installation dans la fenêtre Windows…");
        let result = match installer.filter(|p| p.is_file()) {
            Some(p) => install_obs(&p),
            None => install_obs_winget(),
        };
        match result {
            Ok(()) => progress.step(2, 2, &format!("OBS {} installé", manifest.obs_version.as_deref().unwrap_or(""))),
            Err(e) => {
                progress.step(2, 3, &e);
                warnings.push(format!("OBS : {e}"));
            }
        }
    }

    // 3. Config OBS
    progress.step(3, 1, "copie…");
    let n = data.extract("obs", &t.loc.obs)?;
    progress.step(3, 2, &format!("{n} fichiers"));

    // 4. Plugins OBS
    progress.step(4, 1, "copie…");
    let mut plugin_files = 0;
    for name in &manifest.obs_plugins {
        let dest = t.loc.obs_plugins.join(name);
        match data.extract(&format!("obs-plugins/{name}"), &dest) {
            Ok(n) => plugin_files += n,
            Err(e) => warnings.push(format!("Plugin OBS « {name} » non restauré ({e}) — droits administrateur nécessaires ?")),
        }
    }
    progress.step(4, 2, &if manifest.obs_plugins.is_empty() { "aucun".to_string() } else { format!("{} plugin(s), {plugin_files} fichiers", manifest.obs_plugins.len()) });

    // 5. Streamer.bot
    progress.step(5, 1, "copie…");
    let n = data.extract("streamerbot", &t.loc.streamerbot)?;
    progress.step(5, 2, &format!("{n} fichiers"));

    // 6. Overlays + médias des scènes
    progress.step(6, 1, "copie…");
    let n = data.extract("overlays", &t.loc.overlays)?;
    let new_profile = t.loc.user_profile.to_string_lossy().to_string();
    let mut media_ok = 0;
    for m in &manifest.media {
        let (target, _) = rewrite_profile(&m.original, &manifest.user_profile, &new_profile);
        let mut dest = PathBuf::from(target.replace('/', "\\"));
        // En PC simulé, un chemin hors du profil irait sur le vrai disque : on le range dans le bac à sable.
        if sandboxed && !super::is_inside(&dest.to_string_lossy(), &t.loc.user_profile) {
            dest = t.loc.user_profile.join(r"Documents\Blackat-Media").join(dest.file_name().unwrap_or_default());
        }
        if dest.exists() {
            warnings.push(format!("Média déjà présent, conservé : {}", dest.display()));
            continue;
        }
        match data.extract_file(&m.pack, &dest) {
            Ok(true) => media_ok += 1,
            _ => warnings.push(format!("Média non restauré : {}", m.original)),
        }
    }
    progress.step(6, 2, &format!("{n} fichiers · {media_ok}/{} média(s)", manifest.media.len()));

    // 7. Programmes Blackat + config Soomfon + raccourci
    progress.step(7, 1, "copie…");
    let app_dest = t.local_appdata.join(r"Programs\Blackat");
    let app_src = opts.pack_dir.join("app");
    let n = copy_dir(&app_src, &app_dest).map_err(|e| format!("programmes Blackat : {e}"))?;
    let icon = app_dest.join("blackat.ico");
    let _ = std::fs::write(&icon, ICON);
    data.extract_file("soomfon/config.toml", &t.loc.soomfon.join("config.toml"))?;
    std::fs::create_dir_all(&t.desktop).map_err(|e| e.to_string())?;
    if let Err(e) = desktop_shortcut(&t.desktop, &app_dest.join("blackat-launcher.exe"), &icon) {
        warnings.push(e);
    }
    progress.step(7, 2, &format!("{n} fichiers · raccourci Bureau"));

    // 8. Chemins de l'ancien PC + périphériques audio
    progress.step(8, 1, "réécriture…");
    let known = if sandboxed { Vec::new() } else { audio_endpoints() };
    let mut rewrites = 0;
    let mut audio = Vec::new();
    let old = manifest.user_profile.clone();
    for dir in [&t.loc.obs, &t.loc.streamerbot.join("data")] {
        for_text_files(dir, &mut |path, text| {
            let (mut out, n) = rewrite_profile(&text, &old, &new_profile);
            rewrites += n;
            // Audio : uniquement côté OBS, et seulement si l'on connaît les périphériques présents.
            if !known.is_empty() && is_inside(&path.to_string_lossy(), &t.loc.obs) {
                let (fixed, replaced) = fix_audio_devices(&out, &known);
                if !replaced.is_empty() {
                    audio.extend(replaced);
                    out = fixed;
                }
            }
            (out != text).then_some(out)
        });
    }
    if !audio.is_empty() {
        warnings.push(format!(
            "{} périphérique(s) audio absent(s) de ce PC remplacé(s) par « Par défaut » : vérifie ton micro dans OBS.",
            audio.len()
        ));
    }
    progress.step(8, 2, &format!("{rewrites} chemin(s) · {} audio", audio.len()));

    // 9. Contrôle : chaque fichier local cité par les scènes existe-t-il ?
    progress.step(9, 1, "contrôle…");
    let mut missing = Vec::new();
    let scenes = t.loc.obs.join(r"basic\scenes");
    for entry in std::fs::read_dir(&scenes).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "json") {
            if let Ok(v) = std::fs::read_to_string(&p).map(|s| serde_json::from_str::<serde_json::Value>(&s)) {
                if let Ok(v) = v {
                    collect_missing(&v, &mut missing);
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        progress.step(9, 2, "tous les fichiers des scènes sont présents");
    } else {
        progress.step(9, 3, &format!("{} fichier(s) manquant(s)", missing.len()));
    }
    Ok(Outcome { warnings, missing, backups, launcher: app_dest.join("blackat-launcher.exe") })
}

use super::is_inside;

fn collect_missing(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            if let Some(p) = local_path(s) {
                if !Path::new(&p).exists() && !p.to_lowercase().starts_with("c:/windows/") {
                    out.push(p);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_missing(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| collect_missing(x, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reecriture_toutes_ecritures() {
        let json = r#"{"url":"file:///C:/Users/Blackat/Documents/x.html","file":"C:\\Users\\Blackat\\Videos\\a.mp3","b":"C:/users/blackat/Pictures/p.png","autre":"C:/Users/Blackat2/z"}"#;
        let (out, n) = rewrite_profile(json, r"C:\Users\Blackat", r"D:\Users\Nouveau");
        assert_eq!(n, 3);
        assert!(out.contains("file:///D:/Users/Nouveau/Documents/x.html"));
        assert!(out.contains(r"D:\\Users\\Nouveau\\Videos\\a.mp3"));
        assert!(out.contains("D:/Users/Nouveau/Pictures/p.png"));
        assert!(out.contains("C:/Users/Blackat2/z"), "un autre profil ne doit pas être touché");
        // JSON toujours valide.
        serde_json::from_str::<serde_json::Value>(&out).unwrap();
    }

    #[test]
    fn meme_profil_rien_a_faire() {
        let (out, n) = rewrite_profile("C:/Users/A/x", r"C:\Users\A", r"c:\users\a");
        assert_eq!((out.as_str(), n), ("C:/Users/A/x", 0));
    }

    #[test]
    fn audio_inconnu_par_defaut() {
        let known = vec!["{0.0.1.00000000}.{AAA}".to_string()];
        let text = "{\n \"device_id\": \"{0.0.1.00000000}.{BBB}\",\n \"device_id\": \"{0.0.1.00000000}.{aaa}\",\n \"device_id\": \"default\"\n}\n[Audio]\r\nMonitoringDeviceId={0.0.0.00000000}.{CCC}\r\n";
        let (out, replaced) = fix_audio_devices(text, &known);
        assert_eq!(replaced, vec!["{0.0.1.00000000}.{BBB}", "{0.0.0.00000000}.{CCC}"]);
        assert!(out.contains("\"device_id\": \"default\",\n"));
        assert!(out.contains("{aaa}"), "un périphérique présent est conservé");
        assert!(out.contains("MonitoringDeviceId=default\r\n"));
    }
}
