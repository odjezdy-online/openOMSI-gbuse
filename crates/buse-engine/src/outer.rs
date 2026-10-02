//! Vnější panely (čelní, boční, zadní; terčové BS 210 a LED BS 310) s databází z gBUSE0.
//!
//! Vykreslování je přepsané podle rutiny náhledu v `gBUSE0.exe` (0x004a2173 a její pomocné
//! 0x004a1ac4, 0x004a1b34, 0x004a1e30, 0x004a19a0), popsané v `reference/GBUSE_FORMAT.md`:
//!
//! * Panel má až tři **pole** - linka, cíl, zastávka (nácestné) - každé s oknem
//!   `[dolní, horní, levý, pravý]` v záhlaví databáze. Řádky se počítají **odspodu**:
//!   okno s horním okrajem 18 a dolním 0 je celých 19 řádků.
//! * Text pole se kreslí po řádcích do pomocného bufferu a při konci řádku (`0A`, `0C`, `0D`)
//!   se přenese do okna **vodorovně na střed** (`ESC s n` = pevný počátek místo středu).
//! * `E0..EF` font, `B0..BF` mezera mezi znaky, `C0..DF` svislá poloha v okně (odshora).
//! * `ESC l/p/h/d n` mění levý / pravý / horní / dolní okraj okna (n - 16), `ESC c` vrátí okno
//!   z konfigurace, `ESC i` invertuje okno, `ESC o` ho smaže, `ESC b` = cíl zabere celý panel
//!   (ostatní pole se nekreslí), `ESC w` = cíl zabere i pole zastávky.
//! * První `0A` je nový řádek textu; každé další je krok animace (počká se), `0B` skočí zpět
//!   za první `0A`.

use crate::db::{find_sections, parse_table, DbError, Record, Section};
use crate::hex::parse_intel_hex;
use crate::panel::{Frame, Inputs};
use crate::text::Log;

/// Okno pole; řádky odspodu (0 = spodní řádek panelu), sloupce zleva, obojí včetně.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub bottom: u8,
    pub top: u8,
    pub left: u8,
    pub right: u8,
}

impl Window {
    pub fn width(&self) -> usize {
        (self.right as usize + 1).saturating_sub(self.left as usize)
    }

    pub fn height(&self) -> usize {
        (self.top as usize + 1).saturating_sub(self.bottom as usize)
    }
}

/// „Formátovací řetězec" pole: font, mezera, svislá poloha prvního a druhého řádku. Platí
/// pro text, který nepřišel z databáze (volný text po IBIS; tady název ze hry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub font: u8,
    pub spacing: u8,
    pub y: u8,
    pub y2: u8,
}

#[derive(Debug, Clone, Copy)]
struct GlyphRef {
    start: u32,
    w: u16,
    h: u8,
}

/// Font vnějšího panelu: glyfy do výšky 32 řádků, sloupec = u32, bit (h-1) = horní řádek.
#[derive(Debug, Clone)]
pub struct BigFont {
    pub id: u8,
    glyphs: Box<[Option<GlyphRef>; 256]>,
    cols: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct BigGlyph<'a> {
    pub w: usize,
    pub h: usize,
    pub cols: &'a [u32],
}

impl BigFont {
    pub fn glyph(&self, code: u8) -> Option<BigGlyph<'_>> {
        self.glyphs[code as usize].map(|g| BigGlyph {
            w: g.w as usize,
            h: g.h as usize,
            cols: &self.cols[g.start as usize..g.start as usize + g.w as usize],
        })
    }
}

pub const FIELD_LINE: usize = 0;
pub const FIELD_DEST: usize = 1;
pub const FIELD_STOP: usize = 2;

#[derive(Debug, Clone)]
pub struct OuterDb {
    pub image: Vec<u8>,
    pub name: Option<String>,
    pub sections: Vec<Section>,
    pub rows: usize,
    pub width: usize,
    /// Okna polí linka, cíl, zastávka; pole zastávky jen když má databáze tabulku DRU.
    pub windows: [Option<Window>; 3],
    pub formats: [Format; 3],
    fonts: [Option<Box<BigFont>>; 16],
    pub lin: Vec<Record>,
    pub cil: Vec<Record>,
    pub dru: Vec<Record>,
}

