//! Plugin « Taper un texte » : la touche écrit un message tout prêt là où se trouve le curseur
//! (chat Twitch, Discord, jeu…), puis appuie sur Entrée si demandé.
//!
//! Les caractères partent un par un en Unicode : accents et émojis passent, quelle que soit la
//! disposition du clavier.

/// Écrit le texte dans un fil à part (la boucle du démon n'attend pas).
pub fn type_text(text: &str, enter: bool) {
    if text.is_empty() && !enter {
        log::warn!("Touche « Taper un texte » vide");
        return;
    }
    let text = text.to_string();
    std::thread::spawn(move || {
        send(&text, enter);
        log::info!("Texte tapé ({} caractères{})", text.chars().count(), if enter { " + Entrée" } else { "" });
    });
}

/// Ce qui part au clavier : unités UTF-16 du texte, retours à la ligne transformés en Entrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stroke {
    Unit(u16),
    Enter,
}

fn strokes(text: &str, enter: bool) -> Vec<Stroke> {
    let mut out = Vec::new();
    for line in text.replace("\r\n", "\n").split_inclusive('\n') {
        let (body, newline) = match line.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (line, false),
        };
        out.extend(body.encode_utf16().map(Stroke::Unit));
        if newline {
            out.push(Stroke::Enter);
        }
    }
    if enter {
        out.push(Stroke::Enter);
    }
    out
}

#[cfg(windows)]
fn send(text: &str, enter: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
        VIRTUAL_KEY, VK_RETURN,
    };
    let key = |vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    for s in strokes(text, enter) {
        let pair = match s {
            Stroke::Unit(u) => {
                [key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE), key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]
            }
            Stroke::Enter => [key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)), key(VK_RETURN, 0, KEYEVENTF_KEYUP)],
        };
        // SAFETY : tableau d'entrées clavier valide pendant l'appel.
        unsafe {
            SendInput(&pair, std::mem::size_of::<INPUT>() as i32);
        }
        // Un léger délai : certains jeux et le chat Twitch perdent des lettres envoyées trop vite.
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

#[cfg(not(windows))]
fn send(_text: &str, _enter: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texte_accents_et_entree() {
        assert_eq!(strokes("é", true), vec![Stroke::Unit(0xE9), Stroke::Enter]);
        assert_eq!(strokes("a\nb", false), vec![Stroke::Unit(b'a' as u16), Stroke::Enter, Stroke::Unit(b'b' as u16)]);
        // Émoji = deux unités UTF-16.
        assert_eq!(strokes("🔥", false).len(), 2);
        assert!(strokes("", false).is_empty());
    }
}
