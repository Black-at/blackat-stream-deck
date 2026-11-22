//! Plugin « Compteur » : morts, victoires, abonnés du jour…
//!
//! Chaque compteur a un nom ; plusieurs touches peuvent agir sur le même (une « +1 », une « −1 »,
//! une « remise à zéro »). La valeur est gardée dans `%APPDATA%\soomfon-deck\compteurs.json` et
//! écrite aussi dans `%APPDATA%\soomfon-deck\compteurs\<nom>.txt` : dans OBS, une source
//! « Texte (GDI+) » avec « Lire depuis un fichier » l'affiche en direct.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub struct Counters {
    dir: PathBuf,
    values: BTreeMap<String, i64>,
}

/// Nom de fichier sûr pour un compteur (`None` si le nom est vide).
pub fn file_name(name: &str) -> Option<String> {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' })
        .collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

impl Counters {
    /// `dir` = dossier de l'appli (`%APPDATA%\soomfon-deck`).
    pub fn load(dir: &Path) -> Self {
        let values = std::fs::read_to_string(dir.join("compteurs.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self { dir: dir.to_path_buf(), values }
    }

    pub fn get(&self, name: &str) -> i64 {
        self.values.get(name.trim()).copied().unwrap_or(0)
    }

    /// Applique l'appui : `step` ajouté à la valeur, ou remise à zéro si `step == 0`.
    pub fn press(&mut self, name: &str, step: i32) -> i64 {
        let Some(file) = file_name(name) else { return 0 };
        let value = if step == 0 { 0 } else { self.get(name).saturating_add(step as i64) };
        self.values.insert(name.trim().to_string(), value);
        self.save(&file, value);
        value
    }

    fn save(&self, file: &str, value: i64) {
        let folder = self.dir.join("compteurs");
        let result = std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(folder.join(format!("{file}.txt")), value.to_string()))
            .and_then(|()| {
                let json = serde_json::to_string_pretty(&self.values).unwrap_or_default();
                std::fs::write(self.dir.join("compteurs.json"), json)
            });
        if let Err(e) = result {
            log::warn!("Compteur : enregistrement impossible ({e})");
        }
    }
}

/// Libellé du mode dans l'éditeur.
pub fn step_label(step: i32) -> String {
    match step {
        0 => "Remettre à zéro".into(),
        s if s > 0 => format!("Ajouter {s}"),
        s => format!("Retirer {}", -s),
    }
}

/// Modes proposés dans l'éditeur.
pub const STEPS: [i32; 5] = [1, -1, 5, 10, 0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plus_moins_zero_et_fichiers() {
        let dir = std::env::temp_dir().join(format!("compteurs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = Counters::load(&dir);
        assert_eq!(c.press("Morts", 1), 1);
        assert_eq!(c.press("Morts", 1), 2);
        assert_eq!(c.press("Morts", -1), 1);
        assert_eq!(std::fs::read_to_string(dir.join("compteurs/Morts.txt")).unwrap(), "1");
        // Relu depuis le disque.
        assert_eq!(Counters::load(&dir).get("Morts"), 1);
        assert_eq!(c.press("Morts", 0), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn noms_de_fichier() {
        assert_eq!(file_name(" Morts "), Some("Morts".into()));
        assert_eq!(file_name("a/b:c"), Some("a_b_c".into()));
        assert_eq!(file_name("   "), None);
        assert_eq!(step_label(-1), "Retirer 1");
    }
}
