//! Stavový automat panelu: stránky (cykly), běžící text, nasouvání.
//!
//! `Panel::tick(dt, &Inputs)` vrací snímek jen tehdy, když se změnil. V horké cestě se
//! nealokuje: buffery se při změně stránky přepisují na místě.

use crate::config::{default_dop, Align, Config, Field, PageSpec, Slide, Var};
use crate::db::Db;
use crate::names::{encode_text, NameIndex};
use crate::text::{render_parts, Log, Placeholder, RenderOpts};

/// Odkaz na zastávku / cíl: ZST id (když ho volající zná) a název z hry (fallback).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StopRef<'a> {
    pub id: &'a str,
    pub name: &'a str,
}

impl StopRef<'_> {
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.name.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Inputs<'a> {
    /// Číslo linky (doplní se nulami na 3 číslice a hledá v LIN).
    pub line: &'a str,
    pub dest: StopRef<'a>,
    pub next_stop: StopRef<'a>,
    /// Svítí STOP (cestující stiskl tlačítko).
    pub stop_pressed: bool,
    /// Příští zastávka je na znamení; `None` = řídí se příznakem z `placeholder_flag`.
    pub request_stop: Option<bool>,
    /// Herní čas v sekundách od půlnoci.
    pub time_s: Option<u32>,
    pub zone: &'a str,
    pub info: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    /// Pruhy po 8 řádcích shora dolů; sloupec = bajt, bit 7 = horní řádek pruhu.
    pub strips: Vec<Vec<u8>>,
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

impl Frame {
    /// „Nibble row": jeden hex znak na sloupec; `hi` = horní 4 řádky pruhu, jinak spodní 4.
    /// Nejvyšší bit znaku = horní řádek čtveřice. `out` musí mít aspoň `width` prvků.
    pub fn nibble_row_utf16(&self, strip: usize, hi: bool, out: &mut [u16]) {
        for (o, &c) in out.iter_mut().zip(&self.strips[strip]) {
            *o = HEX[(if hi { c >> 4 } else { c & 15 }) as usize] as u16;
        }
    }

    pub fn nibble_row(&self, strip: usize, hi: bool) -> String {
        self.strips[strip].iter().map(|&c| HEX[(if hi { c >> 4 } else { c & 15 }) as usize] as char).collect()
    }

    pub fn ascii(&self) -> String {
        let mut s = String::new();
        for (i, strip) in self.strips.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&crate::text::ascii_art(strip, 8));
        }
        s
    }
}

const CH_LINE: u8 = 1;
const CH_DEST: u8 = 2;
const CH_STOP: u8 = 4;
const CH_INFO: u8 = 8;
const CH_TIME: u8 = 16;
const CH_ZONE: u8 = 32;
const CH_PRESS: u8 = 64;

fn var_mask(v: Var) -> u8 {
    match v {
        Var::Line => CH_LINE,
        Var::Dest => CH_DEST,
        Var::NextStop => CH_STOP,
        Var::Info => CH_INFO,
        Var::Time => CH_TIME,
        Var::Zone => CH_ZONE,
        Var::LineDest => CH_LINE | CH_DEST,
        Var::Unknown(_) => 0,
    }
}

fn spec_mask(spec: &PageSpec) -> u8 {
    spec.fields().iter().fold(0, |m, f| m | var_mask(f.var))
}

/// Kopie vstupů z minulého snímku (pro detekci změn; Stringy si drží kapacitu).
#[derive(Debug, Default)]
struct Seen {
    line: String,
    dest_id: String,
    dest_name: String,
    stop_id: String,
    stop_name: String,
    zone: String,
    info: String,
    minute: Option<u32>,
    pressed: bool,
    request: Option<bool>,
    first: bool,
}

fn update(dst: &mut String, src: &str) -> bool {
    if dst == src {
        return false;
    }
    dst.clear();
    dst.push_str(src);
    true
}

