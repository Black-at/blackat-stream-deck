//! Plugin « Lancer » : la touche ouvre un programme, un fichier, un dossier ou un site web.
//!
//! Tout passe par Windows (ShellExecute), exactement comme un double-clic : un `.exe` se lance,
//! un `.mp4` s'ouvre dans le lecteur par défaut, une adresse `https://…` dans le navigateur.
//! Pour un programme, le dossier de travail est celui du programme (OBS en a besoin).

use std::path::Path;

/// Ce que vise la touche, pour choisir un texte et une icône par défaut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Web,
    Program,
    Folder,
    File,
}

pub fn classify(target: &str) -> Target {
    let t = target.trim().to_lowercase();
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.") {
        return Target::Web;
    }
    if Path::new(target.trim()).is_dir() {
        return Target::Folder;
    }
    if [".exe", ".lnk", ".bat", ".cmd", ".url"].iter().any(|e| t.ends_with(e)) {
        return Target::Program;
    }
    Target::File
}

/// Texte court pour la touche : nom du programme, du fichier ou du site.
pub fn short_name(target: &str) -> String {
    let t = target.trim().trim_end_matches(['/', '\\']);
    if classify(t) == Target::Web {
        let host = t.split("://").last().unwrap_or(t).split('/').next().unwrap_or(t);
        let host = host.strip_prefix("www.").unwrap_or(host);
        // twitch.tv → Twitch
        let name = host.split('.').next().unwrap_or(host);
        let mut c = name.chars();
        return c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    }
    let path = Path::new(t);
    path.file_stem().or(path.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| t.to_string())
}

/// Adresse prête à ouvrir : `www.twitch.tv` → `https://www.twitch.tv`.
fn normalized(target: &str) -> String {
    let t = target.trim().trim_matches('"');
    if t.to_lowercase().starts_with("www.") { format!("https://{t}") } else { t.to_string() }
}

/// Ouvre la cible dans un fil à part (Windows peut mettre un instant à répondre).
pub fn open(target: &str, args: &str) {
    let target = normalized(target);
    let args = args.trim().to_string();
    if target.is_empty() {
        log::warn!("Touche « Lancer » sans cible");
        return;
    }
    std::thread::spawn(move || match shell_open(&target, &args) {
        Ok(()) => log::info!("Lancé : {target}"),
        Err(e) => log::warn!("Impossible d'ouvrir « {target} » : {e}"),
    });
}

/// `ShellExecuteW` de shell32.dll.
#[cfg(windows)]
type ShellExecuteFn = unsafe extern "system" fn(isize, *const u16, *const u16, *const u16, *const u16, i32) -> isize;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const core::ffi::c_void;
}

/// shell32.dll pèse ~8 Mo en mémoire : on ne la charge qu'au premier appui sur une touche « Lancer »
/// (l'importer normalement la chargerait au démarrage du démon, même sans touche « Lancer »).
#[cfg(windows)]
fn shell_execute() -> Option<ShellExecuteFn> {
    use std::sync::OnceLock;
    static FN: OnceLock<Option<usize>> = OnceLock::new();
    let addr = FN.get_or_init(|| {
        let name: Vec<u16> = "shell32.dll\0".encode_utf16().collect();
        // SAFETY : chaînes terminées par un zéro ; la bibliothèque reste chargée jusqu'à la fin.
        unsafe {
            let module = LoadLibraryW(name.as_ptr());
            if module == 0 {
                return None;
            }
            let f = GetProcAddress(module, c"ShellExecuteW".as_ptr().cast());
            (!f.is_null()).then_some(f as usize)
        }
    });
    // SAFETY : adresse de ShellExecuteW, dont la signature est celle de ShellExecuteFn.
    addr.map(|a| unsafe { std::mem::transmute::<usize, ShellExecuteFn>(a) })
}

#[cfg(windows)]
fn shell_open(target: &str, args: &str) -> Result<(), String> {
    const SW_SHOWNORMAL: i32 = 1;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let exec = shell_execute().ok_or("shell32.dll introuvable")?;
    let (verb, file, params) = (wide("open"), wide(target), wide(args));
    // Dossier de travail = dossier du programme (sinon certains, comme OBS, ne trouvent pas leurs fichiers).
    let dir = Path::new(target)
        .parent()
        .filter(|p| classify(target) == Target::Program && p.is_dir())
        .map(|p| wide(&p.to_string_lossy()));
    // SAFETY : chaînes UTF-16 terminées par un zéro, valides pendant l'appel ; ShellExecuteW ne les garde pas.
    let code = unsafe {
        exec(
            0,
            verb.as_ptr(),
            file.as_ptr(),
            if args.is_empty() { std::ptr::null() } else { params.as_ptr() },
            dir.as_ref().map(|d| d.as_ptr()).unwrap_or(std::ptr::null()),
            SW_SHOWNORMAL,
        )
    };
    // Valeur > 32 = succès (convention historique de ShellExecute).
    match code {
        c if c > 32 => Ok(()),
        2 => Err("fichier introuvable".into()),
        3 => Err("dossier introuvable".into()),
        5 => Err("accès refusé".into()),
        31 => Err("aucune application associée à ce type de fichier".into()),
        c => Err(format!("erreur Windows {c}")),
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str, _args: &str) -> Result<(), String> {
    Err("disponible seulement sous Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms_courts() {
        assert_eq!(short_name("https://www.twitch.tv/blackat"), "Twitch");
        assert_eq!(short_name("www.youtube.com"), "Youtube");
        assert_eq!(short_name(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe"), "obs64");
        assert_eq!(short_name(r"C:\Clips\gg.mp4"), "gg");
    }

    #[test]
    #[cfg(windows)]
    fn shell32_charge_a_la_demande() {
        assert!(shell_execute().is_some());
    }

    #[test]
    fn types_de_cible() {
        assert_eq!(classify("https://twitch.tv"), Target::Web);
        assert_eq!(classify(r"C:\Jeux\jeu.exe"), Target::Program);
        assert_eq!(classify(r"C:\Clips\gg.mp4"), Target::File);
        assert_eq!(classify(&std::env::temp_dir().to_string_lossy()), Target::Folder);
        assert_eq!(normalized("www.twitch.tv"), "https://www.twitch.tv");
    }
}
