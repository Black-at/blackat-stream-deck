//! Dessin des touches 85×85 : fond, icône (SVG ou PNG), texte, surbrillance.
//!
//! Chaque apparence (`KeyVisual`) est dessinée puis encodée en JPEG **une seule fois** :
//! le résultat est gardé en cache, si bien qu'un changement de page déjà vue ne coûte
//! que l'envoi USB.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use image::{RgbImage, RgbaImage, imageops::FilterType};
use resvg::{tiny_skia, usvg};

use crate::layout::{KEY_PX, encode_for_device};

/// Octets JPEG prêts à être envoyés au boîtier.
pub type KeyImage = Arc<[u8]>;
/// Image RGB non encodée (aperçus de l'éditeur).
pub type PreviewImage = RgbImage;

/// Apparence d'une touche. Sert aussi de clé de cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct KeyVisual {
    pub bg: [u8; 3],
    pub icon: Option<String>,
    pub label: Option<String>,
    /// Texte en grand (chrono), à la place du libellé.
    pub big_text: Option<String>,
    /// Contour clair (scène active…).
    pub highlight: bool,
    /// Assombrie (OBS déconnecté…).
    pub dim: bool,
    /// Bloc de texte sur plusieurs lignes (écran latéral : musique, heure…). S'il est présent,
    /// il remplace libellé et grand texte ; l'icône, petite, se place au-dessus.
    pub lines: Vec<TextLine>,
    /// Cadran circulaire (Coil), dessiné sous le texte.
    pub dial: Option<Dial>,
}

/// Cadran de 60 graduations à la façon de l'appli Coil.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dial {
    /// Graduations allumées (0 à 60), dans le sens des aiguilles d'une montre depuis midi.
    pub lit: u8,
    pub color: [u8; 3],
    /// Braises qui montent (0 = aucune ; sinon intensité 1..=255).
    pub embers: u8,
    /// Étape d'animation des braises (change à chaque seconde).
    pub phase: u8,
}

/// Une ligne (ou un paragraphe coupé sur `max_lines` lignes) du bloc de texte.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextLine {
    pub text: String,
    pub px: u8,
    pub color: [u8; 3],
    pub max_lines: u8,
}

impl TextLine {
    pub fn new(text: impl Into<String>, px: u8, color: [u8; 3], max_lines: u8) -> Self {
        Self { text: text.into(), px, color, max_lines }
    }
}

impl KeyVisual {
    /// Touche vide (noire).
    pub fn blank() -> Self {
        Self::default()
    }
}

enum Icon {
    Svg(usvg::Tree),
    Raster(RgbaImage),
}

pub struct Renderer {
    icons_dir: PathBuf,
    quality: u8,
    /// Police chargée telle quelle : les glyphes sont décodés seulement quand on les dessine.
    font: Option<FontVec>,
    icons: HashMap<String, Option<Icon>>,
    cache: HashMap<KeyVisual, KeyImage>,
}

/// Au-delà, on vide le cache (borne la mémoire ; ~2 Ko par entrée).
const CACHE_LIMIT: usize = 400;
const LABEL_PX: f32 = 15.0;
const BIG_TEXT_PX: f32 = 20.0;

impl Renderer {
    pub fn new(icons_dir: PathBuf, quality: u8) -> Self {
        Self { icons_dir, quality, font: load_font(), icons: HashMap::new(), cache: HashMap::new() }
    }

    /// Vide tous les caches (après un rechargement de config : les icônes ont pu changer).
    pub fn reset(&mut self, icons_dir: PathBuf, quality: u8) {
        self.icons_dir = icons_dir;
        self.quality = quality;
        self.icons.clear();
        self.cache.clear();
    }

    /// JPEG de la touche, depuis le cache si possible.
    pub fn render(&mut self, visual: &KeyVisual) -> KeyImage {
        if let Some(img) = self.cache.get(visual) {
            return img.clone();
        }
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        let rgb = self.draw(visual);
        let jpeg: KeyImage = encode_for_device(&rgb, self.quality).into();
        self.cache.insert(visual.clone(), jpeg.clone());
        jpeg
    }

    /// Image RGB 85×85 non encodée (aperçus de l'éditeur).
    pub fn preview(&mut self, v: &KeyVisual) -> RgbImage {
        self.draw(v)
    }