impl Seen {
    fn absorb(&mut self, inp: &Inputs) -> u8 {
        let mut ch = 0;
        if update(&mut self.line, inp.line) {
            ch |= CH_LINE;
        }
        if update(&mut self.dest_id, inp.dest.id) | update(&mut self.dest_name, inp.dest.name) {
            ch |= CH_DEST;
        }
        if update(&mut self.stop_id, inp.next_stop.id) | update(&mut self.stop_name, inp.next_stop.name) {
            ch |= CH_STOP;
        }
        if update(&mut self.zone, inp.zone) {
            ch |= CH_ZONE;
        }
        if update(&mut self.info, inp.info) {
            ch |= CH_INFO;
        }
        let minute = inp.time_s.map(|t| t / 60);
        if minute != self.minute {
            self.minute = minute;
            ch |= CH_TIME;
        }
        if inp.request_stop != self.request {
            self.request = inp.request_stop;
            ch |= CH_STOP;
        }
        if inp.stop_pressed && !self.pressed {
            ch |= CH_PRESS;
        }
        self.pressed = inp.stop_pressed;
        if !self.first {
            // první snímek není „změna zastávky" - panel začíná v klidovém cyklu
            self.first = true;
            ch &= !(CH_STOP | CH_PRESS);
        }
        ch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Idle,
    Stop,
}

#[derive(Debug)]
struct Row {
    idle: Vec<PageSpec>,
    stop: Vec<PageSpec>,
    mode: Mode,
    idx: usize,
    /// Je načtená nějaká neprázdná stránka?
    valid: bool,
    /// Stránka byla načtena v tomto ticku (časovače se rozběhnou až v příštím).
    fresh: bool,
    started: bool,
    spec: PageSpec,
    /// Obsah aktuální stránky (celá šířka textu).
    cur: Vec<u8>,
    /// Co bylo vidět před přechodem (šířka panelu).
    old: Vec<u8>,
    /// Co je vidět teď (šířka panelu).
    vis: Vec<u8>,
    slide_left: u32,
    slide_acc: f64,
    scroll_off: usize,
    scroll_acc: f64,
    scroll_wait: f64,
    passes: u32,
    elapsed: f64,
}

const BLANK: PageSpec = PageSpec::empty();

impl Row {
    fn new(width: usize, idle: Vec<PageSpec>, stop: Vec<PageSpec>) -> Row {
        Row {
            idle,
            stop,
            mode: Mode::Idle,
            idx: 0,
            valid: false,
            fresh: false,
            started: false,
            spec: BLANK,
            cur: Vec::with_capacity(512),
            old: vec![0; width],
            vis: vec![0; width],
            slide_left: 0,
            slide_acc: 0.0,
            scroll_off: 0,
            scroll_acc: 0.0,
            scroll_wait: 0.0,
            passes: 0,
            elapsed: 0.0,
        }
    }

    fn seq(&self, mode: Mode) -> &[PageSpec] {
        match mode {
            Mode::Idle => &self.idle,
            Mode::Stop => &self.stop,
        }
    }
}

/// Všechno, co je potřeba k sestavení obsahu stránky (odděleno od `Row` kvůli borrow checkeru).
struct Builder<'a> {
    db: &'a Db,
    cfg: &'a Config,
    index: &'a NameIndex,
    opts_dest: &'a RenderOpts,
    opts_on: &'a RenderOpts,
    opts_off: &'a RenderOpts,
    log: &'a mut Log,
    scratch: &'a mut Vec<u8>,
    part: &'a mut Vec<u8>,
    id_buf: &'a mut String,
}

fn pad_id(buf: &mut String, id: &str, len: usize) {
    buf.clear();
    let id = id.trim();
    if !id.is_empty() && id.len() <= len && id.bytes().all(|b| b.is_ascii_digit()) {
        for _ in id.len()..len {
            buf.push('0');
        }
    }
    buf.push_str(id);
}

fn dop_text(db: &Db, id: u8) -> &[u8] {
    if id == 0 {
        return &[];
    }
    db.dop(id).unwrap_or(&[])
}

