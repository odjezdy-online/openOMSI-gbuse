//! hof2hex: z HOF souboru mapy vyrobí databáze panelů BUSE (`.hex`), které jdou dál upravovat
//! v gBUSE0 / gBUSE1 a které plugin čte přímo.
//!
//!   hof2hex mapa.hof [dalsi.hof]... -o OUT --font font.fnt [--inner-template ADledA.hex] [--name MAPA400]
//!
//! Výstup:
//!   <name>_cel.hex   čelní panel 140 x 19 (linka + cíl)
//!   <name>_bok.hex   boční panel 112 x 19 (linka, cíl dole, nácestné zastávky nahoře)
//!   <name>_zad.hex   zadní panel (linka)
//!   <name>_led.hex   vnitřní LED panel (jen s --inner-template: fonty, cykly a šablony ze vzoru)
//!   zst_map.csv      název z mapy -> číslo záznamu, společné pro všechny čtyři databáze

use buse_tools::hexgen::{self, OuterKind};
use buse_tools::hof::{self, Hof};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "použití: hof2hex <mapa.hof>... -o OUT --font font.fnt [--inner-template ADledA.hex] [--name JMENO]";

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let (mut hofs, mut out, mut font, mut template, mut name) = (Vec::new(), None, None, None, None);
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a}: chybí hodnota\n{USAGE}"));
        match a.as_str() {
            "-o" | "--out" => out = Some(val()?),
            "--font" => font = Some(val()?),
            "--inner-template" => template = Some(val()?),
            "--name" => name = Some(val()?),
            "-h" | "--help" => return Err(USAGE.into()),
            _ => hofs.push(a.clone()),
        }
    }
    if hofs.is_empty() {
        return Err(USAGE.into());
    }
    let out = PathBuf::from(out.ok_or(USAGE)?);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (mut termini, mut stops, mut lines) = (Vec::new(), Vec::new(), Vec::new());
    let mut sources = Vec::new();
    for h in &hofs {
        let bytes = std::fs::read(h).map_err(|e| format!("{h}: {e}"))?;
        let hof = Hof::parse(&hof::decode(&bytes));
        println!("{h}: {} ({} cílů, {} zastávek, {} linek)", hof.name, hof.termini.len(), hof.stops.len(), hof.lines.len());
        for (name, kind) in hof.names() {
            if kind == "cíl" { &mut termini } else { &mut stops }.push(name);
        }
        for l in hof.lines {
            if !lines.contains(&l) {
                lines.push(l);
            }
        }
        sources.push(Path::new(h).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
    lines.sort();
    let entries = hexgen::number_names(&termini, &stops);
    let name = name.unwrap_or_else(|| "MAPA".to_string());
    println!("{} různých názvů ({} cílů), {} linek", entries.len(), entries.iter().filter(|e| e.terminus).count(), lines.len());
    if entries.iter().filter(|e| e.terminus).count() > 999 {
        eprintln!("pozor: cílů je víc než 999, další se do tabulky CIL nevejdou (kreslí se pak volným textem)");
    }
    let write = |file: &str, data: &[u8]| std::fs::write(out.join(file), data).map_err(|e| format!("{file}: {e}"));
    write("zst_map.csv", hexgen::map_csv(&entries, &sources.join(", ")).as_bytes())?;

    if let Some(f) = &font {
        let fnt = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
        for (kind, suffix) in [(OuterKind::Front, "cel"), (OuterKind::Side, "bok"), (OuterKind::Rear, "zad")] {
            let img = hexgen::outer_image(kind, &fnt, &name, &lines, &entries).map_err(|e| format!("{suffix}: {e}"))?;
            let file = format!("{name}_{suffix}.hex");
            write(&file, hexgen::intel_hex(&img).as_bytes())?;
            let db = buse_engine::OuterDb::from_image(img).map_err(|e| format!("{file}: {e}"))?;
            println!("{file}: {}", db.describe());
        }
    } else {
        println!("bez --font se vnější panely negenerují");
    }
    if let Some(t) = &template {
        let db = buse_tools::load_db(t)?;
        let img = hexgen::inner_image(&db, &name, &lines, &entries)?;
        let file = format!("{name}_led.hex");
        write(&file, hexgen::intel_hex(&img).as_bytes())?;
        let db = buse_engine::Db::from_image(img).map_err(|e| format!("{file}: {e}"))?;
        println!("{file}: {}", db.describe());
    } else {
        println!("bez --inner-template se vnitřní panel negeneruje (font.fnt má jen fonty vnějších panelů)");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