    fn draw(&mut self, v: &KeyVisual) -> RgbImage {
        let size = KEY_PX;
        let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("taille non nulle");

        // Fond, avec contour clair si surbrillance.
        if v.highlight {
            pixmap.fill(tiny_skia::Color::from_rgba8(245, 245, 245, 255));
            let bg = lighten(v.bg, 0.25);
            fill_rect(&mut pixmap, 5.0, 5.0, size as f32 - 10.0, size as f32 - 10.0, bg);
        } else {
            pixmap.fill(tiny_skia::Color::from_rgba8(v.bg[0], v.bg[1], v.bg[2], 255));
        }

        let text = v.big_text.as_deref().or(v.label.as_deref());

        if let Some(dial) = &v.dial {
            draw_dial(&mut pixmap, dial);
        }
        if !v.lines.is_empty() {
            self.draw_block(&mut pixmap, v);
        } else if let Some(path) = &v.icon {
            // Icône : centrée, plus petite s'il y a du texte dessous.
            let (icon_size, top) = match (text, v.big_text.is_some()) {
                (None, _) => (58.0, (size as f32 - 58.0) / 2.0),
                (Some(_), true) => (34.0, 10.0),
                (Some(_), false) => (46.0, 9.0),
            };
            let left = (size as f32 - icon_size) / 2.0;
            self.draw_icon(&mut pixmap, path, left, top, icon_size);
        }

        // Texte.
        if let (true, Some(text), Some(font)) = (v.lines.is_empty(), text, &self.font) {
            let px = if v.big_text.is_some() { BIG_TEXT_PX } else { LABEL_PX };
            let baseline = if v.icon.is_some() {
                size as f32 - 10.0
            } else {
                // Pas d'icône : texte centré verticalement.
                size as f32 / 2.0 + px / 3.0
            };
            draw_text(&mut pixmap, font, text, px, baseline, [255, 255, 255]);
        }

        if v.dim {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(0, 0, 0, 150);
            let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, size as f32, size as f32).unwrap();
            pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        }

