//! Konfigurace panelu. Všechno, co je v `PROMPT.md` označené jako HYPOTÉZA, je tady
//! přepínatelné; textový formát je `klíč = hodnota`, `#` nebo `;` uvozuje komentář.

use crate::text::{MissingGlyph, PictAlias, Placeholder, RenderOpts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    Line,
    NextStop,
    Dest,
    Info,
    Time,
    Zone,
    /// Linka + cíl na jednom řádku (horní řádek BS190).
    LineDest,
    Unknown(u8),
}

/// Nejvíc polí vedle sebe na jedné stránce.
pub const MAX_FIELDS: usize = 4;

/// Pole stránky: okno o dané šířce s jednou proměnnou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// DOP šablona (0 = žádná).
    pub dop: u8,
    pub var: Var,
    /// Šířka pole ve sloupcích; 0 = zbytek panelu.
    pub width: u16,
    /// Text na střed pole (při `align = auto`); jinak vlevo.
    pub center: bool,
}

/// Stránka: jedno až `MAX_FIELDS` polí vedle sebe zleva (linka + cíl, zóna + čas), nebo
/// jedno pole přes celý panel (zastávka) - jen to umí běžící text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSpec {
    pub fields: [Field; MAX_FIELDS],
    pub n: u8,
    /// Doba zobrazení v ms; 0 = „do odvolání" (v cyklu se použije `hold_ms`).
    pub time_ms: u32,
    /// Nasouvat při přechodu na tuto stránku.
    pub slide: bool,
}

const NO_FIELD: Field = Field { dop: 0, var: Var::Unknown(0), width: 0, center: false };

impl PageSpec {
    /// Stránka s jedním polem přes celý panel.
    pub const fn single(dop: u8, var: Var, time_ms: u32, slide: bool, center: bool) -> PageSpec {
        let mut fields = [NO_FIELD; MAX_FIELDS];
        fields[0] = Field { dop, var, width: 0, center };
        PageSpec { fields, n: 1, time_ms, slide }
    }

    pub const fn empty() -> PageSpec {
        PageSpec { fields: [NO_FIELD; MAX_FIELDS], n: 0, time_ms: 0, slide: false }
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields[..self.n as usize]
    }

    pub fn push(&mut self, f: Field) -> bool {
        if (self.n as usize) < MAX_FIELDS {
            self.fields[self.n as usize] = f;
            self.n += 1;
            true
        } else {
            false
        }
    }

