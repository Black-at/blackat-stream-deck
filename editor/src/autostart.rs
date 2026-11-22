//! « Démarrer avec Windows » : entrée `Soomfon Deck` dans les applis de démarrage de la session
//! (`HKCU\…\Run`, visible et désactivable dans Gestionnaire des tâches → Applications de démarrage).
//!
//! Le logiciel officiel SOOMFON (« Stream Controller ») se lance aussi avec Windows et se dispute
//! le boîtier avec soomfon-deck : on propose de le désactiver comme le fait le Gestionnaire des
//! tâches (`StartupApproved`), sans rien supprimer — réactivable depuis le Gestionnaire des tâches.

use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "Soomfon Deck";
const OFFICIAL: &str = "Stream Controller";

fn reg(args: &[&str]) -> Option<String> {
    let out = Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Valeur d'une entrée (`reg query` : « Nom    TYPE    valeur »).
fn query(key: &str, name: &str) -> Option<String> {
    let text = reg(&["query", key, "/v", name])?;
    let line = text.lines().find(|l| l.trim_start().starts_with(name))?;
    let rest = line.trim_start()[name.len()..].trim_start();
    let value = rest.split_once(char::is_whitespace).map(|(_, v)| v.trim()).unwrap_or_default();
    Some(value.to_string())
}

/// Le démon, à côté de l'éditeur.
pub fn daemon_exe() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("soomfon-deck.exe"))).unwrap_or_default()
}

/// Activé = entrée présente, pointant sur ce démon, et non désactivée dans le Gestionnaire des tâches.
pub fn enabled() -> bool {
    let Some(value) = query(RUN, NAME) else { return false };
    let points_here = value.trim_matches('"').eq_ignore_ascii_case(&daemon_exe().to_string_lossy());
    points_here && !disabled_in_task_manager(NAME)
}

pub fn set(on: bool) -> Result<(), String> {
    let ok = if on {
        let exe = daemon_exe();
        if !exe.is_file() {
            return Err(format!("{} introuvable", exe.display()));
        }
        let value = format!("\"{}\"", exe.display());
        reg(&["add", RUN, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"]).is_some()
            // Réactive l'entrée si elle avait été désactivée dans le Gestionnaire des tâches.
            && (query(APPROVED, NAME).is_none() || reg(&["delete", APPROVED, "/v", NAME, "/f"]).is_some())
    } else {
        query(RUN, NAME).is_none() || reg(&["delete", RUN, "/v", NAME, "/f"]).is_some()
    };
    if ok { Ok(()) } else { Err("réglage de démarrage refusé par Windows".into()) }
}

/// Octet impair en tête de la valeur `StartupApproved` = désactivé (convention du Gestionnaire des tâches).
fn disabled_in_task_manager(name: &str) -> bool {
    query(APPROVED, name).and_then(|v| u8::from_str_radix(v.get(0..2)?, 16).ok()).is_some_and(|b| b & 1 == 1)
}

/// Le logiciel officiel démarre-t-il avec Windows ?
pub fn official_enabled() -> bool {
    query(RUN, OFFICIAL).is_some() && !disabled_in_task_manager(OFFICIAL)
}

/// Désactive le démarrage du logiciel officiel, exactement comme « Désactiver » dans le
/// Gestionnaire des tâches (rien n'est supprimé).
pub fn disable_official() -> Result<(), String> {
    reg(&["add", APPROVED, "/v", OFFICIAL, "/t", "REG_BINARY", "/d", "030000000000000000000000", "/f"])
        .map(|_| ())
        .ok_or_else(|| "réglage refusé par Windows".into())
}
