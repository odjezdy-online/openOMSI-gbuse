//! Tokenizace a vykreslení textu gBUSE1 na sloupce (bit 7 = horní řádek).

use crate::db::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Glyph(u8),
    Font(u8),
    Spacing(u8),
    Flag(u8),
    Placeholder,
    Esc(u8),
    Newline,
    Page,
}

/// Iterátor tokenů; končí na `0D`. Neznámé kódy zachovává jako `Glyph`.
pub struct Tokens<'a> {
    raw: &'a [u8],
    i: usize,
}

pub fn tokens(raw: &[u8]) -> Tokens<'_> {
    Tokens { raw, i: 0 }
}

impl Iterator for Tokens<'_> {
    type Item = Tok;

    fn next(&mut self) -> Option<Tok> {
        let b = *self.raw.get(self.i)?;
        if b == 0x0D {
            self.i = self.raw.len();
            return None;
        }
        if b == 0x1B && self.i + 1 < self.raw.len() {
            let v = self.raw[self.i + 1];
            self.i += 2;
            return Some(Tok::Esc(v));
        }
        self.i += 1;
        Some(match b {
            0xE0..=0xEF => Tok::Font(b),
            0xB0..=0xBF => Tok::Spacing(b - 0xB0),
            0xC0..=0xCF => Tok::Flag(b),
            0x26 => Tok::Placeholder,
            0x0A => Tok::Newline,
            0x0C => Tok::Page,
            _ => Tok::Glyph(b),
        })
    }
}

/// Co se vykreslí na místě zástupného znaku `&`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    /// Jako běžný glyf 0x26 aktuálního fontu (referenční dekodér; žádný font ho nemá -> nic).
    AsGlyph,
    /// Nic, bez vlivu na mezery.
    Hide,
    /// Prázdné sloupce dané šířky.
    Blank(u8),
    /// Glyf z daného fontu (výchozí: značka zastávky F1 z E1).
    Symbol { font: u8, glyph: u8 },
    /// Text DOP záznamu.
    Dop(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingGlyph {
    /// Vynechat (referenční dekodér, golden testy).
    Skip,
    /// Rámeček 5 sloupců.
    Box,
    /// Glyf z výchozího fontu (např. kotva `$` je jen v E1); když není ani tam, rámeček.
    Fallback,
}

/// Mapování „glyf v neznámém fontu" -> glyf existujícího fontu (HYPOTÉZA: E5-E9 jsou
/// piktogramové fonty ve firmwaru panelu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictAlias {
    pub font: u8,
    pub glyph: u8,
    pub to_font: u8,
    pub to_glyph: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOpts {
    pub default_font: u8,
    pub default_spacing: u8,
    pub placeholder: Placeholder,
    /// Když je `Some(f)`, platí `placeholder` jen pro texty s příznakem `f` (HYPOTÉZA:
    /// C0 = zastávka na znamení); ostatní texty použijí `placeholder_else`.
    pub placeholder_flag: Option<u8>,
    pub placeholder_else: Placeholder,
    pub missing_glyph: MissingGlyph,
    pub pict_alias: Vec<PictAlias>,
}

impl RenderOpts {
    /// Chování referenčního dekodéru `gbuse_decode.py` (golden testy).
    pub fn reference() -> Self {
        RenderOpts {
            default_font: 0xE1,
            default_spacing: 1,
            placeholder: Placeholder::AsGlyph,
            placeholder_flag: None,
            placeholder_else: Placeholder::AsGlyph,
            missing_glyph: MissingGlyph::Skip,
            pict_alias: Vec::new(),
        }
    }
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts {
            default_font: 0xE1,
            default_spacing: 1,
            // `&` žádný font brněnské databáze nemá, takže se nekreslí; `C0` je podle vnějších
            // panelů (gBUSE0: C0 = horní řádek, CA = o 10 řádků níž) svislý posun, ne příznak
            // zastávky na znamení. Symbol za `&` u záznamů s příznakem jde zapnout v configu.
            placeholder: Placeholder::Hide,
            placeholder_flag: None,
            placeholder_else: Placeholder::Hide,
            missing_glyph: MissingGlyph::Fallback,
            pict_alias: Vec::new(),
        }
    }
}

