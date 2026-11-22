//! Création du pack Blackat (bouton « Sauvegarder » du launcher).
//!
//! 1. Collecte dans une archive chiffrée (AES-256) : config OBS sans les caches, plugins OBS
//!    tiers, Streamer.bot sans son cache WebView2, overlays, config Soomfon, et les fichiers
//!    utilisés par les scènes OBS mais rangés ailleurs (captures, sons, vidéos…).
//! 2. Ajoute l'installeur officiel d'OBS de la même version (téléchargé une fois, gardé en cache).
//! 3. Emballe le tout dans un zip normal avec le launcher, le Stream Deck et l'éditeur.
//!
//! Un fichier verrouillé (programme ouvert) est signalé et ignoré, sans arrêter la sauvegarde.

use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufWriter, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};

use zip::{AesMode, CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::{
    CACHE_DIRS, DATA_FILE, FORMAT, Locations, MANIFEST_FILE, Manifest, MediaEntry, SectionInfo, is_inside, local_path,
    now_utc,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Au-delà, un fichier média n'est pas embarqué (signalé dans le bilan).
const MEDIA_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Étapes affichées dans le launcher.
pub const STEPS: [&str; 8] = [
    "Configuration OBS",
    "Plugins OBS",
    "Streamer.bot",
    "Overlays",
    "Médias des scènes",
    "Stream Deck (Soomfon)",
    "Installeur OBS",
    "Création du pack",
];

pub struct Options {
    pub out_dir: PathBuf,
    pub password: String,
    /// Sauvegarder même si OBS ou Streamer.bot sont ouverts.
    pub force: bool,
}

/// Progression : (étape, état 1 = en cours / 2 = fini / 3 = erreur, détail).
pub trait Progress {
    fn step(&self, index: usize, state: i32, detail: &str);
}

pub struct Outcome {
    pub pack: PathBuf,
    pub manifest: Manifest,
}

pub fn is_running(process: &str) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {process}"), "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&process.to_lowercase()))
        .unwrap_or(false)
}

/// Écrivain de l'archive chiffrée, avec statistiques par section.
struct DataZip<'a> {
    zip: ZipWriter<BufWriter<File>>,
    options: SimpleFileOptions,
    password: &'a str,
    warnings: Vec<String>,
    files: usize,
    bytes: u64,
}

impl<'a> DataZip<'a> {
    fn new(path: &Path, password: &'a str) -> io::Result<Self> {
        Ok(Self {
            zip: ZipWriter::new(BufWriter::new(File::create(path)?)),
            options: SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(true),
            password,
            warnings: Vec::new(),
            files: 0,
            bytes: 0,
        })
    }

    /// Ajoute un fichier. Un fichier illisible (verrouillé…) devient un avertissement.
    fn add_file(&mut self, src: &Path, name: &str) -> io::Result<()> {
        let mut file = match File::open(src) {
            Ok(f) => f,
            Err(e) => {
                self.warnings.push(format!("Fichier ignoré (illisible) : {} — {e}", src.display()));
                return Ok(());
            }
        };
        let options = self.options.with_aes_encryption(AesMode::Aes256, self.password);
        self.zip.start_file(name, options).map_err(io::Error::other)?;
        let n = io::copy(&mut file, &mut self.zip)?;
        self.files += 1;
        self.bytes += n;
        Ok(())
    }

    /// Ajoute un dossier entier sous `prefix`, sans les dossiers que `skip` refuse.
    fn add_dir(&mut self, root: &Path, prefix: &str, skip: &dyn Fn(&Path, &str) -> bool) -> io::Result<()> {
        fn walk(
            z: &mut DataZip,
            root: &Path,
            dir: &Path,
            prefix: &str,
            skip: &dyn Fn(&Path, &str) -> bool,
        ) -> io::Result<()> {
            let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let path = entry.path();
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    if CACHE_DIRS.contains(&name.as_str()) || skip(&path, &rel) {
                        continue;
                    }
                    walk(z, root, &path, prefix, skip)?;
                } else if !skip(&path, &rel) {
                    z.add_file(&path, &format!("{prefix}/{rel}"))?;
                }
            }
            Ok(())
        }
        walk(self, root, root, prefix, skip)
    }

    fn section_start(&mut self) -> (usize, u64) {
        (self.files, self.bytes)
    }

    fn section_end(&self, name: &str, start: (usize, u64)) -> SectionInfo {
        SectionInfo { name: name.into(), files: self.files - start.0, bytes: self.bytes - start.1 }
    }
}

