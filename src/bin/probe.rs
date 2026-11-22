//! Phase 1 — sonde matérielle pour le SOOMFON Studio Control Deck (5548:6670, alias Mirabox HSV293S).
//!
//! Ce que fait la sonde :
//!   1. se connecte au boîtier (protocole v1, 18 index de touches) ;
//!   2. affiche sur chaque touche son *index image* (0..17) sur fond coloré, avec un petit carré
//!      blanc en haut à gauche pour vérifier rotation / miroir ;
//!   3. journalise chaque appui (index brut + octet d'état) dans la console et dans `probe-log.txt`.
//!
//! Usage : `cargo run --release --bin probe -- [durée_en_secondes]` (60 s par défaut, Ctrl+C pour arrêter).
//! Le logiciel officiel « Stream Controller » doit être fermé pendant le test.

use std::{
    fs::File,
    io::Write,
    time::{Duration, Instant},
};

use image::{DynamicImage, Rgb, RgbImage, codecs::jpeg::JpegEncoder};
use mirajazz::{
    device::{Device, DeviceQuery, list_devices},
    error::MirajazzError,
    types::DeviceInput,
};

const VID: u16 = 0x5548;
const PID: u16 = 0x6670;
/// Page d'usage HID 0xFFA0 (vendeur), usage 1 — l'interface de contrôle du boîtier.
const QUERY: DeviceQuery = DeviceQuery::new(0xFFA0, 1, VID, PID);
const KEY_COUNT: usize = 18;

/// Prépare l'image au format attendu par le boîtier (protocole v1) :
/// 85×85, rotation 90°, miroir horizontal + vertical, JPEG qualité 90.
///
/// On le fait nous-mêmes plutôt que via `set_button_image` de mirajazz, qui exige un moteur
/// tokio multi-thread. Bonus : ces octets pourront être mis en cache tels quels par le démon.
fn encode_key(image: DynamicImage) -> Vec<u8> {
    let image = image.rotate90().fliph().flipv().into_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&image)
        .expect("encodage JPEG");
    jpeg
}

/// Police bitmap 5×7 pour les chiffres (chaque ligne = 5 bits, bit de poids fort à gauche).
const DIGITS: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
];

const SIZE: u32 = 85;
const SCALE: u32 = 6;

fn hue_color(i: usize) -> Rgb<u8> {
    // Teintes bien distinctes, assez sombres pour que le texte blanc reste lisible.
    const PALETTE: [[u8; 3]; 6] = [
        [170, 30, 30],
        [30, 120, 30],
        [30, 60, 170],
        [150, 100, 0],
        [120, 30, 140],
        [0, 120, 130],
    ];
    Rgb(PALETTE[i % PALETTE.len()])
}

fn draw_digit(img: &mut RgbImage, digit: usize, x0: u32, y0: u32) {
    for (row, bits) in DIGITS[digit].iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        img.put_pixel(x0 + col * SCALE + dx, y0 + row as u32 * SCALE + dy, Rgb([255, 255, 255]));
                    }
                }
            }
        }
    }
}

/// Image de test : fond coloré, numéro centré, carré blanc en haut à gauche (repère d'orientation).
fn key_image(index: usize) -> DynamicImage {
    let mut img = RgbImage::from_pixel(SIZE, SIZE, hue_color(index));

    for y in 3..13 {
        for x in 3..13 {
            img.put_pixel(x, y, Rgb([255, 255, 255]));
        }
    }

    let digits: Vec<usize> = index.to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let digit_w = 5 * SCALE;
    let gap = SCALE;
    let total_w = digits.len() as u32 * digit_w + (digits.len() as u32 - 1) * gap;
    let x_start = (SIZE - total_w) / 2;
    let y_start = (SIZE - 7 * SCALE) / 2;
    for (n, d) in digits.iter().enumerate() {
        draw_digit(&mut img, *d, x_start + n as u32 * (digit_w + gap), y_start);
    }

    DynamicImage::ImageRgb8(img)
}

fn log(file: &mut File, line: &str) {
    println!("{line}");
    let _ = writeln!(file, "{line}");
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), MirajazzError> {
    let duration = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);

    let mut out = File::create("probe-log.txt").expect("impossible de créer probe-log.txt");

    let devices = list_devices(&[QUERY]).await?;
    if devices.is_empty() {
        log(&mut out, "Aucun boîtier 5548:6670 trouvé (page d'usage 0xFFA0). Est-il branché ?");
        return Ok(());
    }

    let dev = devices.into_iter().next().unwrap();
    log(
        &mut out,
        &format!(
            "Boîtier trouvé : {:04X}:{:04X} série={:?}",
            dev.vendor_id, dev.product_id, dev.serial_number
        ),
    );

    let t = Instant::now();
    let device = Device::connect(&dev, 1, KEY_COUNT, 0).await?;
    log(&mut out, &format!("Connecté en {:?} (série {})", t.elapsed(), device.serial_number()));

    device.set_brightness(60).await?;
    device.clear_all_button_images().await?;

    let t = Instant::now();
    let encoded: Vec<Vec<u8>> = (0..KEY_COUNT).map(|i| encode_key(key_image(i))).collect();
    let encode = t.elapsed();
    let total_bytes: usize = encoded.iter().map(Vec::len).sum();
    for (i, jpeg) in encoded.iter().enumerate() {
        device.write_image(i as u8, jpeg).await?;
    }
    log(&mut out, &format!("JPEG : {total_bytes} octets au total pour 18 touches"));
    device.flush().await?;
    log(
        &mut out,
        &format!(
            "18 images envoyées : encodage JPEG {:?}, total avec envoi USB {:?}",
            encode,
            t.elapsed()
        ),
    );

    log(
        &mut out,
        &format!(
            "Appuie sur les touches dans l'ordre de lecture (gauche→droite, haut→bas). Arrêt dans {duration} s ou Ctrl+C."
        ),
    );

    let reader = device.get_reader(|_, _| Ok(DeviceInput::NoData));
    let start = Instant::now();
    let deadline = tokio::time::sleep(Duration::from_secs(duration));
    let mut keep_alive = tokio::time::interval(Duration::from_secs(10));
    keep_alive.tick().await; // le premier tick est immédiat
    let mut presses = 0u32;

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c, deadline);

    // Une seule lecture USB reste en attente en permanence : on ne la recrée qu'après
    // avoir reçu un rapport, pour ne jamais annuler une lecture en cours (risque de perte).
    let read = reader.raw_read_data(512);
    tokio::pin!(read);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => {
                log(&mut out, "Ctrl+C reçu, arrêt.");
                break;
            }
            _ = &mut deadline => break,
            _ = keep_alive.tick() => {
                device.keep_alive().await?;
            }
            res = &mut read => {
                read.set(reader.raw_read_data(512));
                match res {
                    Ok(data) => {
                        presses += 1;
                        let head: Vec<String> = data[..16].iter().map(|b| format!("{b:02X}")).collect();
                        let ack = data.starts_with(b"ACK");
                        log(
                            &mut out,
                            &format!(
                                "[{:>7.3}s] index_brut={:>3} état={:#04X} ack={} | {}",
                                start.elapsed().as_secs_f32(),
                                data[9],
                                data[10],
                                ack,
                                head.join(" ")
                            ),
                        );
                    }
                    Err(e) => {
                        log(&mut out, &format!("Erreur de lecture : {e:?}"));
                        break;
                    }
                }
            }
        }
    }

    log(&mut out, &format!("Fin : {presses} rapports reçus."));
    device.shutdown().await?;
    Ok(())
}