/// Jednorázové hlášky (neznámý font/glyf se loguje jednou za běh).
#[derive(Debug, Default)]
pub struct Log {
    fonts: u16,
    glyphs: Vec<(u8, u8)>,
    other: Vec<u32>,
    msgs: Vec<String>,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unknown_font(&mut self, font: u8) {
        let bit = 1u16 << (font & 0x0F);
        if self.fonts & bit == 0 {
            self.fonts |= bit;
            self.msgs.push(format!("neznámý font {font:02X}, použit výchozí"));
        }
    }

    pub fn missing_glyph(&mut self, font: u8, code: u8) {
        if !self.glyphs.contains(&(font, code)) {
            self.glyphs.push((font, code));
            self.msgs.push(format!("font {font:02X} nemá glyf {code:02X}"));
        }
    }

    /// Hláška, která se zapíše jen jednou pro daný klíč.
    pub fn once(&mut self, key: u32, msg: impl FnOnce() -> String) {
        if !self.other.contains(&key) {
            self.other.push(key);
            self.msgs.push(msg());
        }
    }

    pub fn has_messages(&self) -> bool {
        !self.msgs.is_empty()
    }

    /// Vybere nasbírané hlášky (volající je zapíše do souboru / game.log).
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.msgs)
    }
}

const BOX: [u8; 5] = [0x7F, 0x41, 0x41, 0x41, 0x7F];

struct State<'a> {
    db: &'a Db,
    opts: &'a RenderOpts,
    font: u8,
    /// Naposledy zvolený font, který v databázi není (pro `pict_alias`).
    unknown: Option<u8>,
    sp: u8,
    first: bool,
}

fn emit(out: &mut Vec<u8>, st: &mut State, cols: impl ExactSizeIterator<Item = u8>) {
    if !st.first {
        out.extend(std::iter::repeat(0).take(st.sp as usize));
    }
    out.extend(cols);
    st.first = false;
}

fn glyph(out: &mut Vec<u8>, st: &mut State, log: &mut Log, font: u8, code: u8) {
    let (db, opts) = (st.db, st.opts);
    let g = db.font(font).or_else(|| db.font(opts.default_font)).and_then(|f| f.glyph(code));
    match g {
        Some(g) => emit(out, st, g.cols.iter().map(|&c| c as u8)),
        None => {
            log.missing_glyph(font, code);
            let alt = db.font(opts.default_font).and_then(|f| f.glyph(code));
            match (opts.missing_glyph, alt) {
                (MissingGlyph::Skip, _) => {}
                (MissingGlyph::Fallback, Some(g)) => emit(out, st, g.cols.iter().map(|&c| c as u8)),
                _ => emit(out, st, BOX.iter().copied()),
            }
        }
    }
}

