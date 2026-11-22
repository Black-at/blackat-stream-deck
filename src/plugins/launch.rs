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

#[cfg(windows)]
fn shell_open(target: &str, args: &str) -> Result<(), String> {
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::{HSTRING, PCWSTR, w},
    };
    let file = HSTRING::from(target);
    let params = HSTRING::from(args);
    // Dossier de travail = dossier du programme (sinon certains, comme OBS, ne trouvent pas leurs fichiers).
    let dir = Path::new(target)
        .parent()
        .filter(|p| classify(target) == Target::Program && p.is_dir())
        .map(|p| HSTRING::from(p.as_os_str()));
    // SAFETY : chaînes valides pendant l'appel ; ShellExecuteW ne les garde pas.
    let code = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            if args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
            dir.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        )
    };
    // Valeur > 32 = succès (convention historique de ShellExecute).
    let code = code.0 as isize;
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
    fn types_de_cible() {
        assert_eq!(classify("https://twitch.tv"), Target::Web);
        assert_eq!(classify(r"C:\Jeux\jeu.exe"), Target::Program);
        assert_eq!(classify(r"C:\Clips\gg.mp4"), Target::File);
        assert_eq!(classify(&std::env::temp_dir().to_string_lossy()), Target::Folder);
        assert_eq!(normalized("www.twitch.tv"), "https://www.twitch.tv");
    }
}
