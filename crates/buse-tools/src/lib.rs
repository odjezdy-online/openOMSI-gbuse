//! Sdílené pomocné funkce nástrojů (PNG výstup, čtení souborů).

pub mod assets;
pub mod hexgen;
pub mod hof;
pub mod vehicle;

use std::path::Path;

/// Barva svítící / zhasnuté LED (shodně s `tools/gbuse_decode.py`).
pub const LED_ON: [u8; 3] = [255, 74, 28];
pub const LED_OFF: [u8; 3] = [44, 16, 10];
pub const BACKGROUND: [u8; 3] = [20, 8, 6];

pub fn write_png(path: &Path, w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(color);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(data).map_err(|e| e.to_string())
}

/// Je bod (px, py) uvnitř kruhu vepsaného do buňky `scale` x `scale`? (Pillow `ellipse`
/// s obdélníkem [0, scale-1] vykreslí pro scale 4 plný čtverec bez rohů.)
pub fn in_dot(px: u32, py: u32, scale: u32) -> bool {
    if scale < 3 {
        return true;
    }
    let c = (scale as f32 - 1.0) / 2.0;
    let (dx, dy) = (px as f32 - c, py as f32 - c);
    dx * dx + dy * dy <= (c + 0.25) * (c + 0.25)
}

/// Vykreslí pruhy (každý = sloupce, bit `h-1` nahoře) pod sebe jako LED náhled.
pub fn strips_rgb(strips: &[Vec<u8>], h: u32, scale: u32) -> (u32, u32, Vec<u8>) {
    let width = strips.iter().map(|s| s.len()).max().unwrap_or(1).max(1) as u32;
    let (w, ht) = (width * scale, strips.len() as u32 * (h + 2) * scale);
    let mut img = vec![0u8; (w * ht * 3) as usize];
    for px in img.chunks_exact_mut(3) {
        px.copy_from_slice(&BACKGROUND);
    }
    for (row, cols) in strips.iter().enumerate() {
        for (x, &c) in cols.iter().enumerate() {
            for y in 0..h {
                let on = (c >> (h - 1 - y)) & 1 != 0;
                let (x0, y0) = (x as u32 * scale, (row as u32 * (h + 2) + y) * scale);
                for dy in 0..scale {
                    for dx in 0..scale {
                        if in_dot(dx, dy, scale) {
                            let i = (((y0 + dy) * w + x0 + dx) * 3) as usize;
                            img[i..i + 3].copy_from_slice(if on { &LED_ON } else { &LED_OFF });
                        }
                    }
                }
            }
        }
    }
    (w, ht, img)
}

pub fn load_db(path: &str) -> Result<buse_engine::Db, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    if bytes.starts_with(buse_engine::db::BIN_MAGIC) {
        return buse_engine::Db::from_bin(&bytes).map_err(|e| format!("{path}: {e}"));
    }
    buse_engine::Db::from_hex(&String::from_utf8_lossy(&bytes)).map_err(|e| format!("{path}: {e}"))
}