fn human(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} Mo", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} Ko", bytes.div_ceil(1024))
    }
}

/// Version d'OBS d'après son journal le plus récent (« OBS 32.2.2 »).
fn obs_version(obs: &Path) -> Option<String> {
    let logs = obs.join("logs");
    let newest = std::fs::read_dir(&logs)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
    // Lecture tolérante : le journal peut contenir des noms de périphériques non UTF-8.
    let text = String::from_utf8_lossy(&std::fs::read(newest.path()).ok()?).into_owned();
    text.lines().find_map(|l| {
        let i = l.find("OBS ")?;
        let v: String = l[i + 4..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        (v.matches('.').count() >= 1).then_some(v)
    })
}

/// Fichiers locaux cités par les collections de scènes OBS (hors dossiers déjà sauvegardés).
fn scene_media(loc: &Locations) -> Vec<(String, PathBuf)> {
    fn collect(v: &serde_json::Value, out: &mut BTreeSet<String>) {
        match v {
            serde_json::Value::String(s) => {
                if let Some(p) = local_path(s) {
                    out.insert(p);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| collect(x, out)),
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    let scenes = loc.obs.join(r"basic\scenes");
    for entry in std::fs::read_dir(&scenes).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "json") {
            if let Ok(v) = std::fs::read_to_string(&path).map(|t| serde_json::from_str::<serde_json::Value>(&t)) {
                if let Ok(v) = v {
                    collect(&v, &mut found);
                }
            }
        }
    }
    let covered = [&loc.obs, &loc.streamerbot, &loc.overlays, &loc.soomfon];
    found
        .into_iter()
        .filter(|p| !covered.iter().any(|root| is_inside(p, root)))
        .filter(|p| !p.to_lowercase().starts_with("c:/windows/"))
        .map(|p| {
            let path = PathBuf::from(&p);
            (p, path)
        })
        .filter(|(_, path)| path.is_file())
        .collect()
}

/// Dossier de cache des installeurs : `%LOCALAPPDATA%\Blackat\cache`.
fn installer_cache() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default().join(r"Blackat\cache")
}

/// Installeur officiel d'OBS pour cette version (téléchargé une fois avec curl.exe de Windows).
fn obs_installer(version: &str) -> Result<PathBuf, String> {
    let name = format!("OBS-Studio-{version}-Windows-x64-Installer.exe");
    let dir = installer_cache();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&name);
    let big_enough = |p: &Path| std::fs::metadata(p).map(|m| m.len() > 50 * 1024 * 1024).unwrap_or(false);
    if big_enough(&path) {
        return Ok(path);
    }
    let url = format!("https://github.com/obsproject/obs-studio/releases/download/{version}/{name}");
    let tmp = path.with_extension("part");
    let status = Command::new("curl.exe")
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&tmp)
        .arg(&url)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("curl introuvable : {e}"))?;
    if !status.success() || !big_enough(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("téléchargement impossible ({url})"));
    }
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Dossier des icônes du Stream Deck : `resources\icons` à côté du programme ou au-dessus.
fn icons_dir(exe_dir: &Path) -> Option<PathBuf> {
    exe_dir.ancestors().take(4).map(|d| d.join(r"resources\icons")).find(|p| p.is_dir())
}

const README: &str = "\
BLACKAT — PACK DE CONFIGURATION DE STREAM
==========================================

1. Clic droit sur ce .zip → « Extraire tout… ».
2. Ouvre le dossier extrait et lance « Blackat Launcher.exe ».
3. Clique sur « Installer » et saisis le mot de passe choisi à la sauvegarde.

