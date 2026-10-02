//! buse-preview: vykreslí text z databáze gBUSE1 do terminálu nebo PNG.
//!
//!   buse-preview data/ADledA.hex --zst 1163 --png out.png
//!   buse-preview data/ADledA.hex --lin 105
//!   buse-preview data/ADledA.hex --zst-preview out.png [--compare reference/zst_preview.png]
//!   buse-preview data/ADledA.hex --fonts outdir

use buse_engine::text::{ascii_art, plain};
use buse_engine::{render_text, Db, Log, RenderOpts};
use buse_tools::{load_db, strips_rgb, write_png};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "použití: buse-preview <db.hex|buse_db.bin> [--zst ID | --lin ID | --dop ID] \
[--png FILE] [--scale N] [--width N] [--panel] [--zst-preview FILE] [--fonts DIR] [--compare REF.png] [--info]";

fn read_png_rgb(path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut rd = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; rd.output_buffer_size()];
    let info = rd.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let n = info.color_type.samples();
    let rgb = buf[..info.buffer_size()].chunks_exact(n).flat_map(|p| match n {
        1 | 2 => [p[0], p[0], p[0]],
        _ => [p[0], p[1], p[2]],
    });
    Ok((info.width, info.height, rgb.collect()))
}

/// Porovná dva náhledy: přesně po pixelech a po LED buňkách (střed buňky svítí / nesvítí).
fn compare(ours: &(u32, u32, Vec<u8>), reference: &str, scale: u32) -> Result<bool, String> {
    let r = read_png_rgb(reference)?;
    if (ours.0, ours.1) != (r.0, r.1) {
        println!("porovnání: rozměry se liší ({}x{} vs {}x{})", ours.0, ours.1, r.0, r.1);
        return Ok(false);
    }
    let diff_px = ours.2.chunks_exact(3).zip(r.2.chunks_exact(3)).filter(|(a, b)| a != b).count();
    let lit = |img: &Vec<u8>, cx: u32, cy: u32| {
        let i = (((cy * scale + scale / 2) * ours.0 + cx * scale + scale / 2) * 3) as usize;
        img[i] > 128
    };
    let (cw, ch) = (ours.0 / scale, ours.1 / scale);
    let mut diff_led = 0;
    for cy in 0..ch {
        for cx in 0..cw {
            diff_led += (lit(&ours.2, cx, cy) != lit(&r.2, cx, cy)) as u32;
        }
    }
    println!(
        "porovnání s {reference}: {} LED buněk, rozdílných {diff_led}; pixelů {}, rozdílných {diff_px}",
        cw * ch,
        ours.0 * ours.1
    );
    Ok(diff_led == 0)
}

fn pad(mut cols: Vec<u8>, width: usize) -> Vec<u8> {
    cols.truncate(width);
    cols.resize(width, 0);
    cols
}

fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let (mut file, mut sel, mut png, mut preview, mut fonts, mut cmp) = (None, None, None, None, None, None);
    let (mut scale, mut width, mut panel, mut info) = (4u32, 135usize, false, false);
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a}: chybí hodnota\n{USAGE}"));
        match a.as_str() {
            "--zst" | "--lin" | "--dop" => sel = Some((a.clone(), val()?)),
            "--png" => png = Some(val()?),
            "--zst-preview" => preview = Some(val()?),
            "--fonts" => fonts = Some(val()?),
            "--compare" => cmp = Some(val()?),
            "--scale" => scale = val()?.parse().map_err(|_| "--scale: číslo")?,
            "--width" => width = val()?.parse().map_err(|_| "--width: číslo")?,
            "--panel" => panel = true,
            "--info" => info = true,
            "-h" | "--help" => return Err(USAGE.into()),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return Err(format!("neznámý argument {a}\n{USAGE}")),
        }
    }
    let db: Db = load_db(&file.ok_or(USAGE)?)?;
    let opts = RenderOpts::reference();
    let mut log = Log::new();
    let mut ok = true;
    if info {
        println!("{}", db.describe());
        for c in &db.cyk {
            println!("cyklus {}: {:?} {:02x?}", c.id, c.names, c.pages);
        }
    }
    if let Some((tab, id)) = sel {
        let raw: &[u8] = match tab.as_str() {
            "--zst" => db.zst(&id).map(|r| r.raw.as_slice()),
            "--lin" => db.lin(&id).map(|r| r.raw.as_slice()),
            _ => id.parse().ok().and_then(|n| db.dop(n)),
        }
        .ok_or(format!("{} {id}: záznam v databázi není", &tab[2..].to_uppercase()))?;
        let mut cols = render_text(&db, raw, &opts, &mut log);
        println!("{} {id}: {} ({} sloupců)", &tab[2..].to_uppercase(), plain(&db, raw), cols.len());
        if panel {
            cols = pad(cols, width);
        }
        println!("{}", ascii_art(&cols, 8));
        if let Some(p) = &png {
            let img = strips_rgb(&[cols], 8, scale);
            write_png(Path::new(p), img.0, img.1, png::ColorType::Rgb, &img.2)?;
            println!("-> {p}");
            if let Some(r) = &cmp {
                ok &= compare(&img, r, scale)?;
            }
        }
    }
    if let Some(p) = preview {
        // stejně jako gbuse_decode.py: zastávky [1..41] oříznuté / doplněné na šířku panelu
        let strips: Vec<Vec<u8>> = db
            .zst
            .iter()
            .skip(1)
            .take(40)
            .map(|r| pad(render_text(&db, &r.raw, &opts, &mut log), width))
            .collect();
        let img = strips_rgb(&strips, 8, scale);
        write_png(Path::new(&p), img.0, img.1, png::ColorType::Rgb, &img.2)?;
        println!("náhled {} zastávek -> {p}", strips.len());
        if let Some(r) = &cmp {
            ok &= compare(&img, r, scale)?;
        }
    }
    if let Some(dir) = fonts {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
        for font in db.fonts() {
            let (mut strips, mut line) = (Vec::new(), Vec::new());
            for code in font.codes() {
                line.extend(font.glyph(code).unwrap().cols.iter().map(|&c| c as u8));
                line.extend([0, 0]);
                if line.len() > 150 {
                    strips.push(std::mem::take(&mut line));
                }
            }
            strips.push(line);
            let img = strips_rgb(&strips, 8, scale);
            let p = Path::new(&dir).join(format!("font_{:02X}.png", font.id));
            write_png(&p, img.0, img.1, png::ColorType::Rgb, &img.2)?;
            println!("font {:02X}: {} glyfů -> {}", font.id, font.codes().count(), p.display());
            if let Some(r) = &cmp {
                let r = Path::new(r).join(format!("font_{:02X}.png", font.id));
                ok &= compare(&img, &r.to_string_lossy(), scale)?;
            }
        }
    }
    for m in log.take() {
        eprintln!("[engine] {m}");
    }
    Ok(ok)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
