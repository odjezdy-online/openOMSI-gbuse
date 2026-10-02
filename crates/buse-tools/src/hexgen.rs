//! Stavba databází gBUSE (`.hex`) z názvů v HOF souboru mapy.
//!
//! Vnější panely (gBUSE0) se skládají od nuly: záhlaví s okny polí, fonty ze souboru `.fnt`
//! (surová sekce FNT, jak ji ukládá editor fontů) a tabulky LIN, CIL a DRU. Vnitřní LED panel
//! (gBUSE1) vychází ze vzorové databáze: fonty, cykly a šablony zůstanou, vymění se LIN a ZST.
//!
//! Čísla záznamů jsou ve všech databázích společná (název → pořadové číslo), takže jedna mapa
//! `zst_map.csv` platí pro vnitřní i vnější panely: CIL má číslo na 3 číslice, ZST a DRU na 4.

use buse_engine::names::{encode_text, normalize};
use buse_engine::outer::{free_text, Format, OuterDb, Window};
use buse_engine::{render_text, Db, Log, RenderOpts};

/// Obraz jako Intel HEX (záznamy po 16 bajtech; souvislé úseky 0xFF se vynechají, jak to
/// dělá gBUSE - čtečka je doplní).
pub fn intel_hex(image: &[u8]) -> String {
    let mut s = String::new();
    for (i, chunk) in image.chunks(16).enumerate() {
        if chunk.iter().all(|&b| b == 0xFF) {
            continue;
        }
        let addr = (i * 16) as u16;
        let mut sum = chunk.len() as u8;
        sum = sum.wrapping_add((addr >> 8) as u8).wrapping_add(addr as u8);
        s.push_str(&format!(":{:02X}{:04X}00", chunk.len(), addr));
        for &b in chunk {
            s.push_str(&format!("{b:02X}"));
            sum = sum.wrapping_add(b);
        }
        s.push_str(&format!("{:02X}\r\n", sum.wrapping_neg()));
    }
    s.push_str(":00000001FF\r\n");
    s
}

/// Sekce: 16B hlavička na adrese dělitelné 32, data na další 32B hranici, za daty 0xFF.
/// Vrací adresu dat.
fn push_section(img: &mut Vec<u8>, tag: &str, gbuse: &str, data: &[u8]) -> usize {
    while img.len() % 32 != 0 {
        img.push(0xFF);
    }
    let head = format!("{tag}: {gbuse}");
    img.extend_from_slice(head.as_bytes());
    while img.len() % 32 != 0 {
        img.push(0xFF);
    }
    let at = img.len();
    img.extend_from_slice(data);
    img.push(0xFF);
    at
}

fn record(out: &mut Vec<u8>, id: &str, text: &[u8]) {
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    out.extend_from_slice(text);
}

/// Jeden název z mapy a jeho číslo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub n: usize,
    pub name: String,
    pub terminus: bool,
}

/// Očísluje názvy: cíle napřed (CIL má jen 3 číslice), pak zastávky; stejný název po
/// normalizaci dostane jedno číslo.
pub fn number_names(termini: &[String], stops: &[String]) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (list, terminus) in [(termini, true), (stops, false)] {
        for name in list {
            let name = name.trim();
            let key = normalize(name);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            out.push(Entry { n: out.len() + 1, name: name.to_string(), terminus });
        }
    }
    out
}

