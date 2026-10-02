//! Databáze gBUSE1: sekce, fonty, charmap, tabulky LIN/ZST, DOP a CYK.
//! Port `tools/gbuse_decode.py`; offsety se nehardcodují, hledají se hlavičky sekcí.

use crate::hex::{parse_intel_hex, HexError};
use std::fmt;

/// Magic kompaktní binárky `buse_db.bin` (za ním následuje surový obraz databáze).
pub const BIN_MAGIC: &[u8; 8] = b"BUSEDB1\0";

#[derive(Debug)]
pub enum DbError {
    Hex(HexError),
    BadMagic,
    Font { id: u8, at: usize, end: usize },
    NoSections,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Hex(e) => e.fmt(f),
            DbError::BadMagic => write!(f, "buse_db.bin: chybí magic BUSEDB1"),
            DbError::Font { id, at, end } => {
                write!(f, "font {id:02X}: nesedí délka ({at:#x} != {end:#x})")
            }
            DbError::NoSections => write!(f, "v obrazu není žádná sekce gBUSE"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<HexError> for DbError {
    fn from(e: HexError) -> Self {
        DbError::Hex(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub tag: String,
    pub gbuse: String,
    pub version: String,
    pub header: usize,
    pub data: usize,
}

#[derive(Debug, Clone, Copy)]
struct GlyphRef {
    start: u32,
    w: u8,
    h: u8,
}

/// Font = až 256 glyfů, sloupce všech glyfů leží v jednom poli.
#[derive(Debug, Clone)]
pub struct Font {
    pub id: u8,
    glyphs: Box<[Option<GlyphRef>; 256]>,
    cols: Vec<u16>,
}

#[derive(Debug, Clone, Copy)]
pub struct Glyph<'a> {
    pub w: u8,
    pub h: u8,
    /// Sloupce zleva; bit (h-1) = horní řádek, bit 0 = spodní.
    pub cols: &'a [u16],
}

impl Font {
    pub fn glyph(&self, code: u8) -> Option<Glyph<'_>> {
        self.glyphs[code as usize].map(|g| Glyph {
            w: g.w,
            h: g.h,
            cols: &self.cols[g.start as usize..g.start as usize + g.w as usize],
        })
    }

    pub fn codes(&self) -> impl Iterator<Item = u8> + '_ {
        (0..=255u8).filter(|&c| self.glyphs[c as usize].is_some())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    pub raw: Vec<u8>,
}

/// Položka cyklu: pole (šířka, šablona, proměnná, režim, doba), nebo konec stránky (`brk`).
///
/// Pole se na panelu skládají vedle sebe zleva, dokud se vejdou do jeho šířky: v brněnské
/// databázi linka (22 sloupců) + cíl (112) a zóna (67) + čas (67); pole přes celou šířku
/// (135: zastávka, mimořádná informace) je stránka samo. Šířky sedí s daty: nejširší LIN
/// má 22 sloupců, nejširší ZST 103 + šipka z DOP 2. Režim a doba jsou HYPOTÉZA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CyclePage {
    /// Šířka pole ve sloupcích (u konce stránky 0).
    pub width: u8,
    pub dop: u8,
    pub var: u8,
    /// 03 u linky, cíle, zastávky a informace, 00 u zóny a času (HYPOTÉZA: zarovnání na
    /// střed a nasouvání).
    pub mode: u8,
    /// HYPOTÉZA: sekundy; 00 = do odvolání.
    pub time: u8,
    /// Samotný bajt 00 mezi poli: konec stránky.
    pub brk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    pub id: u8,
    pub raw: Vec<u8>,
    pub pages: Vec<CyclePage>,
    pub names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Db {
    /// Surový obraz (zachován celý, včetně neznámé hlavičky 0x00-0x3F).
    pub image: Vec<u8>,
    pub name: Option<String>,
    pub sections: Vec<Section>,
    fonts: [Option<Box<Font>>; 16],
    /// Index = kód - 0x20; hodnota = bajt ve Windows-1250 (0 = nepoužito).
    pub charmap: [u8; 0xC0],
    pub cyk: Vec<Cycle>,
    pub dop: Vec<(u8, Vec<u8>)>,
    pub lin: Vec<Record>,
    pub zst: Vec<Record>,
    /// Tabulky gBUSE0 (vnější panely) - jen načtené, engine je nepoužívá.
    pub cil: Vec<Record>,
    pub dru: Vec<Record>,
}

const CP1250_HIGH: [char; 128] = [
    '€', '\u{81}', '‚', '\u{83}', '„', '…', '†', '‡', '\u{88}', '‰', 'Š', '‹', 'Ś', 'Ť', 'Ž', 'Ź',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '\u{98}', '™', 'š', '›', 'ś', 'ť', 'ž', 'ź',
    '\u{A0}', 'ˇ', '˘', 'Ł', '¤', 'Ą', '¦', '§', '¨', '©', 'Ş', '«', '¬', '\u{AD}', '®', 'Ż',
    '°', '±', '˛', 'ł', '´', 'µ', '¶', '·', '¸', 'ą', 'ş', '»', 'Ľ', '˝', 'ľ', 'ż',
    'Ŕ', 'Á', 'Â', 'Ă', 'Ä', 'Ĺ', 'Ć', 'Ç', 'Č', 'É', 'Ę', 'Ë', 'Ě', 'Í', 'Î', 'Ď',
    'Đ', 'Ń', 'Ň', 'Ó', 'Ô', 'Ő', 'Ö', '×', 'Ř', 'Ů', 'Ú', 'Ű', 'Ü', 'Ý', 'Ţ', 'ß',
    'ŕ', 'á', 'â', 'ă', 'ä', 'ĺ', 'ć', 'ç', 'č', 'é', 'ę', 'ë', 'ě', 'í', 'î', 'ď',
    'đ', 'ń', 'ň', 'ó', 'ô', 'ő', 'ö', '÷', 'ř', 'ů', 'ú', 'ű', 'ü', 'ý', 'ţ', '˙',
];

pub fn cp1250_to_char(b: u8) -> char {
    if b < 0x80 {
        b as char
    } else {
        CP1250_HIGH[(b - 0x80) as usize]
    }
}

pub fn char_to_cp1250(c: char) -> Option<u8> {
    if (c as u32) < 0x80 {
        return Some(c as u8);
    }
    CP1250_HIGH.iter().position(|&x| x == c).map(|i| i as u8 + 0x80)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Odpovídá regexu `([A-Z]{3}):.{0,2}gBUSE(\d\w*) ?- ?([\d.]+)` z referenčního dekodéru.
pub(crate) fn find_sections(img: &[u8]) -> Vec<Section> {
    let mut out = Vec::new();
    let mut g = 0;
    while g + 5 <= img.len() {
        if &img[g..g + 5] != b"gBUSE" {
            g += 1;
            continue;
        }
        let mut start = None;
        for k in (0..=2usize).rev() {
            if g < k + 4 {
                continue;
            }
            let colon = g - k - 1;
            if img[colon] == b':'
                && img[colon - 3..colon].iter().all(|b| b.is_ascii_uppercase())
                && img[colon + 1..g].iter().all(|&b| b != b'\n')
            {
                start = Some(colon - 3);
                break;
            }
        }
        let mut p = g + 5;
        let ver0 = p;
        if let (Some(start), true) = (start, p < img.len() && img[p].is_ascii_digit()) {
            p += 1;
            while p < img.len() && is_word(img[p]) {
                p += 1;
            }
            let gbuse = String::from_utf8_lossy(&img[ver0..p]).into_owned();
            if p < img.len() && img[p] == b' ' {
                p += 1;
            }
            if p < img.len() && img[p] == b'-' {
                p += 1;
                if p < img.len() && img[p] == b' ' {
                    p += 1;
                }
                let v0 = p;
                while p < img.len() && (img[p].is_ascii_digit() || img[p] == b'.') {
                    p += 1;
                }
                if p > v0 {
                    out.push(Section {
                        tag: String::from_utf8_lossy(&img[start..start + 3]).into_owned(),
                        gbuse,
                        version: String::from_utf8_lossy(&img[v0..p]).into_owned(),
                        header: start,
                        data: (start + 0x20) & !0x1F,
                    });
                }
            }
        }
        g += 5;
    }
    out
}

fn parse_fonts(img: &[u8], start: usize) -> Result<([Option<Box<Font>>; 16], usize), DbError> {
    let mut fonts: [Option<Box<Font>>; 16] = Default::default();
    let mut p = start;
    while p + 3 <= img.len() && (0xE0..=0xEF).contains(&img[p]) {
        let fid = img[p];
        let ln = (img[p + 1] as usize) << 8 | img[p + 2] as usize;
        let end = p + 3 + ln;
        let mut q = p + 3;
        let mut font = Font { id: fid, glyphs: Box::new([None; 256]), cols: Vec::new() };
        while q < end && q + 3 <= img.len() {
            if q + 2 == end && img[q] == 0xFF && img[q + 1] == 0 {
                // gBUSE1 1.00 (Košice): seznam glyfů končí dvojicí FF 00
                q = end;
                break;
            }
            // [kód][počet bajtů dat][výška][data]: u fontů výšky <= 8 je počet bajtů totéž
            // co šířka, u vyšších (gBUSE0) šířka krát počet bajtů sloupce
            let (code, size, h) = (img[q], img[q + 1] as usize, img[q + 2] as usize);
            let bpc = h.div_ceil(8).max(1);
            let w = size / bpc;
            if q + 3 + size > img.len() || bpc > 2 {
                return Err(DbError::Font { id: fid, at: q, end });
            }
            let raw = &img[q + 3..q + 3 + size];
            let gstart = font.cols.len() as u32;
            for x in 0..w {
                let mut v: u32 = 0;
                for k in 0..bpc {
                    v = v << 8 | raw[x * bpc + k] as u32;
                }
                if bpc * 8 > h {
                    v >>= bpc * 8 - h;
                }
                font.cols.push(v as u16);
            }
            font.glyphs[code as usize] = Some(GlyphRef { start: gstart, w: w as u8, h: h as u8 });
            q += 3 + size;
        }
        if q != end {
            return Err(DbError::Font { id: fid, at: q, end });
        }
        fonts[(fid - 0xE0) as usize] = Some(Box::new(font));
        p = end;
    }
    Ok((fonts, p))
}

fn parse_charmap(img: &[u8], mut p: usize) -> [u8; 0xC0] {
    while p < img.len() && img[p] == 0xFF {
        p += 1;
    }
    let mut map = [0u8; 0xC0];
    for (i, slot) in map.iter_mut().enumerate() {
        if let Some(&v) = img.get(p + i) {
            *slot = v;
        }
    }
    map
}

/// Délka id záznamu: 3 (LIN, CIL) nebo 4 (ZST, DRU; CIL s adresami v gBUSE0 1.21). Správná je
/// ta, se kterou řetěz záznamů dojde čistě k 0xFF.
fn table_idlen(img: &[u8], start: usize, default: usize) -> usize {
    for idlen in [default, 7 - default] {
        let (mut p, mut ok, mut n) = (start, true, 0);
        while p + idlen + 2 <= img.len() && img[p] != 0xFF {
            let len = img[p + idlen] as usize | (img[p + idlen + 1] as usize) << 8;
            let end = p + idlen + 2 + len;
            if !img[p..p + idlen].iter().all(|c| (0x20..0x7F).contains(c)) || end > img.len() || (len > 0 && img[end - 1] != 0x0D) {
                ok = false;
                break;
            }
            p = end;
            n += 1;
        }
        if ok && n > 0 {
            return idlen;
        }
    }
    default
}

pub(crate) fn parse_table(img: &[u8], start: usize, idlen: usize) -> Vec<Record> {
    let idlen = table_idlen(img, start, idlen);
    let mut rows = Vec::new();
    let mut p = start;
    while p + idlen + 2 <= img.len() && img[p] != 0xFF {
        let id: String = img[p..p + idlen]
            .iter()
            .map(|&b| if b < 0x80 { b as char } else { '\u{FFFD}' })
            .collect();
        let n = img[p + idlen] as usize | (img[p + idlen + 1] as usize) << 8;
        let a = p + idlen + 2;
        let b = (a + n).min(img.len());
        rows.push(Record { id, raw: img[a..b].to_vec() });
        p = a + n;
    }
    rows
}

fn parse_records(img: &[u8], start: usize) -> Vec<(u8, Vec<u8>)> {
    let mut rows = Vec::new();
    let mut p = start;
    while p + 3 <= img.len() && img[p] != 0xFF {
        let n = img[p + 1] as usize | (img[p + 2] as usize) << 8;
        let a = p + 3;
        let b = (a + n).min(img.len());
        rows.push((img[p], img[a..b].to_vec()));
        p = a + n;
    }
    rows
}

/// `[00 00][položky...][FF][00 00][jména položek jako Pascal stringy]`; položka je pole
/// (5 B: šířka, DOP, proměnná, režim, doba) nebo samotný bajt 00 = konec stránky (jeho
/// jméno je prázdné, proto má cyklus 6 jména „zastávka", „", „mim.inf.", „").
fn parse_cycle(id: u8, data: &[u8]) -> Cycle {
    let mut p = 2;
    let mut pages = Vec::new();
    while p < data.len() && data[p] != 0xFF {
        if data[p] == 0 {
            pages.push(CyclePage { width: 0, dop: 0, var: 0, mode: 0, time: 0, brk: true });
            p += 1;
            continue;
        }
        if p + 5 > data.len() {
            break;
        }
        let r = &data[p..p + 5];
        pages.push(CyclePage { width: r[0], dop: r[1], var: r[2], mode: r[3], time: r[4], brk: false });
        p += 5;
    }
    p += 3;
    let mut names = Vec::new();
    while p < data.len() {
        let n = data[p] as usize;
        p += 1;
        let end = (p + n).min(data.len());
        names.push(data[p..end].iter().map(|&b| cp1250_to_char(b)).collect());
        p += n;
    }
    Cycle { id, raw: data.to_vec(), pages, names }
}

fn find_name(header: &[u8]) -> Option<String> {
    // regex [A-Z]\w{5,7}_\d{6}
    for s in 0..header.len() {
        if !header[s].is_ascii_uppercase() {
            continue;
        }
        for n in (5..=7).rev() {
            let u = s + 1 + n;
            if u + 7 <= header.len()
                && header[s + 1..u].iter().all(|&b| is_word(b))
                && header[u] == b'_'
                && header[u + 1..u + 7].iter().all(|b| b.is_ascii_digit())
            {
                return Some(String::from_utf8_lossy(&header[s..u + 7]).into_owned());
            }
        }
    }
    None
}

impl Db {
    /// Načte databázi z textu Intel HEX souboru (`ADledA.hex`).
    pub fn from_hex(text: &str) -> Result<Db, DbError> {
        Db::from_image(parse_intel_hex(text)?)
    }

    /// Načte databázi z `buse_db.bin` (magic + surový obraz).
    pub fn from_bin(bin: &[u8]) -> Result<Db, DbError> {
        match bin.strip_prefix(BIN_MAGIC) {
            Some(img) => Db::from_image(img.to_vec()),
            None => Err(DbError::BadMagic),
        }
    }

    pub fn to_bin(&self) -> Vec<u8> {
        let mut out = BIN_MAGIC.to_vec();
        out.extend_from_slice(&self.image);
        out
    }

    pub fn from_image(image: Vec<u8>) -> Result<Db, DbError> {
        let sections = find_sections(&image);
        if sections.is_empty() {
            return Err(DbError::NoSections);
        }
        let mut db = Db {
            name: find_name(&image[..image.len().min(0x40)]),
            fonts: Default::default(),
            charmap: [0; 0xC0],
            cyk: Vec::new(),
            dop: Vec::new(),
            lin: Vec::new(),
            zst: Vec::new(),
            cil: Vec::new(),
            dru: Vec::new(),
            sections,
            image,
        };
        let img = &db.image;
        for s in &db.sections {
            match s.tag.as_str() {
                "FNT" if s.gbuse == "1" => {
                    let (fonts, end) = parse_fonts(img, s.data)?;
                    db.fonts = fonts;
                    db.charmap = parse_charmap(img, end);
                }
                "LIN" => db.lin = parse_table(img, s.data, 3),
                "CIL" => db.cil = parse_table(img, s.data, 3),
                "ZST" => db.zst = parse_table(img, s.data, 4),
                "DRU" => db.dru = parse_table(img, s.data, 4),
                "DOP" => db.dop = parse_records(img, s.data),
                "CYK" => {
                    db.cyk = parse_records(img, s.data)
                        .into_iter()
                        .map(|(id, d)| parse_cycle(id, &d))
                        .collect()
                }
                _ => {}
            }
        }
        Ok(db)
    }

    pub fn font(&self, id: u8) -> Option<&Font> {
        if (0xE0..=0xEF).contains(&id) {
            self.fonts[(id - 0xE0) as usize].as_deref()
        } else {
            None
        }
    }

    pub fn fonts(&self) -> impl Iterator<Item = &Font> {
        self.fonts.iter().filter_map(|f| f.as_deref())
    }

    pub fn lin(&self, id: &str) -> Option<&Record> {
        self.lin.iter().find(|r| r.id == id)
    }

    pub fn zst(&self, id: &str) -> Option<&Record> {
        self.zst.iter().find(|r| r.id == id)
    }

    pub fn dop(&self, id: u8) -> Option<&[u8]> {
        self.dop.iter().find(|r| r.0 == id).map(|r| r.1.as_slice())
    }

    pub fn cycle(&self, id: u8) -> Option<&Cycle> {
        self.cyk.iter().find(|c| c.id == id)
    }

    /// Šířka panelu, pro který je databáze udělaná: hlavička gBUSE1 (první bajt AA nebo AB)
    /// má na třetím místě číslo posledního sloupce. Brno i Košice: 0x86 = 134, tedy 135
    /// sloupců - BS 120.0A má podle certifikátu IDS JMK 8 x 135 bodů.
    pub fn panel_width(&self) -> Option<usize> {
        match self.image.first() {
            Some(0xAA | 0xAB) => self.image.get(2).map(|&last| last as usize + 1).filter(|w| *w >= 8),
            _ => None,
        }
    }

    /// Znak pro kód glyfu podle charmapy databáze (ASCII přímo).
    pub fn code_to_char(&self, code: u8) -> Option<char> {
        if (0x20..0x80).contains(&code) {
            return Some(code as char);
        }
        match self.charmap.get(code.wrapping_sub(0x20) as usize) {
            Some(&v) if v != 0 && code >= 0x20 => Some(cp1250_to_char(v)),
            _ => None,
        }
    }

    /// Kód glyfu pro znak (inverze charmapy); `None`, když ho databáze nezná.
    pub fn char_to_code(&self, c: char) -> Option<u8> {
        if (' '..='\u{7E}').contains(&c) {
            return Some(c as u8);
        }
        let b = char_to_cp1250(c)?;
        self.charmap.iter().position(|&v| v == b).map(|i| i as u8 + 0x20)
    }

    /// Popis pro log: jméno, velikost, sekce a surová (neznámá) hlavička.
    pub fn describe(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let _ = write!(
            s,
            "databáze {} ({} B): {} fontů, {} linek, {} zastávek, {} DOP, {} cyklů; sekce:",
            self.name.as_deref().unwrap_or("?"),
            self.image.len(),
            self.fonts().count(),
            self.lin.len(),
            self.zst.len(),
            self.dop.len(),
            self.cyk.len()
        );
        for sec in &self.sections {
            let _ = write!(s, " {}@{:#06x}(gBUSE{} {})", sec.tag, sec.header, sec.gbuse, sec.version);
        }
        s.push_str("; hlavička:");
        for b in &self.image[..self.image.len().min(0x40)] {
            let _ = write!(s, " {b:02x}");
        }
        s
    }
}