    pub fn has(&self, var: Var) -> bool {
        self.fields().iter().any(|f| f.var == var)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Každé pole na střed svého okna - tak to kreslí gBUSE1 (linka, cíl, zastávka i zóna
    /// a čas).
    Auto,
    Left,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slide {
    None,
    /// Nová stránka vyjíždí zespodu a starou vytlačuje nahoru.
    Push,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Šířka panelu ve sloupcích; 0 = `auto`: z hlavičky databáze (`Db::panel_width`, Brno
    /// 135), viz `bind`.
    pub width: usize,
    /// 1 = jednořádek BS 120 (8 řádků), 2 = dvouřádek (16 řádků, BS 120.0K): nahoře klidový
    /// cyklus (linka + cíl, zóna + čas), dole zastávka.
    pub rows: usize,
    pub align: Align,
    pub scroll_step_ms: f64,
    pub scroll_gap: usize,
    pub scroll_start_delay_ms: f64,
    /// Stránka s běžícím textem skončí nejdřív po jednom celém průběhu.
    pub scroll_full_pass: bool,
    pub slide: Slide,
    pub slide_steps: u32,
    pub slide_step_ms: f64,
    pub render: RenderOpts,
    /// `&` na stránce „cíl" a v řádku linka + cíl.
    pub placeholder_dest: Placeholder,
    /// HYPOTÉZA 2.6: stránky se berou z CYK; `false` = záložní cyklus `fallback_pages`.
    pub use_cyk: bool,
    pub cycle_idle: Vec<u8>,
    pub cycle_stop: u8,
    pub cyk_time_unit_ms: u32,
    /// HYPOTÉZA 2.6: pole „proměnná" stránky cyklu.
    pub var_map: Vec<(u8, Var)>,
    /// HYPOTÉZA: efekt 0 = bez nasouvání, jinak nasouvání.
    pub cyk_effect_slides: bool,
    pub hold_ms: u32,
    pub fallback_pages: Vec<PageSpec>,
    pub fallback_stop_pages: Vec<PageSpec>,
    /// Spodní řádek dvouřádku (klíč `row2_pages`, dříve `bs190_bottom`).
    pub row2_pages: Vec<PageSpec>,
    pub linedest_gap: usize,
    /// Stránka „zastávka" se ukáže při změně příští zastávky.
    pub stop_on_change: bool,
    /// Stránka „zastávka" se ukáže po stisku STOP a drží, dokud STOP svítí.
    pub stop_on_press: bool,
    pub fallback_line_font: u8,
    pub fallback_stop_font: u8,
    /// Vnější panel (gBUSE0): doba kroku animovaných textů v ms. HYPOTÉZA.
    pub outer_step_ms: f64,
    /// Vnější panel: název nácestné zastávky ze hry obalit pomlčkami jako záznamy DRU.
    pub outer_stop_dashes: bool,
    /// Vnější panel: bez čísla linky dostanou cíl a zastávka i sloupce pole linky.
    pub outer_expand_dest: bool,
}

fn page(var: Var, secs: u32) -> PageSpec {
    PageSpec::single(default_dop(var), var, secs * 1000, true, true)
}

/// DOP šablona, kterou pro danou proměnnou používají cykly v brněnské databázi.
pub fn default_dop(var: Var) -> u8 {
    match var {
        Var::Line => 5,
        Var::NextStop => 1,
        Var::Dest => 2,
        Var::Time => 3,
        Var::Zone => 4,
        Var::Info => 7,
        Var::LineDest | Var::Unknown(_) => 0,
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut render = RenderOpts::default();
        // HYPOTÉZA 2.4: E5-E9 jsou piktogramové fonty ve firmwaru; glyf F8 v nich = piktogram.
        render.pict_alias = vec![
            PictAlias { font: 0xE5, glyph: 0xF8, to_font: 0xE1, to_glyph: 0xF8 },
            PictAlias { font: 0xE7, glyph: 0xF8, to_font: 0xE1, to_glyph: 0xF9 },
            PictAlias { font: 0xE8, glyph: 0xF8, to_font: 0xE1, to_glyph: 0xF6 },
            PictAlias { font: 0xE9, glyph: 0xF8, to_font: 0xE1, to_glyph: 0xF7 },
        ];
        Config {
            width: 0,
            rows: 1,
            align: Align::Auto,
            scroll_step_ms: 50.0,
            scroll_gap: 16,
            scroll_start_delay_ms: 0.0,
            scroll_full_pass: true,
            slide: Slide::Push,
            slide_steps: 8,
            slide_step_ms: 40.0,
            render,
            placeholder_dest: Placeholder::Hide,
            use_cyk: true,
            cycle_idle: vec![0, 2],
            cycle_stop: 6,
            cyk_time_unit_ms: 1000,
            var_map: vec![
                (0x01, Var::Line),
                (0x08, Var::NextStop),
                (0x09, Var::Dest),
                (0x0A, Var::Info),
                (0x0D, Var::Time),
                (0x0E, Var::Zone),
            ],
            cyk_effect_slides: true,
            hold_ms: 4000,
            // (bez cyklů v databázi - verze 1.xx - má panel pevné stránky linka+cíl, pásmo+čas)
            fallback_pages: parse_pages("line+dest:4, zone+time:4").unwrap_or_default(),
            fallback_stop_pages: vec![page(Var::NextStop, 6)],
            row2_pages: vec![page(Var::NextStop, 4)],
            linedest_gap: 4,
            stop_on_change: true,
            stop_on_press: true,
            fallback_line_font: 0xE3,
            fallback_stop_font: 0xE1,
            outer_step_ms: 2000.0,
            outer_stop_dashes: true,
            outer_expand_dest: false,
        }
    }
}

fn parse_bool(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "ano" => Ok(true),
        "0" | "false" | "no" | "off" | "ne" => Ok(false),
        _ => Err(format!("čekám true/false, ne '{v}'")),
    }
}

fn parse_num<T: std::str::FromStr>(v: &str) -> Result<T, String> {
    v.parse().map_err(|_| format!("čekám číslo, ne '{v}'"))
}

fn parse_hex(v: &str) -> Result<u8, String> {
    u8::from_str_radix(v.trim().trim_start_matches("0x"), 16).map_err(|_| format!("čekám hex bajt, ne '{v}'"))
}

fn parse_var(v: &str) -> Result<Var, String> {
    Ok(match v.trim().to_ascii_lowercase().as_str() {
        "line" | "linka" => Var::Line,
        "stop" | "zastavka" | "nextstop" => Var::NextStop,
        "dest" | "cil" => Var::Dest,
        "info" => Var::Info,
        "time" | "cas" => Var::Time,
        "zone" | "zona" => Var::Zone,
        "linedest" => Var::LineDest,
        other => return Err(format!("neznámá proměnná stránky '{other}'")),
    })
}

/// `hide | glyph | blank:N | symbol:E1:F1 | dop:N`
pub fn parse_placeholder(v: &str) -> Result<Placeholder, String> {
    let v = v.trim().to_ascii_lowercase();
    let mut it = v.split(':');
    Ok(match it.next().unwrap_or("") {
        "hide" => Placeholder::Hide,
        "glyph" => Placeholder::AsGlyph,
        "blank" => Placeholder::Blank(parse_num(it.next().unwrap_or("5"))?),
        "symbol" => Placeholder::Symbol {
            font: parse_hex(it.next().unwrap_or("E1"))?,
            glyph: parse_hex(it.next().unwrap_or("F1"))?,
        },
        "dop" => Placeholder::Dop(parse_num(it.next().unwrap_or("1"))?),
        other => return Err(format!("neznámý placeholder '{other}'")),
    })
}

/// `line+dest:4, time:4, stop:6` (sekundy; `:0` = do odvolání), volitelně `@N` = DOP šablona.
/// `a+b` jsou pole vedle sebe na jedné stránce: linka má 22 sloupců (jako v cyklech brněnské
/// databáze), poslední pole zbytek panelu, ostatní 67.
fn parse_pages(v: &str) -> Result<Vec<PageSpec>, String> {
    let mut out = Vec::new();
    for item in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if item.contains('+') {
            let (names, secs) = match item.split_once(':') {
                Some((a, s)) => (a, parse_num::<f64>(s.trim())?),
                None => (item, 4.0),
            };
            let vars: Vec<Var> = names.split('+').map(parse_var).collect::<Result<_, _>>()?;
            let mut page = PageSpec::empty();
            page.time_ms = (secs * 1000.0) as u32;
            page.slide = true;
            for (i, &var) in vars.iter().enumerate() {
                let width = if i + 1 == vars.len() { 0 } else if var == Var::Line { 22 } else { 67 };
                if !page.push(Field { dop: default_dop(var), var, width, center: true }) {
                    return Err(format!("stránka '{item}': nejvýš {MAX_FIELDS} pole"));
                }
            }
            out.push(page);
            continue;
        }
        let (item, dop) = match item.split_once('@') {
            Some((a, d)) => (a, Some(parse_num::<u8>(d.trim())?)),
            None => (item, None),
        };
        let (name, secs) = match item.split_once(':') {
            Some((a, s)) => (a, parse_num::<f64>(s.trim())?),
            None => (item, 4.0),
        };
        let var = parse_var(name)?;
        out.push(PageSpec::single(dop.unwrap_or(default_dop(var)), var, (secs * 1000.0) as u32, true, true));
    }
    Ok(out)
}

/// `E8:F8=E1:F6, E9:F8=E1:F7` nebo `none`.
fn parse_aliases(v: &str) -> Result<Vec<PictAlias>, String> {
    if matches!(v.trim().to_ascii_lowercase().as_str(), "none" | "off" | "") {
        return Ok(Vec::new());
    }
    v.split(',')
        .map(|item| {
            let err = || format!("alias '{item}': čekám FONT:GLYF=FONT:GLYF");
            let (a, b) = item.split_once('=').ok_or_else(err)?;
            let (af, ag) = a.split_once(':').ok_or_else(err)?;
            let (bf, bg) = b.split_once(':').ok_or_else(err)?;
            Ok(PictAlias {
                font: parse_hex(af)?,
                glyph: parse_hex(ag)?,
                to_font: parse_hex(bf)?,
                to_glyph: parse_hex(bg)?,
            })
        })
        .collect()
}

/// Rozloží text konfigurace na dvojice (klíč malými písmeny, hodnota).
pub fn parse_key_values(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start_matches('\u{FEFF}');
            let line = line.split(|c| c == '#' || c == ';').next().unwrap_or("").trim();
            let (k, v) = line.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect()
}

impl Config {
    /// Nastaví jednu hodnotu. `Ok(false)` = klíč engine nezná (patří pluginu).
    pub fn set(&mut self, key: &str, v: &str) -> Result<bool, String> {
        match key {
            "width" => {
                self.width = match v.to_ascii_lowercase().as_str() {
                    "auto" | "0" => 0,
                    _ => parse_num::<usize>(v)?.clamp(8, 1024),
                }
            }
            "rows" => self.rows = parse_num::<usize>(v)?.clamp(1, 2),
            "align" => {
                self.align = match v.to_ascii_lowercase().as_str() {
                    "auto" => Align::Auto,
                    "left" => Align::Left,
                    "center" => Align::Center,
                    _ => return Err(format!("align: auto | left | center, ne '{v}'")),
                }
            }
            "scroll_step_ms" => self.scroll_step_ms = parse_num::<f64>(v)?.max(1.0),
            "scroll_gap" => self.scroll_gap = parse_num(v)?,
            "scroll_start_delay_ms" => self.scroll_start_delay_ms = parse_num(v)?,
            "scroll_full_pass" => self.scroll_full_pass = parse_bool(v)?,
            "slide" => {
                self.slide = match v.to_ascii_lowercase().as_str() {
                    "none" | "off" => Slide::None,
                    "push" => Slide::Push,
                    _ => return Err(format!("slide: push | none, ne '{v}'")),
                }
            }
            "slide_steps" => self.slide_steps = parse_num::<u32>(v)?.clamp(1, 8),
            "slide_step_ms" => self.slide_step_ms = parse_num::<f64>(v)?.max(1.0),
            "default_font" => self.render.default_font = parse_hex(v)?,
            "placeholder" => self.render.placeholder = parse_placeholder(v)?,
            "placeholder_else" => self.render.placeholder_else = parse_placeholder(v)?,
            "placeholder_dest" => self.placeholder_dest = parse_placeholder(v)?,
            "placeholder_flag" => {
                self.render.placeholder_flag = match v.to_ascii_lowercase().as_str() {
                    "none" | "off" => None,
                    _ => Some(parse_hex(v)?),
                }
            }
            "missing_glyph" => {
                self.render.missing_glyph = match v.to_ascii_lowercase().as_str() {
                    "skip" => MissingGlyph::Skip,
                    "box" => MissingGlyph::Box,
                    "fallback" => MissingGlyph::Fallback,
                    _ => return Err(format!("missing_glyph: fallback | box | skip, ne '{v}'")),
                }
            }
            "pict_alias" => self.render.pict_alias = parse_aliases(v)?,
            "use_cyk" => self.use_cyk = parse_bool(v)?,
            "cycle_idle" => {
                self.cycle_idle =
                    v.split(',').map(|s| parse_num(s.trim())).collect::<Result<_, _>>()?
            }
            "cycle_stop" => self.cycle_stop = parse_num(v)?,
            "cyk_time_unit_ms" => self.cyk_time_unit_ms = parse_num(v)?,
            "cyk_effect_slides" => self.cyk_effect_slides = parse_bool(v)?,
            "var_map" => {
                self.var_map = v
                    .split(',')
                    .map(|item| {
                        let (code, var) =
                            item.split_once('=').ok_or(format!("var_map '{item}': čekám KÓD=proměnná"))?;
                        Ok((parse_hex(code)?, parse_var(var)?))
                    })
                    .collect::<Result<_, String>>()?
            }
            "hold_ms" => self.hold_ms = parse_num(v)?,
            "fallback_pages" => self.fallback_pages = parse_pages(v)?,
            "fallback_stop_pages" => self.fallback_stop_pages = parse_pages(v)?,
            "row2_pages" | "bs190_bottom" => self.row2_pages = parse_pages(v)?,
            "linedest_gap" => self.linedest_gap = parse_num(v)?,
            "stop_on_change" => self.stop_on_change = parse_bool(v)?,
            "stop_on_press" => self.stop_on_press = parse_bool(v)?,
            "fallback_line_font" => self.fallback_line_font = parse_hex(v)?,
            "fallback_stop_font" => self.fallback_stop_font = parse_hex(v)?,
            "outer_step_ms" => self.outer_step_ms = parse_num::<f64>(v)?.max(20.0),
            "outer_stop_dashes" => self.outer_stop_dashes = parse_bool(v)?,
            "outer_expand_dest" => self.outer_expand_dest = parse_bool(v)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Doplní šířku `auto` z hlavičky databáze (bez ní 135 = BS 120.0A). Volá ho
    /// `Panel::new`; kdo potřebuje šířku dřív (plugin, converter), zavolá ho sám.
    pub fn bind(&mut self, db: &crate::db::Db) {
        if self.width == 0 {
            self.width = db.panel_width().unwrap_or(135).clamp(8, 1024);
        }
    }

    /// Načte konfiguraci z textu; vrací i hlášky (chybné hodnoty) a klíče, které engine nezná.
    pub fn parse(text: &str) -> (Config, Vec<String>, Vec<(String, String)>) {
        let (mut cfg, mut warn, mut rest) = (Config::default(), Vec::new(), Vec::new());
        for (k, v) in parse_key_values(text) {
            match cfg.set(&k, &v) {
                Ok(true) => {}
                Ok(false) => rest.push((k, v)),
                Err(e) => warn.push(format!("{k}: {e}")),
            }
        }
        (cfg, warn, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let (cfg, warn, rest) = Config::parse(
            "# komentář\nwidth = 96\nrows=2\nuse_cyk = false ; záložní cyklus\n\
             fallback_pages = line:2, dest:3@2, time\nplaceholder = symbol:E1:F1\n\
             pict_alias = none\nvar_line = IBIS_Linie\nslide = bogus\n",
        );
        assert_eq!((cfg.width, cfg.rows, cfg.use_cyk), (96, 2, false));
        assert_eq!(cfg.fallback_pages.len(), 3);
        assert_eq!(cfg.fallback_pages[0].time_ms, 2000);
        assert_eq!(cfg.fallback_pages[1].fields()[0].dop, 2);
        assert!(cfg.render.pict_alias.is_empty());
        assert_eq!(rest, [("var_line".to_string(), "IBIS_Linie".to_string())]);
        assert_eq!(warn.len(), 1);
    }
}