        // Pixmap opaque → RGB (les valeurs prémultipliées sont exactes quand alpha = 255).
        let mut rgb = RgbImage::new(size, size);
        for (dst, src) in rgb.pixels_mut().zip(pixmap.pixels()) {
            *dst = image::Rgb([src.red(), src.green(), src.blue()]);
        }
        rgb
    }

    /// Icône (petite) puis lignes de texte, le tout centré verticalement.
    fn draw_block(&mut self, pixmap: &mut tiny_skia::Pixmap, v: &KeyVisual) {
        let Some(font) = self.font.take() else { return };
        let max_w = KEY_PX as f32 - 8.0;
        let wrapped: Vec<(Vec<String>, &TextLine)> =
            v.lines.iter().map(|l| (wrap(&font, &l.text, l.px as f32, max_w, l.max_lines.max(1) as usize), l)).collect();
        const ICON: f32 = 24.0;
        const GAP: f32 = 4.0;
        let line_h = |px: u8| px as f32 * 1.18;
        let text_h: f32 = wrapped.iter().map(|(rows, l)| rows.len() as f32 * line_h(l.px)).sum();
        let total = text_h + if v.icon.is_some() { ICON + GAP } else { 0.0 };
        let mut y = ((KEY_PX as f32 - total) / 2.0).max(2.0);
        if let Some(path) = v.icon.clone() {
            self.draw_icon(pixmap, &path, (KEY_PX as f32 - ICON) / 2.0, y, ICON);
            y += ICON + GAP;
        }
        for (rows, l) in &wrapped {
            for row in rows {
                // Ligne de base ≈ 80 % de la hauteur de ligne.
                draw_text(pixmap, &font, row, l.px as f32, y + line_h(l.px) * 0.8, l.color);
                y += line_h(l.px);
            }
        }
        self.font = Some(font);
    }

    fn draw_icon(&mut self, pixmap: &mut tiny_skia::Pixmap, path: &str, x: f32, y: f32, size: f32) {
        if !self.icons.contains_key(path) {
            let icon = load_icon(&resolve(&self.icons_dir, path));
            self.icons.insert(path.to_string(), icon);
        }
        match self.icons.get(path) {
            Some(Some(Icon::Svg(tree))) => {
                let s = tree.size();
                let scale = size / s.width().max(s.height());
                let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(x, y);
                resvg::render(tree, transform, &mut pixmap.as_mut());
            }
            Some(Some(Icon::Raster(img))) => {
                let px = size.round() as u32;
                let resized = image::imageops::resize(img, px, px, FilterType::Triangle);
                if let Some(src) = rgba_to_pixmap(&resized) {
                    pixmap.draw_pixmap(
                        x.round() as i32,
                        y.round() as i32,
                        src.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        None,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Liste les icônes disponibles (SVG, PNG, JPG), en chemins relatifs « dossier/nom.ext ».
pub fn list_icons(icons_dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| ["svg", "png", "jpg", "jpeg"].contains(&e.to_ascii_lowercase().as_str()))
            {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(icons_dir, icons_dir, &mut out);
    out.sort();
    out
}

fn resolve(icons_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() { p.to_path_buf() } else { icons_dir.join(p) }
}

fn load_icon(path: &Path) -> Option<Icon> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Icône introuvable {} : {e}", path.display());
            return None;
        }
    };
    let is_svg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let icon = if is_svg {
        usvg::Tree::from_data(&data, &usvg::Options::default()).map(Icon::Svg).map_err(|e| e.to_string())
    } else {
        image::load_from_memory(&data).map(|i| Icon::Raster(i.into_rgba8())).map_err(|e| e.to_string())
    };
    icon.inspect_err(|e| log::warn!("Icône illisible {} : {e}", path.display())).ok()
}

/// Police des touches : Inter Bold, intégrée à l'exécutable (même police que l'éditeur).
const INTER_BOLD: &[u8] = include_bytes!("../resources/fonts/Inter-Bold.ttf");

fn load_font() -> Option<FontVec> {
    match FontVec::try_from_vec(INTER_BOLD.to_vec()) {
        Ok(font) => return Some(font),
        Err(e) => log::warn!("Police Inter illisible ({e}) : police de Windows utilisée"),
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    for name in ["segoeuib.ttf", "arialbd.ttf", "segoeui.ttf", "arial.ttf"] {
        let path = Path::new(&windir).join("Fonts").join(name);
        if let Ok(data) = std::fs::read(&path) {
            match FontVec::try_from_vec(data) {
                Ok(font) => return Some(font),
                Err(e) => log::warn!("Police {} illisible : {e}", path.display()),
            }
        }
    }
    log::warn!("Aucune police trouvée : les touches n'auront pas de texte");
    None
}

fn fill_rect(pixmap: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3]) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
}

pub fn lighten(rgb: [u8; 3], amount: f32) -> [u8; 3] {
    rgb.map(|c| (c as f32 + (255.0 - c as f32) * amount).round() as u8)
}

fn rgba_to_pixmap(img: &RgbaImage) -> Option<tiny_skia::Pixmap> {
    let mut data = Vec::with_capacity(img.as_raw().len());
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        let pm = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
        data.extend_from_slice(&[pm(r), pm(g), pm(b), a]);
    }
    tiny_skia::Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(img.width(), img.height())?)
}

/// Échelle ab_glyph correspondant à une taille de police `em_px` (taille « classique » en pixels).
fn em_scale(font: &FontVec, em_px: f32) -> PxScale {
    let units = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(em_px * font.height_unscaled() / units)
}