/// `zst_map.csv`: `název;číslo` pro plugin (vnitřní i vnější panely).
pub fn map_csv(entries: &[Entry], source: &str) -> String {
    let mut s = format!(
        "# zst_map.csv - názvy zastávek a cílů mapy -> číslo záznamu v databázích panelů\n\
         # vygenerováno hof2hex z {source}; stejné číslo má ZST (vnitřní panel), CIL a DRU (vnější)\n"
    );
    for e in entries {
        s.push_str(&format!("{};{:04};100;;{}\n", e.name, e.n, if e.terminus { "cíl" } else { "zastávka" }));
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuterKind {
    /// Čelní: linka 28 sloupců, cíl 112, celých 19 řádků.
    Front,
    /// Boční: linka 28 sloupců, cíl ve spodních 9 řádcích, nácestné zastávky v horních 10.
    Side,
    /// Zadní: jen pole linky 28 x 19 (okno cíle jako u čelního, panel ho nemá).
    Rear,
}

/// Databáze vnějšího panelu: fonty z `fnt` (surová sekce FNT), linky, cíle a (u bočního)
/// zastávky. Texty se do oken formátují stejně jako názvy ze hry (`outer::free_text`):
/// největší font, který se vejde, jinak dva řádky. V gBUSE0 jdou potom upravit ručně.
pub fn outer_image(kind: OuterKind, fnt: &[u8], name: &str, lines: &[String], entries: &[Entry]) -> Result<Vec<u8>, String> {
    let width: u8 = if kind == OuterKind::Front { 140 } else { 112 };
    let line_win = Window { bottom: 0, top: 18, left: 0, right: 27 };
    let dest_win = Window { bottom: 0, top: if kind == OuterKind::Side { 8 } else { 18 }, left: 28, right: width - 1 };
    let stop_win = Window { bottom: 9, top: 18, left: 28, right: width - 1 };
    // fonty, které soubor má: velký pro linku, malý pro texty
    let ids: Vec<u8> = {
        let (mut p, mut ids) = (0, Vec::new());
        while p + 3 <= fnt.len() && (0xE0..=0xEF).contains(&fnt[p]) {
            ids.push(fnt[p]);
            p += 3 + ((fnt[p + 1] as usize) << 8 | fnt[p + 2] as usize);
        }
        ids
    };
    if ids.is_empty() {
        return Err("soubor fontů nezačíná fontem E0..EF (čekám surovou sekci FNT z gBUSE0)".into());
    }
    let pick = |want: &[u8]| want.iter().copied().find(|f| ids.contains(f)).unwrap_or(ids[0]);
    let fmt_line = Format { font: pick(&[0xE8, 0xEA, 0xE5]), spacing: 1, y: 0, y2: 10 };
    let fmt_text = Format { font: pick(&[0xE3, 0xE1]), spacing: 1, y: 0, y2: 10 };

    let build = |lin: &[u8], cil: &[u8], dru: &[u8]| -> Vec<u8> {
        let mut img = vec![0u8; 0x60];
        img[0] = 0x57;
        let win = |img: &mut [u8], o: usize, w: Window| img[o..o + 4].copy_from_slice(&[w.bottom, w.top, w.left, w.right]);
        let fmt = |img: &mut [u8], o: usize, f: Format| img[o..o + 4].copy_from_slice(&[f.font, f.spacing, f.y, f.y2]);
        win(&mut img, 1, line_win);
        win(&mut img, 5, dest_win);
        // (bajty 9..12: povely IBIS polí linky a cíle, převzato z databází gBUSE0 1.21)
        img[9..13].copy_from_slice(&[0x45, 0x00, 0xF1, 0x00]);
        fmt(&mut img, 0x0D, fmt_line);
        fmt(&mut img, 0x11, fmt_text);
        // jméno databáze: 16 znaků doplněných 0D
        let label: Vec<u8> = name.bytes().filter(|b| (0x20..0x7F).contains(b)).take(14).collect();
        img[0x1F..0x2F].fill(0x0D);
        img[0x1F..0x1F + label.len()].copy_from_slice(&label);
        win(&mut img, 0x2F, if kind == OuterKind::Side { stop_win } else { Window { bottom: 0, top: 0, left: 0, right: 0 } });
        fmt(&mut img, 0x33, Format { font: pick(&[0xE1, 0xE3]), ..fmt_text });
        for o in [0x37, 0x39, 0x3B, 0x3D, 0x3F, 0x42] {
            img[o..o + 2].copy_from_slice(&[0x00, 0x4F]);
        }
        img[0x4F..0x60].fill(0xFF);
        let none = [0x00u8, 0x4F];
        let ptr = |at: usize| [(at >> 8) as u8, at as u8];
        let at = push_section(&mut img, "FNT", "gBUSE0 - 1.21", fnt);
        img[0x15..0x17].copy_from_slice(&ptr(at));
        let at = push_section(&mut img, "LIN", "gBUSE0 - 1.21", lin);
        img[0x17..0x19].copy_from_slice(&ptr(at));
        let p = if cil.is_empty() { none } else { ptr(push_section(&mut img, "CIL", "gBUSE0 - 1.21", cil)) };
        img[0x19..0x1B].copy_from_slice(&p);
        img[0x1B..0x1D].copy_from_slice(&none);
        if !dru.is_empty() {
            let at = push_section(&mut img, "DRU", "gBUSE0 - 1.21", dru);
            img[0x37..0x39].copy_from_slice(&ptr(at));
        }
        img
    };
    // napřed prázdná databáze kvůli fontům, pak texty
    let empty = OuterDb::from_image(build(&[], &[], &[])).map_err(|e| e.to_string())?;
    let (mut lin, mut cil, mut dru) = (Vec::new(), Vec::new(), Vec::new());
    for l in lines {
        let l = l.trim();
        if !l.is_empty() && l.len() <= 3 && l.bytes().all(|b| b.is_ascii_digit()) {
            record(&mut lin, &format!("{l:0>3}"), &free_text(&empty, line_win, fmt_line, l.trim_start_matches('0')));
        }
    }
    if kind != OuterKind::Rear {
        for e in entries.iter().filter(|e| e.n <= 999) {
            record(&mut cil, &format!("{:03}", e.n), &free_text(&empty, dest_win, fmt_text, &e.name));
        }
    }
    if kind == OuterKind::Side {
        for e in entries.iter().filter(|e| e.n <= 9999) {
            record(&mut dru, &format!("{:04}", e.n), &free_text(&empty, stop_win, fmt_text, &format!("-{}-", e.name)));
        }
    }
    let img = build(&lin, &cil, &dru);
    if img.len() > 0xFFFF {
        return Err(format!("databáze má {} B, do 64 kB se nevejde (méně názvů, nebo menší fonty)", img.len()));
    }
    Ok(img)
}

/// Databáze vnitřního LED panelu: vzor (fonty, cykly, šablony) s vyměněnými tabulkami LIN a
/// ZST. Zastávka je `E1 B1 C0 & název` jako v Brně; co se fontem E1 nevejde do 103 sloupců
/// (pole cíle se šipkou), dostane úzký font E0.
pub fn inner_image(template: &Db, name: &str, lines: &[String], entries: &[Entry]) -> Result<Vec<u8>, String> {
    let sec = |tag: &str| template.sections.iter().find(|s| s.tag == tag).ok_or(format!("vzorová databáze nemá sekci {tag}"));
    let (lin_s, zst_s) = (sec("LIN")?, sec("ZST")?);
    if zst_s.header < lin_s.header {
        return Err("vzorová databáze má ZST před LIN, s tím generátor nepočítá".into());
    }
    let ver = format!("gBUSE{} - {}", lin_s.gbuse, lin_s.version);
    let mut log = Log::new();
    let opts = RenderOpts::default();
    let (mut lin, mut zst) = (Vec::new(), Vec::new());
    record(&mut lin, "000", &[0xE3, 0xB1, 0x20, 0x0D]);
    for l in lines {
        let l = l.trim();
        if !l.is_empty() && l.len() <= 3 && l.bytes().all(|b| b.is_ascii_digit()) && l.parse::<u32>() != Ok(0) {
            let digits = l.trim_start_matches('0');
            let mut t = vec![0xE3, if digits.len() == 1 { 0xB3 } else { 0xB1 }];
            t.extend_from_slice(digits.as_bytes());
            t.push(0x0D);
            record(&mut lin, &format!("{l:0>3}"), &t);
        }
    }
    record(&mut zst, "0000", &[0xE1, 0xB1, 0xC0, 0x26, 0x0D]);
    for e in entries.iter().filter(|e| e.n <= 9999) {
        let text = |font: u8, space: bool| {
            let mut t = vec![font, 0xB1, 0xC0, 0x26];
            if space {
                t.push(0x20);
            }
            encode_text(template, &e.name, &mut t);
            t.push(0x0D);
            t
        };
        let mut t = text(0xE1, true);
        if render_text(template, &t, &opts, &mut log).len() > 103 {
            t = text(0xE0, false);
        }
        record(&mut zst, &format!("{:04}", e.n), &t);
    }
    let mut img = template.image[..lin_s.header].to_vec();
    // ukazatele na data sekcí v záhlaví (big-endian) se přepíšou podle staré hodnoty
    let repoint = |img: &mut [u8], old: usize, new: usize| {
        let (o, n) = ([(old >> 8) as u8, old as u8], [(new >> 8) as u8, new as u8]);
        for i in 9..0x1E {
            if img[i..i + 2] == o {
                img[i..i + 2].copy_from_slice(&n);
                return true;
            }
        }
        false
    };
    let at = push_section(&mut img, "LIN", &ver, &lin);
    if !repoint(&mut img, lin_s.data, at) {
        return Err("v záhlaví vzorové databáze není ukazatel na LIN".into());
    }
    let at = push_section(&mut img, "ZST", &ver, &zst);
    if !repoint(&mut img, zst_s.data, at) {
        return Err("v záhlaví vzorové databáze není ukazatel na ZST".into());
    }
    // jméno databáze (např. T0A0011_050603) zůstává vzorové, jen datum se nepřepisuje;
    // vlastní jméno se vejde do 14 znaků před dvojicí 0D 0D
    if let Some(end) = img[..0x40].windows(2).position(|w| w == [0x0D, 0x0D]) {
        // (tvar jména jako u vzoru: sedm znaků, podtržítko, šest číslic)
        let mut label: Vec<u8> = name.bytes().filter(|b| b.is_ascii_alphanumeric()).map(|b| b.to_ascii_uppercase()).take(7).collect();
        if !label.is_empty() && end >= 14 {
            label.resize(7, b'0');
            label.extend_from_slice(b"_000000");
            img[end - 14..end].copy_from_slice(&label);
        }
    }
    if img.len() > 0xFFFF {
        return Err(format!("databáze má {} B, do 64 kB se nevejde", img.len()));
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_hex_round_trips() {
        let mut img: Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();
        img[32..64].fill(0xFF);
        let text = intel_hex(&img);
        assert!(text.ends_with(":00000001FF\r\n"));
        assert_eq!(buse_engine::hex::parse_intel_hex(&text).unwrap(), img);
    }

    #[test]
    fn names_are_numbered_once() {
        let e = number_names(&["Hlavní nádraží".into(), "Bystrc".into()], &["Hlavni nadrazi".into(), "Achtelky".into()]);
        assert_eq!(e.iter().map(|x| (x.n, x.name.as_str(), x.terminus)).collect::<Vec<_>>(), [(1, "Hlavní nádraží", true), (2, "Bystrc", true), (3, "Achtelky", false)]);
        assert!(map_csv(&e, "x.hof").contains("Achtelky;0003;100;;zastávka\n"));
    }
}
