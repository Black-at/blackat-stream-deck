//! Géométrie du boîtier : correspondance entre les positions « logiques » (0..14, lues ligne
//! par ligne de gauche à droite) et les index utilisés par le matériel.
//! Valeurs mesurées avec la sonde — voir docs/MATERIEL.md.

use image::{RgbImage, codecs::jpeg::JpegEncoder, imageops};

/// Nombre de touches physiques.
pub const KEYS: usize = 15;
/// Cases de l'écran latéral (à droite des touches, affichage seul), de haut en bas.
pub const SIDE: usize = 3;
/// Tout ce qu'on dessine : 15 touches (positions 0..14) puis l'écran latéral (15..17).
pub const SLOTS: usize = KEYS + SIDE;
/// Nombre d'index que le protocole connaît (15 touches + 3 cases de l'écran latéral).
pub const DEVICE_KEY_COUNT: usize = 18;
/// Taille d'une touche en pixels.
pub const KEY_PX: u32 = 85;
/// Touche Retour dans les dossiers : en bas à droite.
pub const BACK_POS: usize = 14;

/// Position logique → index image (celui passé à `write_image`).
const POS_TO_IMAGE: [u8; KEYS] = [12, 9, 6, 3, 0, 13, 10, 7, 4, 1, 14, 11, 8, 5, 2];

/// Index image à utiliser pour dessiner la case `pos` (0..14 touches, 15..17 écran latéral,
/// dont les index image valent 15, 16, 17 de haut en bas — relevé du 2026-11-12).
pub fn image_index(pos: usize) -> u8 {
    if pos < KEYS { POS_TO_IMAGE[pos] } else { pos as u8 }
}

/// Convertit l'index brut reçu lors d'un appui en position logique.
/// Renvoie `None` pour les valeurs hors des 15 touches physiques.
pub fn pos_from_raw(raw: u8) -> Option<usize> {
    let image = raw.checked_sub(1)?;
    POS_TO_IMAGE.iter().position(|&i| i == image)
}

/// Prépare une image 85×85 pour le boîtier : rotation 90°, miroir horizontal + vertical, JPEG.
pub fn encode_for_device(img: &RgbImage, quality: u8) -> Vec<u8> {
    let rotated = imageops::rotate90(img);
    let flipped = imageops::flip_vertical(&imageops::flip_horizontal(&rotated));
    let mut jpeg = Vec::with_capacity(4096);
    JpegEncoder::new_with_quality(&mut jpeg, quality)
        .encode_image(&flipped)
        .expect("l'encodage JPEG d'une image en mémoire ne peut pas échouer");
    jpeg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_position_index() {
        for pos in 0..KEYS {
            let raw = image_index(pos) + 1;
            assert_eq!(pos_from_raw(raw), Some(pos));
        }
    }

    #[test]
    fn valeurs_mesurees() {
        // Rangée du haut, de gauche à droite : 13, 10, 7, 4, 1 (relevé de la sonde).
        let top: Vec<_> = [13u8, 10, 7, 4, 1].iter().map(|&r| pos_from_raw(r)).collect();
        assert_eq!(top, vec![Some(0), Some(1), Some(2), Some(3), Some(4)]);
        // Bas à droite = index brut 3 = touche Retour.
        assert_eq!(pos_from_raw(3), Some(BACK_POS));
    }

    #[test]
    fn touches_fantomes_ignorees() {
        assert_eq!(pos_from_raw(0), None);
        assert_eq!(pos_from_raw(16), None);
        assert_eq!(pos_from_raw(17), None);
        assert_eq!(pos_from_raw(18), None);
    }
}