fn run(out: &mut Vec<u8>, st: &mut State, log: &mut Log, raw: &[u8], ph: Placeholder, depth: u8) {
    let (db, opts) = (st.db, st.opts);
    for tok in tokens(raw) {
        match tok {
            Tok::Font(f) => {
                if db.font(f).is_some() {
                    st.font = f;
                    st.unknown = None;
                } else {
                    log.unknown_font(f);
                    st.font = opts.default_font;
                    st.unknown = Some(f);
                }
            }
            Tok::Spacing(v) => st.sp = v,
            Tok::Glyph(code) => {
                let alias = st
                    .unknown
                    .and_then(|u| opts.pict_alias.iter().find(|a| a.font == u && a.glyph == code));
                match alias {
                    Some(a) => glyph(out, st, log, a.to_font, a.to_glyph),
                    None => {
                        let font = st.font;
                        glyph(out, st, log, font, code)
                    }
                }
            }
            Tok::Placeholder => match ph {
                Placeholder::AsGlyph => {
                    // referenční chování: glyf 0x26, když ho font má, jinak nic (bez hlášky)
                    let f = db.font(st.font).or_else(|| db.font(opts.default_font));
                    if let Some(g) = f.and_then(|f| f.glyph(0x26)) {
                        emit(out, st, g.cols.iter().map(|&c| c as u8));
                    }
                }
                Placeholder::Hide => {}
                Placeholder::Blank(n) => emit(out, st, std::iter::repeat(0).take(n as usize)),
                Placeholder::Symbol { font, glyph: code } => glyph(out, st, log, font, code),
                Placeholder::Dop(id) => {
                    if let (Some(dop), true) = (db.dop(id), depth == 0) {
                        let (font, sp, unknown) = (st.font, st.sp, st.unknown);
                        run(out, st, log, dop, Placeholder::Hide, depth + 1);
                        (st.font, st.sp, st.unknown) = (font, sp, unknown);
                    }
                }
            },
            Tok::Flag(_) | Tok::Esc(_) | Tok::Newline | Tok::Page => {}
        }
    }
}

/// Má text daný příznak (`C0`-`CF`)?
pub fn has_flag(raw: &[u8], flag: u8) -> bool {
    tokens(raw).any(|t| t == Tok::Flag(flag))
}

/// Vykreslí několik navazujících částí (DOP šablona + hodnota) do `out`; stav fontu a mezer
/// přechází z části do části. `flag_from` = část, podle jejíchž příznaků se řídí `&`.
pub fn render_parts(db: &Db, parts: &[&[u8]], opts: &RenderOpts, out: &mut Vec<u8>, log: &mut Log) {
    let flagged = match opts.placeholder_flag {
        None => true,
        Some(f) => parts.iter().any(|p| has_flag(p, f)),
    };
    let ph = if flagged { opts.placeholder } else { opts.placeholder_else };
    let mut st = State {
        db,
        opts,
        font: opts.default_font,
        unknown: None,
        sp: opts.default_spacing,
        first: true,
    };
    for p in parts {
        run(out, &mut st, log, p, ph, 0);
    }
}

/// Vykreslí text na sloupce (připojí je do `out`).
pub fn render_into(db: &Db, raw: &[u8], opts: &RenderOpts, out: &mut Vec<u8>, log: &mut Log) {
    render_parts(db, &[raw], opts, out, log);
}

/// Vykreslí text na sloupce; bit 7 = horní řádek.
pub fn render_text(db: &Db, raw: &[u8], opts: &RenderOpts, log: &mut Log) -> Vec<u8> {
    let mut out = Vec::new();
    render_into(db, raw, opts, &mut out, log);
    out
}

/// Čitelný text záznamu (port `plain` z referenčního dekodéru).
pub fn plain(db: &Db, raw: &[u8]) -> String {
    let mut s = String::new();
    for tok in tokens(raw) {
        match tok {
            Tok::Glyph(c) => match db.code_to_char(c) {
                Some(ch) => s.push(ch),
                None => s.push_str(&format!("[{c:02X}]")),
            },
            Tok::Placeholder => s.push('&'),
            Tok::Spacing(v) if v >= 2 && !s.is_empty() && !s.ends_with(' ') => s.push(' '),
            _ => {}
        }
    }
    s
}

/// Sloupce jako ASCII art (`#` svítí), `h` řádků.
pub fn ascii_art(cols: &[u8], h: usize) -> String {
    let mut s = String::with_capacity((cols.len() + 1) * h);
    for y in 0..h {
        for &c in cols {
            s.push(if (c >> (h - 1 - y)) & 1 != 0 { '#' } else { '.' });
        }
        if y + 1 < h {
            s.push('\n');
        }
    }
    s
}