Le launcher installe OBS (une demande d'autorisation Windows), puis remet en place :
la configuration OBS (profils, scènes, sources), les plugins OBS, Streamer.bot,
les overlays, les fichiers utilisés par les scènes et le Stream Deck.
Une configuration déjà présente sur ce PC est mise de côté (copie .bak) avant d'être remplacée.

Le fichier data.blackat est chiffré (AES-256) : il contient ta clé de stream et tes
connexions (Twitch…). Garde ce pack en lieu sûr et ne le partage pas.
";

/// Crée le pack. Renvoie son chemin et le manifeste.
pub fn run(opts: &Options, progress: &dyn Progress) -> Result<Outcome, String> {
    if opts.password.chars().count() < 8 {
        return Err("mot de passe trop court (8 caractères minimum)".into());
    }
    if !opts.force {
        let open: Vec<&str> = [("obs64.exe", "OBS"), ("Streamer.bot.exe", "Streamer.bot")]
            .into_iter()
            .filter(|(p, _)| is_running(p))
            .map(|(_, n)| n)
            .collect();
        if !open.is_empty() {
            return Err(format!("ferme d'abord {} pour une copie fiable", open.join(" et ")));
        }
    }

    let loc = Locations::current();
    let (date, created) = now_utc();
    std::fs::create_dir_all(&opts.out_dir).map_err(|e| format!("dossier de sortie : {e}"))?;
    let data_path = opts.out_dir.join(format!(".{DATA_FILE}.tmp"));
    let mut z = DataZip::new(&data_path, &opts.password).map_err(|e| e.to_string())?;
    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    let io_err = |e: io::Error| e.to_string();

    // 0. Configuration OBS
    progress.step(0, 1, "copie…");
    let s = z.section_start();
    if loc.obs.is_dir() {
        let skip = |_: &Path, rel: &str| {
            let top = rel.split('/').next().unwrap_or("");
            ["logs", "crashes", "updates", "profiler_data"].contains(&top) || rel.ends_with(".tmp")
        };
        z.add_dir(&loc.obs, "obs", &skip).map_err(io_err)?;
    } else {
        warnings.push("OBS : dossier de configuration introuvable".into());
    }
    let info = z.section_end("obs", s);
    progress.step(0, 2, &format!("{} fichiers · {}", info.files, human(info.bytes)));
    sections.push(info);

    // 1. Plugins OBS tiers (C:\ProgramData\obs-studio\plugins)
    progress.step(1, 1, "copie…");
    let s = z.section_start();
    let mut plugins = Vec::new();
    for entry in std::fs::read_dir(&loc.obs_plugins).into_iter().flatten().flatten() {
        if entry.path().is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            z.add_dir(&entry.path(), &format!("obs-plugins/{name}"), &|_, _| false).map_err(io_err)?;
            plugins.push(name);
        }
    }
    let info = z.section_end("obs-plugins", s);
    progress.step(1, 2, &if plugins.is_empty() { "aucun".to_string() } else { plugins.join(", ") });
    sections.push(info);

    // 2. Streamer.bot (dossier portable, sans son cache WebView2 ni ses journaux)
    progress.step(2, 1, "copie…");
    let s = z.section_start();
    if loc.streamerbot.is_dir() {
        let skip = |_: &Path, rel: &str| {
            let top = rel.split('/').next().unwrap_or("");
            top == "Streamer.bot.exe.WebView2" || top == "logs"
        };
        z.add_dir(&loc.streamerbot, "streamerbot", &skip).map_err(io_err)?;
    } else {
        warnings.push("Streamer.bot : dossier introuvable".into());
    }
    let info = z.section_end("streamerbot", s);
    progress.step(2, 2, &format!("{} fichiers · {}", info.files, human(info.bytes)));
    sections.push(info);

    // 3. Overlays (origine + copie Studio, historique Git compris)
    progress.step(3, 1, "copie…");
    let s = z.section_start();
    if loc.overlays.is_dir() {
        let skip = |_: &Path, rel: &str| rel.split('/').any(|part| part == ".claude");
        z.add_dir(&loc.overlays, "overlays", &skip).map_err(io_err)?;
    } else {
        warnings.push("Overlays : dossier Blackat-Overlays introuvable".into());
    }
    let info = z.section_end("overlays", s);
    progress.step(3, 2, &format!("{} fichiers · {}", info.files, human(info.bytes)));
    sections.push(info);

    // 4. Médias utilisés par les scènes, rangés ailleurs
    progress.step(4, 1, "recherche…");
    let s = z.section_start();
    let mut media = Vec::new();
    for (i, (original, path)) in scene_media(&loc).into_iter().enumerate() {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MEDIA_MAX_BYTES {
            warnings.push(format!("Média trop gros, non embarqué : {original} ({})", human(size)));
            continue;
        }
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "fichier".into());
        let pack = format!("media/{:03}/{file_name}", i + 1);
        z.add_file(&path, &pack).map_err(io_err)?;
        media.push(MediaEntry { original, pack });
    }
    let info = z.section_end("media", s);
    progress.step(4, 2, &format!("{} fichier(s) · {}", media.len(), human(info.bytes)));
    sections.push(info);

    // 5. Stream Deck : config Soomfon (le journal n'est pas utile)
    progress.step(5, 1, "copie…");
    let s = z.section_start();
    let config = loc.soomfon.join("config.toml");
    if config.is_file() {
        z.add_file(&config, "soomfon/config.toml").map_err(io_err)?;
    }
    let info = z.section_end("soomfon", s);
    progress.step(5, 2, if info.files > 0 { "config.toml" } else { "aucune config" });
    sections.push(info);

    warnings.append(&mut z.warnings);
    z.zip.finish().map_err(|e| e.to_string())?.flush().map_err(io_err)?;

    // 6. Installeur OBS
    progress.step(6, 1, "préparation…");
    let version = obs_version(&loc.obs);
    let installer = match version.as_deref() {
        Some(v) => match obs_installer(v) {
            Ok(p) => {
                progress.step(6, 2, &format!("OBS {v} · {}", human(std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0))));
                Some(p)
            }
            Err(e) => {
                warnings.push(format!("Installeur OBS non inclus ({e}) : OBS sera installé par winget (Internet requis)"));
                progress.step(6, 3, "non inclus (winget à l'installation)");
                None
            }
        },
        None => {
            warnings.push("Version d'OBS inconnue : OBS sera installé par winget (Internet requis)".into());
            progress.step(6, 3, "version inconnue");
            None
        }
    };

    let manifest = Manifest {
        format: FORMAT,
        created,
        user_profile: loc.user_profile.to_string_lossy().to_string(),
        appdata: loc.appdata.to_string_lossy().to_string(),
        obs_version: version,
        obs_installer: installer.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()),
        obs_plugins: plugins,
        sections,
        media,
        warnings,
    };

    // 7. Pack final (zip normal)
    progress.step(7, 1, "compression…");
    let pack = opts.out_dir.join(format!("Blackat-Pack-{date}.zip"));
    let result = write_outer(&pack, &data_path, installer.as_deref(), &manifest);
    let _ = std::fs::remove_file(&data_path);
    result?;
    let size = std::fs::metadata(&pack).map(|m| m.len()).unwrap_or(0);
    progress.step(7, 2, &human(size));
    Ok(Outcome { pack, manifest })
}