/// Cadran : 60 graduations (une sur cinq plus longue), anneau lumineux, braises.
fn draw_dial(pixmap: &mut tiny_skia::Pixmap, d: &Dial) {
    let c = KEY_PX as f32 / 2.0;
    let dim = [42u8, 49, 66];
    // Halo doux autour de l'anneau.
    let mut glow = tiny_skia::Paint::default();
    glow.anti_alias = true;
    glow.set_color_rgba8(d.color[0], d.color[1], d.color[2], 38);
    if let Some(circle) = tiny_skia::PathBuilder::from_circle(c, c, 36.0) {
        let stroke = tiny_skia::Stroke { width: 7.0, ..Default::default() };
        pixmap.stroke_path(&circle, &glow, &stroke, tiny_skia::Transform::identity(), None);
    }
    for i in 0..60u8 {
        let major = i % 5 == 0;
        let angle = (i as f32 * 6.0 - 90.0).to_radians();
        let (r_out, r_in) = (40.0, if major { 32.5 } else { 35.0 });
        let (sin, cos) = angle.sin_cos();
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(c + cos * r_in, c + sin * r_in);
        pb.line_to(c + cos * r_out, c + sin * r_out);
        let Some(path) = pb.finish() else { continue };
        let on = i < d.lit;
        let rgb = if on { d.color } else { dim };
        let mut paint = tiny_skia::Paint::default();
        paint.anti_alias = true;
        paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
        let stroke = tiny_skia::Stroke {
            width: if major { 2.4 } else { 1.6 },
            line_cap: tiny_skia::LineCap::Round,
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, tiny_skia::Transform::identity(), None);
    }
    // Braises : petites étincelles orange qui montent depuis le bas du cadran.
    if d.embers > 0 {
        for k in 0..5u32 {
            let seed = k * 37 + 11;
            // Sous le texte, dans le bas du cadran : 0..12 px de montée.
            let rise = ((d.phase as u32 * 5 + seed * 7) % 12) as f32;
            let x = 30.0 + ((seed * 13 + d.phase as u32 * 3) % 25) as f32;
            let y = 75.0 - rise;
            let fade = 1.0 - rise / 12.0;
            let alpha = (d.embers as f32 * fade * 0.9) as u8;
            let mut paint = tiny_skia::Paint::default();
            paint.anti_alias = true;
            paint.set_color_rgba8(255, 150, 60, alpha);
            if let Some(dot) = tiny_skia::PathBuilder::from_circle(x, y, 1.3 + fade) {
                pixmap.fill_path(&dot, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
            }
        }
    }
}

/// Largeur du texte à la taille `px`.
fn text_width(font: &FontVec, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(em_scale(font, px));
    text.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum()
}

/// Coupe `text` en lignes de largeur `max_w` (mots entiers si possible), au plus `max_lines` ;
/// ce qui dépasse est remplacé par « … ».
fn wrap(font: &FontVec, text: &str, px: f32, max_w: f32, max_lines: usize) -> Vec<String> {
    let fits = |s: &str| text_width(font, s, px) <= max_w;
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut overflow = false;
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
        if fits(&candidate) {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        // Mot plus large que la case : coupé au caractère.
        let mut piece = String::new();
        for c in word.chars() {
            piece.push(c);
            if !fits(&piece) {
                piece.pop();
                lines.push(std::mem::take(&mut piece));
                piece.push(c);
            }
        }
        current = piece;
        if lines.len() >= max_lines {
            overflow = true;
            break;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        overflow = true;
    }
    if overflow {
        if let Some(last) = lines.last_mut() {
            while !last.is_empty() && !fits(&format!("{last}…")) {
                last.pop();
            }
            *last = format!("{}…", last.trim_end());
        }
    }
    lines
}

/// Écrit `text` centré horizontalement, réduit si nécessaire pour tenir dans la touche.
fn draw_text(pixmap: &mut tiny_skia::Pixmap, font: &FontVec, text: &str, px: f32, baseline: f32, color: [u8; 3]) {
    let max_width = KEY_PX as f32 - 6.0;
    let width_at = |px: f32| {
        let scaled = font.as_scaled(em_scale(font, px));
        text.chars().map(|c| scaled.h_advance(scaled.glyph_id(c))).sum::<f32>()
    };
    let mut px = px;
    while width_at(px) > max_width && px > 9.0 {
        px -= 1.0;
    }

    let scale = em_scale(font, px);
    let scaled = font.as_scaled(scale);
    let mut x = (KEY_PX as f32 - width_at(px)) / 2.0;
    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    let data = pixmap.data_mut();
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        let glyph = id.with_scale_and_position(scale, point(x, baseline));
        x += scaled.h_advance(id);
        let Some(outline) = font.outline_glyph(glyph) else { continue };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let px_x = bounds.min.x as i32 + gx as i32;
            let px_y = bounds.min.y as i32 + gy as i32;
            if px_x < 0 || px_y < 0 || px_x >= w || px_y >= h {
                return;
            }
            let a = (coverage.clamp(0.0, 1.0) * 255.0) as u16;
            if a == 0 {
                return;
            }
            let i = ((px_y * w + px_x) * 4) as usize;
            for ch in 0..3 {
                let dst = data[i + ch] as u16;
                data[i + ch] = ((color[ch] as u16 * a + dst * (255 - a)) / 255) as u8;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_cache_evite_un_second_rendu() {
        let mut r = Renderer::new(PathBuf::from("resources/icons"), 80);
        let v = KeyVisual { bg: [30, 60, 170], label: Some("Test".into()), ..Default::default() };
        let a = r.render(&v);
        let b = r.render(&v);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(a.starts_with(&[0xFF, 0xD8]), "doit être un JPEG");
    }

    /// Mesure (lancer avec `cargo test --release taille_jpeg -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn taille_jpeg() {
        let icons = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/icons");
        let visuals = [
            KeyVisual { bg: [34, 38, 49], icon: Some("obs/record.svg".into()), label: Some("REC".into()), ..Default::default() },
            KeyVisual { bg: [31, 63, 122], icon: Some("obs/scene.svg".into()), label: Some("Gameplay".into()), highlight: true, ..Default::default() },
            KeyVisual { bg: [34, 38, 49], icon: Some("apps/discord.png".into()), label: Some("Discord".into()), ..Default::default() },
            KeyVisual::blank(),
        ];
        for q in [50u8, 60, 70, 80, 90] {
            let mut r = Renderer::new(icons.clone(), q);
            let sizes: Vec<usize> = visuals.iter().map(|v| r.render(v).len()).collect();
            let reports: Vec<usize> = sizes.iter().map(|s| s.div_ceil(512)).collect();
            println!("q{q}: octets {sizes:?} → rapports de 512 o {reports:?}");
        }
    }

    #[test]
    fn coupure_des_lignes() {
        let Some(font) = load_font() else { return };
        let rows = wrap(&font, "Never Gonna Give You Up (Remastered 2022 Version)", 13.0, 77.0, 2);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].ends_with('…'), "{rows:?}");
        for r in &rows {
            assert!(text_width(&font, r, 13.0) <= 77.0, "{r}");
        }
        assert_eq!(wrap(&font, "Daft Punk", 11.0, 77.0, 1), vec!["Daft Punk"]);
    }

    /// Aperçus PNG de l'écran latéral (`SOOMFON_PREVIEW_DIR=<dossier> cargo test apercu -- --ignored`).
    #[test]
    #[ignore]
    fn apercu_ecran_lateral() {
        let Ok(dir) = std::env::var("SOOMFON_PREVIEW_DIR") else { return };
        let icons = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/icons");
        let mut r = Renderer::new(icons, 90);
        let (w, g) = ([255, 255, 255], [140, 155, 185]);
        let cases = [
            ("musique", KeyVisual {
                bg: [10, 14, 26],
                icon: Some("misc/music.svg".into()),
                lines: vec![TextLine::new("Blinding Lights (Remastered 2020)", 12, w, 2), TextLine::new("The Weeknd", 10, g, 1)],
                ..Default::default()
            }),
            ("heure", KeyVisual {
                bg: [10, 14, 26],
                lines: vec![TextLine::new("18:42", 25, w, 1), TextLine::new("jeu. 12 nov.", 11, g, 1)],
                ..Default::default()
            }),
            ("logo", KeyVisual { bg: [5, 7, 13], icon: Some("misc/blackat.svg".into()), ..Default::default() }),
        ];
        let view = |remaining: Option<u32>, progress: f32, waiting: bool, ready: bool| crate::coil::CoilView {
            remaining,
            progress,
            heat: if remaining.is_some() { 1.0 - progress } else { 0.0 },
            today: 7,
            waiting,
            ready,
            phase: 3,
        };
        let coil = [
            ("coil-chaud", crate::visuals::coil_visual(&view(Some(27), 0.1, false, false), None)),
            ("coil-tiede", crate::visuals::coil_visual(&view(Some(14), 0.55, false, false), None)),
            ("coil-attends", crate::visuals::coil_visual(&view(Some(20), 0.35, true, false), None)),
            ("coil-pret", crate::visuals::coil_visual(&view(None, 1.0, false, true), None)),
            ("coil-touche", crate::visuals::coil_visual(&view(None, 1.0, false, false), Some(("15 s", None)))),
        ];
        let cases = cases.into_iter().chain(coil);
        for (name, v) in cases {
            let img = r.preview(&v);
            let big = image::imageops::resize(&img, 255, 255, FilterType::Nearest);
            big.save(Path::new(&dir).join(format!("{name}.png"))).unwrap();
        }
    }

    #[test]
    fn eclaircir() {
        assert_eq!(lighten([0, 100, 255], 0.5), [128, 178, 255]);
    }
}