impl Builder<'_> {
    fn line(&mut self, dop: u8, inp: &Inputs, out: &mut Vec<u8>) -> bool {
        if inp.line.trim().is_empty() {
            return false;
        }
        let (db, cfg) = (self.db, self.cfg);
        pad_id(self.id_buf, inp.line, 3);
        let prefix = dop_text(db, dop);
        match db.lin(self.id_buf) {
            Some(rec) => render_parts(db, &[prefix, &rec.raw], &cfg.render, out, self.log),
            None => {
                self.scratch.clear();
                self.scratch.extend([cfg.fallback_line_font, 0xB1]);
                encode_text(db, inp.line.trim(), self.scratch);
                render_parts(db, &[prefix, self.scratch], &cfg.render, out, self.log);
            }
        }
        true
    }

    /// `dest`: cíl. Databáze verze 1.xx (Košice) mají cíle ve vlastní tabulce CIL s třímístným
    /// id; verze 2.00 (Brno) berou cíl ze zastávek.
    fn stop(&mut self, dop: u8, s: StopRef, dest: bool, opts: &RenderOpts, out: &mut Vec<u8>) -> bool {
        if s.is_empty() {
            return false;
        }
        let (db, cfg, index) = (self.db, self.cfg, self.index);
        let cil = if dest && !db.cil.is_empty() && !s.id.trim().is_empty() {
            pad_id(self.id_buf, s.id, 3);
            db.cil.iter().find(|r| r.id == *self.id_buf)
        } else {
            None
        };
        pad_id(self.id_buf, s.id, 4);
        let rec = cil.or_else(|| db.zst(self.id_buf).filter(|_| !s.id.trim().is_empty())).or_else(|| {
            let m = index.find(s.name).filter(|m| m.score == 100)?;
            db.zst.get(m.index)
        });
        let prefix = dop_text(db, dop);
        match rec {
            Some(rec) => render_parts(db, &[prefix, &rec.raw], opts, out, self.log),
            None => {
                if s.name.trim().is_empty() {
                    return false;
                }
                // fallback: název ze hry fontem E1 s mezerou 1, jako ZST text bez řídicích kódů
                self.scratch.clear();
                self.scratch.extend([cfg.fallback_stop_font, 0xB1]);
                encode_text(db, s.name.trim(), self.scratch);
                render_parts(db, &[prefix, self.scratch], opts, out, self.log);
            }
        }
        true
    }

    fn text(&mut self, dop: u8, text: &str, out: &mut Vec<u8>) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let (db, cfg) = (self.db, self.cfg);
        self.scratch.clear();
        encode_text(db, text.trim(), self.scratch);
        render_parts(db, &[dop_text(db, dop), self.scratch], &cfg.render, out, self.log);
        true
    }

    /// Vykreslí obsah stránky do `out`; `false` = stránka je prázdná a přeskočí se.
    ///
    /// Jedno pole přes celý panel: text kratší než panel se zarovná (a doplní na šířku
    /// panelu), delší zůstane celý a běží. Víc polí vedle sebe: každé do svého okna,
    /// co se nevejde, je oříznuté (data s tím počítají: linka má nejvýš 22 sloupců, název
    /// s šipkou 109 ze 112).
    fn build(&mut self, spec: PageSpec, inp: &Inputs, out: &mut Vec<u8>) -> bool {
        out.clear();
        let w = self.cfg.width;
        let align = self.cfg.align;
        let center = |f: &Field| match align {
            Align::Auto => f.center,
            Align::Left => false,
            Align::Center => true,
        };
        let fields = spec.fields();
        if let [f] = fields {
            if f.width == 0 || f.width as usize >= w {
                if !self.field(*f, inp, out) {
                    return false;
                }
                if out.len() <= w {
                    let pad = if center(f) { (w - out.len()) / 2 } else { 0 };
                    out.resize(w - pad, 0);
                    out.splice(0..0, std::iter::repeat(0).take(pad));
                }
                return true;
            }
        }
        let mut part = std::mem::take(self.part);
        out.resize(w, 0);
        let (mut x, mut any) = (0, false);
        for f in fields {
            if x >= w {
                break;
            }
            let fw = if f.width == 0 { w - x } else { (f.width as usize).min(w - x) };
            part.clear();
            if self.field(*f, inp, &mut part) {
                any = true;
                let len = part.len().min(fw);
                let pad = if center(f) { (fw - len) / 2 } else { 0 };
                out[x + pad..x + pad + len].copy_from_slice(&part[..len]);
            }
            x += fw;
        }
        *self.part = part;
        any
    }

    /// Vykreslí jedno pole (šablona + hodnota proměnné) na konec `out`; `false` = prázdné.
    fn field(&mut self, spec: Field, inp: &Inputs, out: &mut Vec<u8>) -> bool {
        let (db, cfg) = (self.db, self.cfg);
        let (opts_dest, opts_on, opts_off) = (self.opts_dest, self.opts_on, self.opts_off);
        match spec.var {
            Var::Line => self.line(spec.dop, inp, out),
            Var::NextStop => {
                let opts = match inp.request_stop {
                    None => &cfg.render,
                    Some(true) => opts_on,
                    Some(false) => opts_off,
                };
                self.stop(spec.dop, inp.next_stop, false, opts, out)
            }
            Var::Dest => self.stop(spec.dop, inp.dest, true, opts_dest, out),
            Var::LineDest => {
                let line = self.line(default_dop(Var::Line), inp, out);
                if line && !inp.dest.is_empty() {
                    out.extend(std::iter::repeat(0).take(cfg.linedest_gap));
                }
                self.stop(default_dop(Var::Dest), inp.dest, true, opts_dest, out) || line
            }
            Var::Time => match inp.time_s {
                Some(t) => {
                    let (h, m) = (t / 3600 % 24, t / 60 % 60);
                    let digits = [b'0' + (h / 10) as u8, b'0' + (h % 10) as u8, b':', b'0' + (m / 10) as u8, b'0' + (m % 10) as u8];
                    // (databáze bez šablon DOP - verze 1.xx - má „Čas:" ve firmwaru panelu)
                    self.scratch.clear();
                    if db.dop.is_empty() {
                        self.scratch.extend([cfg.fallback_stop_font, 0xB1]);
                        encode_text(db, "Čas: ", self.scratch);
                    }
                    render_parts(db, &[dop_text(db, spec.dop), self.scratch, &digits], &cfg.render, out, self.log);
                    true
                }
                None => false,
            },
            Var::Zone => self.text(spec.dop, inp.zone, out),
            Var::Info => self.text(spec.dop, inp.info, out),
            Var::Unknown(code) => {
                self.log.once(0x100 | code as u32, || format!("cyklus: neznámá proměnná stránky {code:02X}, stránka přeskočena"));
                false
            }
        }
    }
}