fn parse_big_fonts(img: &[u8], start: usize) -> [Option<Box<BigFont>>; 16] {
    let mut fonts: [Option<Box<BigFont>>; 16] = Default::default();
    let mut p = start;
    while p + 3 <= img.len() && (0xE0..=0xEF).contains(&img[p]) {
        let fid = img[p];
        let end = (p + 3 + ((img[p + 1] as usize) << 8 | img[p + 2] as usize)).min(img.len());
        let mut q = p + 3;
        let mut font = BigFont { id: fid, glyphs: Box::new([None; 256]), cols: Vec::new() };
        while q + 3 <= end {
            if q + 2 == end && img[q] == 0xFF && img[q + 1] == 0 {
                break;
            }
            // [kód][počet bajtů dat][výška][data]; sloupec začíná vždy novým bajtem, bity
            // shora (MSB = horní řádek)
            let (code, size, h) = (img[q], img[q + 1] as usize, img[q + 2] as usize);
            if q + 3 + size > end {
                break;
            }
            let bpc = h.div_ceil(8).max(1);
            if (1..=32).contains(&h) {
                let raw = &img[q + 3..q + 3 + size];
                let gstart = font.cols.len() as u32;
                let w = size / bpc;
                for x in 0..w {
                    let mut v: u32 = 0;
                    for k in 0..bpc {
                        v = v << 8 | raw[x * bpc + k] as u32;
                    }
                    font.cols.push(v >> (bpc * 8 - h));
                }
                font.glyphs[code as usize] = Some(GlyphRef { start: gstart, w: w as u16, h: h as u8 });
            }
            q += 3 + size;
        }
        fonts[(fid - 0xE0) as usize] = Some(Box::new(font));
        p = end;
    }
    fonts
}