fn write_outer(pack: &Path, data: &Path, installer: Option<&Path>, manifest: &Manifest) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| e.to_string();
    let exe = std::env::current_exe().map_err(|e| err(&e))?;
    let exe_dir = exe.parent().ok_or("dossier du launcher introuvable")?.to_path_buf();
    let mut zip = ZipWriter::new(BufWriter::new(File::create(pack).map_err(|e| err(&e))?));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(true);
    // Fichiers déjà compressés (exe, installeur, archive chiffrée) : stockés tels quels, plus rapide.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).large_file(true);

    let add = |zip: &mut ZipWriter<BufWriter<File>>, src: &Path, name: &str, opts: SimpleFileOptions| -> Result<(), String> {
        let mut f = File::open(src).map_err(|e| format!("{} : {e}", src.display()))?;
        zip.start_file(name, opts).map_err(|e| err(&e))?;
        io::copy(&mut f, zip).map_err(|e| err(&e))?;
        Ok(())
    };

    add(&mut zip, &exe, "Blackat Launcher.exe", deflate)?;
    zip.start_file("LISEZMOI.txt", deflate).map_err(|e| err(&e))?;
    zip.write_all(README.replace('\n', "\r\n").as_bytes()).map_err(|e| err(&e))?;
    zip.start_file(MANIFEST_FILE, deflate).map_err(|e| err(&e))?;
    zip.write_all(serde_json::to_string_pretty(manifest).map_err(|e| err(&e))?.as_bytes()).map_err(|e| err(&e))?;

    // Programmes Blackat (installés dans %LOCALAPPDATA%\Programs\Blackat à la restauration)
    for name in ["blackat-launcher.exe", "soomfon-deck.exe", "soomfon-editor.exe"] {
        let src = exe_dir.join(name);
        if src.is_file() {
            add(&mut zip, &src, &format!("app/{name}"), deflate)?;
        }
    }
    if let Some(icons) = icons_dir(&exe_dir) {
        let mut stack = vec![icons.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p.strip_prefix(&icons).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    add(&mut zip, &p, &format!("app/resources/icons/{rel}"), deflate)?;
                }
            }
        }
    }
    if let Some(inst) = installer {
        let name = inst.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        add(&mut zip, inst, &format!("installers/{name}"), stored)?;
    }
    add(&mut zip, data, super::DATA_FILE, stored)?;
    zip.finish().map_err(|e| err(&e))?.flush().map_err(|e| err(&e))?;
    Ok(())
}