pub struct Panel {
    cfg: Config,
    db: Db,
    index: NameIndex,
    opts_dest: RenderOpts,
    opts_on: RenderOpts,
    opts_off: RenderOpts,
    rows: Vec<Row>,
    seen: Seen,
    frame: Frame,
    frame_no: u32,
    emitted: bool,
    log: Log,
    scratch: Vec<u8>,
    part: Vec<u8>,
    tmp: Vec<u8>,
    id_buf: String,
}

/// Stránky cyklů `ids`: pole se skládají vedle sebe, dokud se vejdou do šířky panelu; pole
/// přes celý panel je stránka samo, položka „konec stránky" stránku uzavře. Doba stránky je
/// nejdelší z dob jejích polí.
fn cycle_pages(db: &Db, cfg: &Config, ids: &[u8], log: &mut Log) -> Vec<PageSpec> {
    let width = cfg.width;
    let mut out = Vec::new();
    for &id in ids {
        let Some(c) = db.cycle(id) else {
            log.once(0x200 | id as u32, || format!("cyklus {id} v databázi není"));
            continue;
        };
        let (mut page, mut used) = (PageSpec::empty(), 0usize);
        for p in &c.pages {
            if p.brk {
                if page.n > 0 {
                    out.push(page);
                }
                (page, used) = (PageSpec::empty(), 0);
                continue;
            }
            let full = p.width as usize >= width;
            let fw = if full { width } else { p.width as usize };
            if page.n > 0 && (used + fw > width || page.n as usize == crate::config::MAX_FIELDS) {
                out.push(page);
                (page, used) = (PageSpec::empty(), 0);
            }
            let var = cfg.var_map.iter().find(|m| m.0 == p.var).map_or(Var::Unknown(p.var), |m| m.1);
            // na střed svého pole stojí všechno, i zóna a čas (režim 00): tak to kreslí gBUSE1
            page.push(Field { dop: p.dop, var, width: if full { 0 } else { p.width as u16 }, center: true });
            page.time_ms = page.time_ms.max(p.time as u32 * cfg.cyk_time_unit_ms);
            page.slide |= !cfg.cyk_effect_slides || p.mode != 0;
            used += fw;
        }
        if page.n > 0 {
            out.push(page);
        }
    }
    out
}