fn find_name(header: &[u8]) -> Option<String> {
    // jméno databáze stojí před dvojicí 0D 0D (např. T000012_050906, „2001 03", „AD 2003 04")
    let end = header.windows(2).position(|w| w == [0x0D, 0x0D])?;
    let start = header[..end].iter().rposition(|&b| !(0x20..0x7F).contains(&b)).map_or(0, |i| i + 1);
    let s = String::from_utf8_lossy(&header[start..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

impl OuterDb {
    pub fn from_hex(text: &str) -> Result<OuterDb, DbError> {
        OuterDb::from_image(parse_intel_hex(text)?)
    }

    pub fn from_image(image: Vec<u8>) -> Result<OuterDb, DbError> {
        let sections = find_sections(&image);
        if sections.is_empty() || image.len() < 0x40 {
            return Err(DbError::NoSections);
        }
        let win = |o: usize| Window { bottom: image[o], top: image[o + 1], left: image[o + 2], right: image[o + 3] };
        let fmt = |o: usize| Format { font: image[o], spacing: image[o + 1], y: image[o + 2], y2: image[o + 3] };
        let mut db = OuterDb {
            name: find_name(&image[0x1F..0x40]),
            rows: 0,
            width: 0,
            // záhlaví gBUSE0: [1..5) okno linky, [5..9) okno cíle, [0x0D..) a [0x11..) jejich
            // formátovací řetězce, [0x2F..) okno zastávky, [0x33..) jeho formát
            windows: [Some(win(1)), Some(win(5)), None],
            formats: [fmt(0x0D), fmt(0x11), fmt(0x33)],
            fonts: Default::default(),
            lin: Vec::new(),
            cil: Vec::new(),
            dru: Vec::new(),
            sections,
            image,
        };
        for s in &db.sections {
            match s.tag.as_str() {
                "FNT" => db.fonts = parse_big_fonts(&db.image, s.data),
                "LIN" => db.lin = parse_table(&db.image, s.data, 3),
                "CIL" => db.cil = parse_table(&db.image, s.data, 3),
                "DRU" => db.dru = parse_table(&db.image, s.data, 4),
                _ => {}
            }
        }
        if !db.dru.is_empty() {
            let i = &db.image;
            db.windows[FIELD_STOP] = Some(Window { bottom: i[0x2F], top: i[0x30], left: i[0x31], right: i[0x32] });
        }
        let wins = db.windows.iter().flatten();
        db.rows = wins.clone().map(|w| w.top as usize + 1).max().unwrap_or(19).clamp(1, 32);
        db.width = wins.map(|w| w.right as usize + 1).max().unwrap_or(112).clamp(8, 256);
        // (okno linky s pravým okrajem 0 = panel bez pole linky)
        Ok(db)
    }

    pub fn font(&self, id: u8) -> Option<&BigFont> {
        self.fonts.get((id & 0x0F) as usize).and_then(|f| f.as_deref())
    }

    pub fn fonts(&self) -> impl Iterator<Item = &BigFont> {
        self.fonts.iter().filter_map(|f| f.as_deref())
    }

    fn record<'a>(table: &'a [Record], id: &str) -> Option<&'a Record> {
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        let len = table.first().map_or(3, |r| r.id.chars().count());
        // číslo se doplní (nebo zkrátí o úvodní nuly) na délku id tabulky: mapa názvů má čísla
        // na 4 číslice, CIL na 3
        let number = id.bytes().all(|b| b.is_ascii_digit()).then(|| id.trim_start_matches('0'));
        let padded = match number {
            Some(n) if n.len() <= len => format!("{n:0>len$}"),
            _ => id.to_string(),
        };
        table.iter().find(|r| r.id == padded)
    }

    pub fn describe(&self) -> String {
        let w = |w: &Option<Window>| match w {
            Some(w) => format!("řádky {}..{} odspodu, sloupce {}..{}", w.bottom, w.top, w.left, w.right),
            None => "není".into(),
        };
        format!(
            "vnější panel {} ({} B): {} x {} bodů; pole linky: {}; pole cíle: {}; pole zastávky: {}; {} linek, {} cílů, {} zastávek, fonty {}",
            self.name.as_deref().unwrap_or("?"),
            self.image.len(),
            self.width,
            self.rows,
            w(&self.windows[0]),
            w(&self.windows[1]),
            w(&self.windows[2]),
            self.lin.len(),
            self.cil.len(),
            self.dru.len(),
            self.fonts().map(|f| format!("{:02X}", f.id)).collect::<Vec<_>>().join(" ")
        )
    }
}

/// Kód znaku v kódování bratří Kamenických (texty gBUSE); `None` pro znak mimo tabulku.
fn kamenicky(c: char) -> Option<u8> {
    const T: &[(char, u8)] = &[
        ('Č', 0x80), ('ü', 0x81), ('é', 0x82), ('ď', 0x83), ('ä', 0x84), ('Ď', 0x85), ('Ť', 0x86), ('č', 0x87),
        ('ě', 0x88), ('Ě', 0x89), ('Ĺ', 0x8A), ('Í', 0x8B), ('ľ', 0x8C), ('ĺ', 0x8D), ('Ä', 0x8E), ('Á', 0x8F),
        ('É', 0x90), ('ž', 0x91), ('Ž', 0x92), ('ô', 0x93), ('ö', 0x94), ('Ó', 0x95), ('ů', 0x96), ('Ú', 0x97),
        ('ý', 0x98), ('Ö', 0x99), ('Ü', 0x9A), ('Š', 0x9B), ('Ľ', 0x9C), ('Ý', 0x9D), ('Ř', 0x9E), ('ť', 0x9F),
        ('á', 0xA0), ('í', 0xA1), ('ó', 0xA2), ('ú', 0xA3), ('ň', 0xA4), ('Ň', 0xA5), ('Ů', 0xA6), ('Ô', 0xA7),
        ('š', 0xA8), ('ř', 0xA9), ('ŕ', 0xAA), ('Ŕ', 0xAB),
    ];
    if (' '..='\u{7E}').contains(&c) {
        return Some(c as u8);
    }
    T.iter().find(|t| t.0 == c).map(|t| t.1)
}

/// Kód glyfu pro znak ve fontu: Kamenických, u velkého Í i kód 7F (tak ho mají databáze
/// vnějších panelů), pak velké písmeno a nakonec písmeno bez diakritiky.
fn glyph_code(font: &BigFont, c: char) -> Option<u8> {
    let has = |code: u8| font.glyph(code).filter(|g| g.w > 0).map(|_| code);
    let direct = |c: char| kamenicky(c).and_then(has).or_else(|| if c == 'Í' { has(0x7F) } else { None });
    let upper = c.to_uppercase().next().unwrap_or(c);
    let plain = crate::names::fold_char(c);
    direct(c).or_else(|| direct(upper)).or_else(|| direct(plain)).or_else(|| direct(plain.to_ascii_uppercase()))
}

/// Text ve fontu: kódy glyfů, šířka s danou mezerou a výška nejvyššího glyfu; `None`, když
/// font některý znak nemá.
fn measure(font: &BigFont, text: &str, spacing: usize) -> Option<(Vec<u8>, usize, usize)> {
    let (mut codes, mut w, mut h) = (Vec::new(), 0, 0);
    for c in text.chars() {
        let code = glyph_code(font, c)?;
        let g = font.glyph(code)?;
        w += g.w + if codes.is_empty() { 0 } else { spacing };
        h = h.max(g.h);
        codes.push(code);
    }
    Some((codes, w, h))
}

/// Volný text (název ze hry) pro okno: největší font, kterým se vejde na jeden řádek,
/// svisle na střed; jinak dva řádky menším fontem; jinak nejmenší font, co se nevejde,
/// ořízne okno. Obdoba AUTOFORMÁTu editoru cílů.
pub fn free_text(db: &OuterDb, win: Window, fmt: Format, text: &str) -> Vec<u8> {
    let text = text.trim();
    let (ww, wh) = (win.width(), win.height());
    let spacing = fmt.spacing.min(15) as usize;
    let mut one: Option<(usize, u8, Vec<u8>, usize)> = None; // (výška, font, kódy, šířka)
    let mut smallest: Option<(usize, u8, Vec<u8>)> = None;
    for font in db.fonts() {
        let Some((codes, w, h)) = measure(font, text, spacing) else { continue };
        if h == 0 || h > wh {
            continue;
        }
        // při stejné výšce má přednost font pole ze záhlaví databáze
        if w <= ww && one.as_ref().is_none_or(|o| h > o.0 || (h == o.0 && font.id & 0x0F == fmt.font & 0x0F && o.1 != font.id)) {
            one = Some((h, font.id, codes.clone(), w));
        }
        if smallest.as_ref().is_none_or(|s| h < s.0) {
            smallest = Some((h, font.id, codes));
        }
    }
    let line = |font: u8, y: usize, codes: &[u8], out: &mut Vec<u8>| {
        out.extend([0xE0 | (font & 0x0F), 0xB0 + spacing as u8, 0xC0 + y.min(31) as u8]);
        out.extend_from_slice(codes);
    };
    let mut out = Vec::new();
    // dva řádky: jen když jednořádkový font vyjde malý (do poloviny okna) a text má mezeru
    let small_one = one.as_ref().is_none_or(|o| o.0 * 2 <= wh);
    if small_one {
        let mut best: Option<(usize, u8, Vec<u8>, Vec<u8>)> = None;
        let spaces: Vec<usize> = text.char_indices().filter(|c| c.1 == ' ' || c.1 == '-').map(|c| c.0).collect();
        for font in db.fonts() {
            for &i in &spaces {
                let (a, b) = (text[..i].trim_end(), text[i + 1..].trim_start());
                let (Some((ca, wa, ha)), Some((cb, wb, hb))) = (measure(font, a, spacing), measure(font, b, spacing)) else { continue };
                let h = ha.max(hb);
                if h == 0 || h * 2 > wh + 1 || wa > ww || wb > ww {
                    continue;
                }
                // nejvyšší font, při shodě nejvyrovnanější řádky
                let better = best.as_ref().is_none_or(|x| h > x.0);
                if better || (best.as_ref().is_some_and(|x| h == x.0 && x.1 == font.id) && wa.abs_diff(wb) < measure_diff(font, &best, spacing)) {
                    best = Some((h, font.id, ca, cb));
                }
            }
        }
        if let Some((h, font, a, b)) = best {
            if one.as_ref().is_none_or(|o| h >= o.0) {
                line(font, 0, &a, &mut out);
                out.push(0x0A);
                line(font, wh - h, &b, &mut out);
                out.push(0x0D);
                return out;
            }
        }
    }
    match (one, smallest) {
        (Some((h, font, codes, _)), _) | (None, Some((h, font, codes))) => {
            line(font, (wh - h) / 2, &codes, &mut out);
        }
        (None, None) => {}
    }
    out.push(0x0D);
    out
}

fn measure_diff(font: &BigFont, best: &Option<(usize, u8, Vec<u8>, Vec<u8>)>, spacing: usize) -> usize {
    let w = |codes: &[u8]| codes.iter().map(|&c| font.glyph(c).map_or(0, |g| g.w)).sum::<usize>() + codes.len().saturating_sub(1) * spacing;
    best.as_ref().map_or(usize::MAX, |b| w(&b.2).abs_diff(w(&b.3)))
}

const BUF_W: usize = 256;
const BUF_H: usize = 64;

/// Rozpracovaný text jednoho pole.
#[derive(Debug, Clone)]
struct Run {
    raw: Vec<u8>,
    pos: usize,
    loop_start: usize,
    first_lf: bool,
    done: bool,
    home: Window,
    win: Window,
    font: u8,
    spacing: usize,
    y: usize,
    buf: Vec<u8>,
    cursor: usize,
    maxx: usize,
    maxy: usize,
    miny: usize,
    first: bool,
    centered: bool,
    or_mode: bool,
    /// `ESC b`: text zabral celý panel, ostatní pole se nekreslí.
    hide_others: bool,
    /// `ESC w`: text zabral i pole zastávky.
    hide_stop: bool,
    /// `ESC z n`: násobek doby kroku animace.
    units: u32,
    acc: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Pause,
    End,
}

impl Run {
    fn new(raw: Vec<u8>, win: Window, fmt: Format, rows: usize) -> Run {
        let mut r = Run {
            raw,
            pos: 0,
            loop_start: 0,
            first_lf: true,
            done: false,
            home: win,
            win,
            font: fmt.font,
            spacing: (fmt.spacing & 0x0F) as usize,
            y: (fmt.y & 0x1F) as usize,
            buf: vec![0; BUF_W * BUF_H],
            cursor: 0,
            maxx: 0,
            maxy: 0,
            miny: 0,
            first: true,
            centered: true,
            or_mode: false,
            hide_others: false,
            hide_stop: false,
            units: 1,
            acc: 0.0,
        };
        r.clear(rows);
        r
    }

    fn clear(&mut self, rows: usize) {
        self.buf.fill(0);
        self.cursor = 0;
        self.maxx = 0;
        self.maxy = 0;
        self.miny = rows.saturating_sub(1);
        self.first = true;
    }

    fn glyph(&mut self, db: &OuterDb, code: u8, log: &mut Log) {
        match db.font(self.font).and_then(|f| f.glyph(code)).filter(|g| g.w > 0) {
            Some(g) => {
                if !self.first {
                    self.cursor += self.spacing;
                }
                self.first = false;
                for (i, &col) in g.cols.iter().enumerate() {
                    let x = self.cursor + i;
                    for r in 0..g.h {
                        let y = self.y + r;
                        if y < db.rows.min(BUF_H) && x < BUF_W {
                            self.buf[y * BUF_W + x] = (col >> (g.h - 1 - r) & 1) as u8;
                        }
                    }
                }
                self.cursor += g.w;
                self.maxx = self.maxx.max(self.cursor);
                self.maxy = self.maxy.max(g.h + self.y);
            }
            None => {
                let font = self.font;
                log.once(0x4000 | (font as u32 & 0xF) << 8 | code as u32, || format!("vnější panel: font E{:X} nemá glyf {code:02X}", font & 0xF));
            }
        }
        self.miny = self.miny.min(self.y);
    }

    /// Řádky okna na displeji (shora): `(první, poslední)`.
    fn rows_of(win: Window, rows: usize) -> (isize, isize) {
        let last = rows as isize - 1;
        (last - win.top as isize, last - win.bottom as isize)
    }

    /// Přenese rozpracovaný řádek z bufferu do okna (na střed, nebo od pevného počátku).
    fn flush(&mut self, disp: &mut [u8], rows: usize, width: usize) {
        if self.maxx != 0 {
            let (l, r) = (self.win.left as usize, self.win.right as usize);
            let start = if self.centered {
                let ww = (r + 1).saturating_sub(l);
                l + if ww < self.maxx { 0 } else { (ww - self.maxx) / 2 }
            } else {
                0
            };
            let (top, bottom) = Run::rows_of(self.win, rows);
            for x in l..=r.min(width.saturating_sub(1)) {
                for row in self.miny..self.maxy.min(BUF_H) {
                    let dr = top + row as isize;
                    if dr < 0 || dr > bottom || dr >= rows as isize {
                        continue;
                    }
                    let d = &mut disp[dr as usize * width + x];
                    if x < start {
                        *d = 0;
                    } else {
                        let src = if x - start < BUF_W { self.buf[row * BUF_W + (x - start)] } else { 0 };
                        if !self.or_mode || src == 1 {
                            *d = src;
                        }
                    }
                }
            }
        }
        self.centered = true;
        self.or_mode = false;
    }

    /// Pro každý bod okna na displeji.
    fn each(win: Window, rows: usize, width: usize, disp: &mut [u8], mut f: impl FnMut(&mut u8)) {
        let (top, bottom) = Run::rows_of(win, rows);
        for row in top.max(0)..=bottom.min(rows as isize - 1) {
            for x in win.left as usize..=(win.right as usize).min(width.saturating_sub(1)) {
                f(&mut disp[row as usize * width + x]);
            }
        }
    }

    /// Kreslí do další pauzy (krok animace) nebo do konce textu.
    fn step(&mut self, db: &OuterDb, disp: &mut [u8], log: &mut Log) -> Step {
        let (rows, width) = (db.rows, db.width);
        // (smyčka `0B` bez jediné pauzy by se točila donekonečna)
        let mut budget = self.raw.len() * 2 + 16;
        while self.pos < self.raw.len() && budget > 0 {
            budget -= 1;
            let b = self.raw[self.pos];
            self.pos += 1;
            match b {
                0x0A => {
                    self.flush(disp, rows, width);
                    self.clear(rows);
                    if self.first_lf {
                        self.first_lf = false;
                        self.loop_start = self.pos;
                    } else {
                        return Step::Pause;
                    }
                }
                0x0B => {
                    self.flush(disp, rows, width);
                    self.clear(rows);
                    self.pos = self.loop_start;
                    return Step::Pause;
                }
                0x0C => {
                    self.flush(disp, rows, width);
                    self.clear(rows);
                }
                0x0D => break,
                0x1B => {
                    let Some(&cmd) = self.raw.get(self.pos) else { break };
                    self.pos += 1;
                    let arg = |s: &mut Run| {
                        let v = s.raw.get(s.pos).copied().unwrap_or(0x10).wrapping_sub(0x10);
                        s.pos += 1;
                        v
                    };
                    match cmd {
                        b'a' => {
                            arg(self);
                        }
                        b'b' => self.hide_others = true,
                        b'c' => self.win = self.home,
                        b'd' => self.win.bottom = arg(self),
                        b'h' => self.win.top = arg(self),
                        b'l' => self.win.left = arg(self),
                        b'p' => self.win.right = arg(self),
                        b'i' => Run::each(self.win, rows, width, disp, |d| *d ^= 1),
                        b'o' => Run::each(self.win, rows, width, disp, |d| *d = 0),
                        b's' => {
                            self.cursor = arg(self) as usize;
                            self.centered = false;
                            self.first = true;
                        }
                        b'w' => self.hide_stop = true,
                        b'x' => self.or_mode = true,
                        b'z' => self.units = (arg(self) as u32).max(1),
                        // sedmibitové obdoby B0.., C0.., E0..
                        0x20..=0x2F => self.spacing = (cmd - 0x20) as usize,
                        0x30..=0x4F => self.y = (cmd - 0x30) as usize,
                        0x50..=0x5F => self.font = cmd - 0x50,
                        // `n` (posun okna o řádek nahoru), `r` (o sloupec vlevo), `v`: nekreslí se
                        _ => {}
                    }
                }
                0xB0..=0xBF => self.spacing = (b - 0xB0) as usize,
                0xC0..=0xDF => self.y = (b - 0xC0) as usize,
                0xE0..=0xEF => self.font = b - 0xE0,
                _ => self.glyph(db, b, log),
            }
        }
        self.flush(disp, rows, width);
        self.done = true;
        Step::End
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OuterConfig {
    /// Doba jednoho kroku animace (další `0A` v textu) v ms; `ESC z n` ji násobí. HYPOTÉZA.
    pub step_ms: f64,
    /// Kreslit pole linky / cíle / zastávky.
    pub fields: [bool; 3],
    /// Volný text zastávky se obalí pomlčkami jako záznamy DRU (`-Achtelky-`).
    pub stop_dashes: bool,
    /// Šířka snímku: 0 = celá šířka z databáze; menší ořízne zprava (zadní panel má v
    /// databázi okna jako čelní, ale jen 28 sloupců).
    pub width: usize,
    /// Bez čísla linky dostanou cíl a zastávka i sloupce pole linky (text stojí na středu
    /// celého panelu, jak to dělá skript VMatrix).
    pub expand_dest: bool,
}

impl Default for OuterConfig {
    fn default() -> Self {
        OuterConfig { step_ms: 2000.0, fields: [true; 3], stop_dashes: true, width: 0, expand_dest: false }
    }
}

pub struct OuterPanel {
    db: OuterDb,
    cfg: OuterConfig,
    disp: Vec<u8>,
    runs: [Option<Run>; 3],
    seen: [String; 5],
    started: bool,
    frame: Frame,
    frame_no: u32,
    log: Log,
}

impl OuterPanel {
    pub fn new(db: OuterDb, cfg: OuterConfig) -> OuterPanel {
        let strips = db.rows.div_ceil(8);
        let width = if cfg.width == 0 { db.width } else { cfg.width.min(db.width) };
        OuterPanel {
            disp: vec![0; db.rows * db.width],
            frame: Frame { width, strips: vec![vec![0; width]; strips] },
            runs: [None, None, None],
            seen: Default::default(),
            started: false,
            frame_no: 0,
            log: Log::new(),
            cfg,
            db,
        }
    }

    pub fn db(&self) -> &OuterDb {
        &self.db
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub fn frame_no(&self) -> u32 {
        self.frame_no
    }

    pub fn log(&mut self) -> &mut Log {
        &mut self.log
    }

    /// Okno pole pro dané vstupy (viz `OuterConfig::expand_dest`).
    fn window(&self, field: usize, inp: &Inputs) -> Option<Window> {
        let win = self.db.windows[field]?;
        match self.db.windows[FIELD_LINE] {
            Some(line) if self.cfg.expand_dest && field != FIELD_LINE && inp.line.trim().is_empty() && line.right < win.left => {
                Some(Window { left: line.left, ..win })
            }
            _ => Some(win),
        }
    }

    /// Bajty textu pole pro dané vstupy; `None` = pole zůstane prázdné.
    fn text_for(&self, field: usize, inp: &Inputs) -> Option<Vec<u8>> {
        let db = &self.db;
        let win = self.window(field, inp)?;
        if !self.cfg.fields[field] || win.right <= win.left {
            return None;
        }
        let fmt = db.formats[field];
        match field {
            FIELD_LINE => {
                let line = inp.line.trim();
                if line.is_empty() {
                    return None;
                }
                Some(match OuterDb::record(&db.lin, line) {
                    Some(r) => r.raw.clone(),
                    None => free_text(db, win, fmt, line),
                })
            }
            FIELD_DEST => {
                if inp.dest.is_empty() {
                    return None;
                }
                Some(match OuterDb::record(&db.cil, inp.dest.id) {
                    Some(r) => r.raw.clone(),
                    None if inp.dest.name.trim().is_empty() => return None,
                    None => free_text(db, win, fmt, inp.dest.name),
                })
            }
            _ => {
                if inp.next_stop.is_empty() {
                    return None;
                }
                Some(match OuterDb::record(&db.dru, inp.next_stop.id) {
                    Some(r) => r.raw.clone(),
                    None if inp.next_stop.name.trim().is_empty() => return None,
                    None if self.cfg.stop_dashes => free_text(db, win, fmt, &format!("-{}-", inp.next_stop.name.trim())),
                    None => free_text(db, win, fmt, inp.next_stop.name),
                })
            }
        }
    }

    fn rebuild(&mut self, inp: &Inputs) {
        self.disp.fill(0);
        for field in [FIELD_LINE, FIELD_STOP, FIELD_DEST] {
            self.runs[field] = self.text_for(field, inp).and_then(|raw| {
                let win = self.window(field, inp)?;
                Some(Run::new(raw, win, self.db.formats[field], self.db.rows))
            });
        }
        // cíl se kreslí poslední: `ESC b` / `ESC w` mu dávají přednost před linkou a zastávkou
        for field in [FIELD_LINE, FIELD_STOP, FIELD_DEST] {
            if let Some(run) = self.runs[field].as_mut() {
                run.step(&self.db, &mut self.disp, &mut self.log);
            }
        }
        self.apply_takeover();
    }

    fn apply_takeover(&mut self) {
        let (others, stop) = self.runs[FIELD_DEST].as_ref().map_or((false, false), |r| (r.hide_others, r.hide_stop));
        if others {
            self.runs[FIELD_LINE] = None;
        }
        if others || stop {
            self.runs[FIELD_STOP] = None;
        }
    }

    /// Posune čas o `dt` sekund; vrací snímek jen při změně.
    pub fn tick(&mut self, dt: f64, inp: &Inputs) -> Option<&Frame> {
        let now = [inp.line, inp.dest.id, inp.dest.name, inp.next_stop.id, inp.next_stop.name];
        let changed = !self.started || self.seen.iter().zip(now).any(|(a, b)| a != b);
        if changed {
            for (s, n) in self.seen.iter_mut().zip(now) {
                s.clear();
                s.push_str(n);
            }
            self.rebuild(inp);
        } else {
            let dt_ms = if dt.is_finite() { (dt * 1000.0).clamp(0.0, 10_000.0) } else { 0.0 };
            for field in [FIELD_LINE, FIELD_STOP, FIELD_DEST] {
                let Some(run) = self.runs[field].as_mut() else { continue };
                if run.done {
                    continue;
                }
                run.acc += dt_ms;
                let period = self.cfg.step_ms.max(20.0) * run.units as f64;
                if run.acc >= period {
                    run.acc -= period;
                    run.step(&self.db, &mut self.disp, &mut self.log);
                }
            }
            self.apply_takeover();
        }
        let (rows, width) = (self.db.rows, self.db.width);
        let mut dirty = !self.started;
        self.started = true;
        for (s, strip) in self.frame.strips.iter_mut().enumerate() {
            for (x, cell) in strip.iter_mut().enumerate() {
                let mut v = 0u8;
                for r in 0..8 {
                    let row = s * 8 + r;
                    if row < rows && self.disp[row * width + x] != 0 {
                        v |= 0x80 >> r;
                    }
                }
                if *cell != v {
                    *cell = v;
                    dirty = true;
                }
            }
        }
        if dirty {
            self.frame_no = self.frame_no.wrapping_add(1);
            Some(&self.frame)
        } else {
            None
        }
    }

    /// Displej jako text (`#` = svítí), řádek na řádek panelu.
    pub fn ascii(&self) -> String {
        let (rows, width) = (self.db.rows, self.db.width);
        (0..rows)
            .map(|r| (0..width).map(|x| if self.disp[r * width + x] != 0 { '#' } else { '.' }).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
