//! Plugin « Raccourci clavier / macro » : la touche envoie une combinaison de touches.
//!
//! Syntaxe (insensible à la casse) : `ctrl+shift+m` pour une combinaison, et plusieurs
//! combinaisons séparées par des virgules pour une petite macro jouée dans l'ordre :
//! `ctrl+c, alt+tab, ctrl+v`. Les touches partent avec leur code matériel (scan code) en plus
//! du code virtuel : les jeux, Discord et OBS les voient comme un vrai clavier.

/// Délai entre deux combinaisons d'une macro (laisse le temps à l'appli de réagir).
const STEP_DELAY_MS: u64 = 60;

/// Une touche reconnue : code virtuel Windows et « touche étendue » (flèches, Inser, Suppr…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub vk: u8,
    pub extended: bool,
}

/// Une combinaison : touches tenues (Ctrl, Maj…) puis touches appuyées.
pub type Combo = Vec<Key>;

const fn k(vk: u8) -> Key {
    Key { vk, extended: false }
}

const fn ext(vk: u8) -> Key {
    Key { vk, extended: true }
}

/// Nom → touche. Plusieurs noms (français / anglais) pour la même touche.
fn named(name: &str) -> Option<Key> {
    Some(match name {
        "ctrl" | "control" | "ctl" => k(0x11),
        "shift" | "maj" => k(0x10),
        "alt" => k(0x12),
        "altgr" => ext(0xA5),
        "win" | "windows" | "super" | "meta" => ext(0x5B),
        "enter" | "entree" | "entrée" | "return" => k(0x0D),
        "esc" | "escape" | "echap" | "échap" => k(0x1B),
        "tab" => k(0x09),
        "space" | "espace" => k(0x20),
        "backspace" | "retour" => k(0x08),
        "delete" | "del" | "suppr" => ext(0x2E),
        "insert" | "ins" | "inser" => ext(0x2D),
        "home" | "debut" | "début" => ext(0x24),
        "end" | "fin" => ext(0x23),
        "pageup" | "pgup" => ext(0x21),
        "pagedown" | "pgdn" => ext(0x22),
        "up" | "haut" => ext(0x26),
        "down" | "bas" => ext(0x28),
        "left" | "gauche" => ext(0x25),
        "right" | "droite" => ext(0x27),
        "printscreen" | "prtsc" | "impr" => ext(0x2C),
        "pause" => k(0x13),
        "capslock" | "verrmaj" => k(0x14),
        "numlock" => ext(0x90),
        "scrolllock" => k(0x91),
        "menu" | "apps" => ext(0x5D),
        "plus" => k(0xBB),
        "minus" | "moins" => k(0xBD),
        "comma" | "virgule" => k(0xBC),
        "period" | "point" => k(0xBE),
        "mute" | "muet" => ext(0xAD),
        "volumeup" => ext(0xAF),
        "volumedown" => ext(0xAE),
        "playpause" => ext(0xB3),
        "next" => ext(0xB0),
        "previous" | "prev" => ext(0xB1),
        _ => return None,
    })
}

fn parse_key(token: &str) -> Result<Key, String> {
    let t = token.trim().to_lowercase();
    if t.is_empty() {
        return Err("touche manquante (deux « + » de suite ?)".into());
    }
    if let Some(key) = named(&t) {
        return Ok(key);
    }
    let chars: Vec<char> = t.chars().collect();
    // Une lettre ou un chiffre.
    if chars.len() == 1 && chars[0].is_ascii_alphanumeric() {
        return Ok(k(chars[0].to_ascii_uppercase() as u8));
    }
    // F1 à F24.
    if let Some(n) = t.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) {
            return Ok(k(0x70 + n - 1));
        }
    }
    // Pavé numérique : num0 à num9.
    if let Some(n) = t.strip_prefix("num").and_then(|n| n.parse::<u8>().ok()) {
        if n <= 9 {
            return Ok(k(0x60 + n));
        }
    }
    Err(format!("touche « {} » inconnue", token.trim()))
}

/// Lit une macro : combinaisons séparées par des virgules, touches séparées par des « + ».
pub fn parse(text: &str) -> Result<Vec<Combo>, String> {
    let mut combos = Vec::new();
    for part in text.split(',') {
        if part.trim().is_empty() {
            continue;
        }
        let combo = part.split('+').map(parse_key).collect::<Result<Combo, String>>()?;
        combos.push(combo);
    }
    if combos.is_empty() {
        return Err("aucune touche (ex. ctrl+shift+m)".into());
    }
    Ok(combos)
}

/// Version lisible pour l'éditeur : `ctrl+shift+m` → `Ctrl + Shift + M`.
pub fn pretty(text: &str) -> String {
    text.split(',')
        .filter(|p| !p.trim().is_empty())
        .map(|combo| {
            combo
                .split('+')
                .map(|t| {
                    let t = t.trim();
                    let mut c = t.chars();
                    match c.next() {
                        Some(first) => first.to_uppercase().chain(c).collect::<String>(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" + ")
        })
        .collect::<Vec<_>>()
        .join("  ›  ")
}

/// Joue la macro dans un fil à part (la boucle du démon n'attend pas les délais).
pub fn send(text: &str) {
    match parse(text) {
        Ok(combos) => {
            std::thread::spawn(move || {
                for (i, combo) in combos.iter().enumerate() {
                    if i > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(STEP_DELAY_MS));
                    }
                    press_combo(combo);
                }
            });
        }
        Err(e) => log::warn!("Raccourci « {text} » : {e}"),
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
}

/// Appuie les touches dans l'ordre, puis les relâche dans l'ordre inverse.
#[cfg(windows)]
fn press_combo(combo: &Combo) {
    const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const MAPVK_VK_TO_VSC: u32 = 0;
    let event = |key: &Key, up: bool| {
        let mut flags = if key.extended { KEYEVENTF_EXTENDEDKEY } else { 0 };
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        // SAFETY : appels Win32 sans pointeur ; simulent l'appui ou le relâchement d'une touche.
        unsafe {
            let scan = MapVirtualKeyW(key.vk as u32, MAPVK_VK_TO_VSC) as u8;
            keybd_event(key.vk, scan, flags, 0);
        }
    };
    for key in combo {
        event(key, false);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    for key in combo.iter().rev() {
        event(key, true);
    }
}

#[cfg(not(windows))]
fn press_combo(_combo: &Combo) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinaison_simple() {
        let c = parse("Ctrl+Shift+M").unwrap();
        assert_eq!(c, vec![vec![k(0x11), k(0x10), k(b'M')]]);
    }

    #[test]
    fn macro_et_touches_speciales() {
        let c = parse("ctrl+c, alt+tab ,f13, win+up, num5").unwrap();
        assert_eq!(c.len(), 5);
        assert_eq!(c[2], vec![k(0x7C)]);
        assert_eq!(c[3], vec![ext(0x5B), ext(0x26)]);
        assert_eq!(c[4], vec![k(0x65)]);
    }

    #[test]
    fn erreurs_lisibles() {
        assert!(parse("").unwrap_err().contains("aucune touche"));
        assert!(parse("ctrl+").unwrap_err().contains("manquante"));
        assert!(parse("ctrl+banane").unwrap_err().contains("« banane » inconnue"));
        assert!(parse("f25").is_err());
    }

    #[test]
    fn affichage() {
        assert_eq!(pretty("ctrl+shift+m"), "Ctrl + Shift + M");
        assert_eq!(pretty("ctrl+c, ctrl+v"), "Ctrl + C  ›  Ctrl + V");
    }
}