impl Panel {
    pub fn new(db: Db, mut cfg: Config) -> Panel {
        cfg.bind(&db);
        let mut log = Log::new();
        let (mut idle, mut stop) = (Vec::new(), Vec::new());
        if cfg.use_cyk {
            idle = cycle_pages(&db, &cfg, &cfg.cycle_idle, &mut log);
            stop = cycle_pages(&db, &cfg, &[cfg.cycle_stop], &mut log);
            if idle.is_empty() {
                log.once(0x300, || "CYK: žádné stránky klidového cyklu, použit záložní cyklus".into());
            }
        }
        if idle.is_empty() {
            idle = cfg.fallback_pages.clone();
        }
        if stop.is_empty() {
            stop = cfg.fallback_stop_pages.clone();
        }
        let width = cfg.width;
        let rows = if cfg.rows >= 2 {
            // dvouřádek: nahoře klidový cyklus (linka + cíl, zóna + čas), dole zastávka
            vec![Row::new(width, idle, Vec::new()), Row::new(width, cfg.row2_pages.clone(), stop)]
        } else {
            vec![Row::new(width, idle, stop)]
        };
        let with = |ph: Placeholder| RenderOpts {
            placeholder: ph,
            placeholder_flag: None,
            ..cfg.render.clone()
        };
        Panel {
            index: NameIndex::new(&db),
            opts_dest: with(cfg.placeholder_dest),
            opts_on: with(cfg.render.placeholder),
            opts_off: with(cfg.render.placeholder_else),
            frame: Frame { width, strips: vec![vec![0; width]; rows.len()] },
            rows,
            seen: Seen::default(),
            frame_no: 0,
            emitted: false,
            log,
            scratch: Vec::with_capacity(256),
            part: Vec::with_capacity(256),
            tmp: Vec::with_capacity(512),
            id_buf: String::with_capacity(16),
            cfg,
            db,
        }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn name_index(&self) -> &NameIndex {
        &self.index
    }

    /// Poslední snímek (i když se zrovna nezměnil).
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Počet dosud vydaných (změněných) snímků.
    pub fn frame_no(&self) -> u32 {
        self.frame_no
    }

    pub fn log(&mut self) -> &mut Log {
        &mut self.log
    }

    /// Posune čas o `dt` sekund. Vrací snímek jen tehdy, když se od minula změnil.
    pub fn tick(&mut self, dt: f64, inp: &Inputs) -> Option<&Frame> {
        let changed = self.seen.absorb(inp);
        let dt_ms = if dt.is_finite() { (dt * 1000.0).clamp(0.0, 10_000.0) } else { 0.0 };
        let Panel { cfg, db, index, opts_dest, opts_on, opts_off, rows, log, scratch, part, tmp, id_buf, frame, .. } = self;
        let mut b = Builder { db, cfg, index, opts_dest, opts_on, opts_off, log, scratch, part, id_buf };
        let mut dirty = false;
        for (row, strip) in rows.iter_mut().zip(&mut frame.strips) {
            row_tick(row, &mut b, tmp, dt_ms, changed, inp);
            compose(row, b.cfg);
            if *strip != row.vis {
                strip.copy_from_slice(&row.vis);
                dirty = true;
            }
        }
        if dirty || !self.emitted {
            self.emitted = true;
            self.frame_no = self.frame_no.wrapping_add(1);
            Some(&self.frame)
        } else {
            None
        }
    }
}

/// Načte stránku `spec` jako aktuální (obsah už je v `row.cur`).
fn enter(row: &mut Row, cfg: &Config, mode: Mode, idx: usize, spec: PageSpec, slide: bool) {
    row.old.copy_from_slice(&row.vis);
    row.mode = mode;
    row.idx = idx;
    row.spec = spec;
    row.valid = true;
    row.fresh = true;
    row.slide_left = if slide && spec.slide && cfg.slide == Slide::Push { cfg.slide_steps } else { 0 };
    row.slide_acc = 0.0;
    row.scroll_off = 0;
    row.scroll_acc = 0.0;
    row.scroll_wait = cfg.scroll_start_delay_ms;
    row.passes = 0;
    row.elapsed = 0.0;
}

/// Najde první neprázdnou stránku v `mode` od `from` (v klidovém cyklu dokola) a přejde na ni.
fn goto(row: &mut Row, b: &mut Builder, tmp: &mut Vec<u8>, inp: &Inputs, mode: Mode, from: usize, slide: bool) -> bool {
    let n = row.seq(mode).len();
    let count = if mode == Mode::Idle { n } else { n.saturating_sub(from) };
    for k in 0..count {
        let idx = if mode == Mode::Idle { (from + k) % n } else { from + k };
        let spec = row.seq(mode)[idx];
        if b.build(spec, inp, tmp) {
            let same = row.valid && row.mode == mode && row.idx == idx && row.cur == *tmp;
            if same {
                // jediná neprázdná stránka cyklu: běží dál bez nového nasunutí
                row.elapsed = 0.0;
                row.passes = 0;
            } else {
                std::mem::swap(&mut row.cur, tmp);
                enter(row, b.cfg, mode, idx, spec, slide);
            }
            return true;
        }
    }
    false
}

/// Přechod na další stránku; když žádná neprázdná není, panel zhasne.
fn advance(row: &mut Row, b: &mut Builder, tmp: &mut Vec<u8>, inp: &Inputs) {
    let next = row.idx + 1;
    let found = match row.mode {
        Mode::Stop => goto(row, b, tmp, inp, Mode::Stop, next, true) || goto(row, b, tmp, inp, Mode::Idle, 0, true),
        Mode::Idle => goto(row, b, tmp, inp, Mode::Idle, next, true),
    };
    if !found && row.valid {
        row.cur.clear();
        enter(row, b.cfg, Mode::Idle, 0, BLANK, true);
        row.valid = false;
    }
}

fn row_tick(row: &mut Row, b: &mut Builder, tmp: &mut Vec<u8>, dt_ms: f64, changed: u8, inp: &Inputs) {
    let cfg = b.cfg;
    row.fresh = false;
    let trigger = !row.stop.is_empty()
        && ((cfg.stop_on_change && changed & CH_STOP != 0 && !inp.next_stop.is_empty())
            || (cfg.stop_on_press && changed & CH_PRESS != 0));
    if !row.started {
        row.started = true;
        goto(row, b, tmp, inp, Mode::Idle, 0, false);
    } else if trigger && goto(row, b, tmp, inp, Mode::Stop, 0, true) {
        // stránka „zastávka" (znovu od začátku cyklu zastávky)
    } else if !row.valid {
        if changed != 0 {
            goto(row, b, tmp, inp, Mode::Idle, 0, true);
        }
    } else if changed & spec_mask(&row.spec) != 0 {
        let spec = row.spec;
        if !b.build(spec, inp, tmp) {
            advance(row, b, tmp, inp);
        } else if *tmp != row.cur {
            std::mem::swap(&mut row.cur, tmp);
            // (jen čas: nová minuta se přepíše na místě)
            if changed & spec_mask(&spec) & !CH_TIME != 0 {
                // změna hodnoty uprostřed stránky (i uprostřed posuvu): stránka začne znovu
                let (mode, idx) = (row.mode, row.idx);
                enter(row, cfg, mode, idx, spec, true);
            }
        }
    }

    if row.fresh {
        return;
    }
    if row.slide_left > 0 {
        row.slide_acc += dt_ms;
        while row.slide_left > 0 && row.slide_acc >= cfg.slide_step_ms {
            row.slide_acc -= cfg.slide_step_ms;
            row.slide_left -= 1;
        }
        return;
    }
    if !row.valid {
        return;
    }
    row.elapsed += dt_ms;
    let scrolling = row.cur.len() > cfg.width;
    if scrolling {
        if row.scroll_wait > 0.0 {
            row.scroll_wait -= dt_ms;
        } else {
            let period = row.cur.len() + cfg.scroll_gap;
            row.scroll_acc += dt_ms;
            while row.scroll_acc >= cfg.scroll_step_ms {
                row.scroll_acc -= cfg.scroll_step_ms;
                row.scroll_off += 1;
                if row.scroll_off >= period {
                    row.scroll_off = 0;
                    row.passes += 1;
                }
            }
        }
    }
    let dur = if row.spec.time_ms == 0 { cfg.hold_ms } else { row.spec.time_ms } as f64;
    let pass_done = !scrolling || !cfg.scroll_full_pass || row.passes >= 1;
    let held = row.mode == Mode::Stop && row.spec.has(Var::NextStop) && cfg.stop_on_press && inp.stop_pressed;
    if row.elapsed >= dur && pass_done && !held {
        advance(row, b, tmp, inp);
    }
}

/// Složí viditelný obsah řádku: obsah stránky (zarovnaný už z `build`) nebo okno do
/// běžícího textu, a nasouvání.
fn compose(row: &mut Row, cfg: &Config) {
    let (w, len) = (cfg.width, row.cur.len());
    if len <= w {
        row.vis.fill(0);
        row.vis[..len].copy_from_slice(&row.cur);
    } else {
        let period = len + cfg.scroll_gap;
        for (x, v) in row.vis.iter_mut().enumerate() {
            let i = (row.scroll_off + x) % period;
            *v = if i < len { row.cur[i] } else { 0 };
        }
    }
    if row.slide_left > 0 {
        // hotových kroků `done` z `steps`: starý obsah vyjel o `shift` řádků nahoru
        let done = cfg.slide_steps - row.slide_left;
        let shift = done * 8 / cfg.slide_steps;
        for (v, &o) in row.vis.iter_mut().zip(&row.old) {
            *v = (((o as u32) << shift) as u8) | ((*v as u32) >> (8 - shift)) as u8;
        }
    }
}
