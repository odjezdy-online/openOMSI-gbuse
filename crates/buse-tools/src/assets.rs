//! Generované soubory pro hru: nibble font (.oft + .bmp), LED textury, model (.o3d),
//! skript vozu (.osc + seznamy proměnných), blok pro model.cfg a `buse_db.lua`.
//!
//! Formáty podle `docs/FORMATS.md` z openOMSI (popis originálu) a podle souborů, které
//! už v OMSI 2 fungují (KRUE_7x5.oft, herman_celni_LED v `SOR_NB12\Model\NB.cfg`).

use buse_engine::Db;
use std::fmt::Write as _;

/// Jméno fontu v `[newfont]`; skript ho hledá přes `GetFontIndex`.
pub const NIB_FONT: &str = "BUSE_nib";
pub const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// 24bit BMP (bottom-up, řádky zarovnané na 4 B) z RGB dat shora dolů.
pub fn bmp24(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let stride = (w * 3).div_ceil(4) * 4;
    let size = 54 + stride * h;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(stride * h).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    for y in (0..h).rev() {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            out.extend_from_slice(&[rgb[i + 2], rgb[i + 1], rgb[i]]);
        }
        out.extend(std::iter::repeat(0).take((stride - w * 3) as usize));
    }
    out
}

/// Rozložení nibble fontu v bitmapě: glyf číslice `k` je sloupec `1 + 2k`, řádky 1..=4.
pub const NIB_BMP_W: u32 = 34;
pub const NIB_BMP_H: u32 = 6;

pub fn nib_x(k: u32) -> u32 {
    1 + 2 * k
}

/// Bitmapa nibble fontu (bílá = svítí; slouží jako barva i jako alfa maska).
/// Nejvyšší bit číslice = horní řádek čtveřice.
pub fn nib_font_rgb() -> Vec<u8> {
    let mut rgb = vec![0u8; (NIB_BMP_W * NIB_BMP_H * 3) as usize];
    for k in 0..16u32 {
        for row in 0..4u32 {
            if k >> (3 - row) & 1 != 0 {
                let i = (((1 + row) * NIB_BMP_W + nib_x(k)) * 3) as usize;
                rgb[i..i + 3].fill(255);
            }
        }
    }
    rgb
}

/// `.oft`: `[newfont] name bitmap alpha height gap` + `[char] ch x0 x1 y` (x1 se nekreslí).
pub fn nib_font_oft(bmp_name: &str) -> String {
    let mut s = String::from(
        "OMSI Font File\r\n\r\n  BUSE LED panel: 16 glyfu 1x4 px, jeden hex znak = 4 radky jednoho sloupce.\r\n  Vygenerovano gbuse-convert.\r\n\r\n",
    );
    let _ = write!(s, "[newfont]\r\n{NIB_FONT}\r\n{bmp_name}\r\n{bmp_name}\r\n4\r\n0\r\n\r\n");
    for k in 0..16u32 {
        let _ = write!(s, "[char]\r\n{}\r\n{}\r\n{}\r\n1\r\n\r\n", HEX_DIGITS[k as usize] as char, nib_x(k), nib_x(k) + 1);
    }
    s
}

/// Barva svítícího bodu vnitřního LED panelu (červená 625 nm).
pub const LED_RED: [u8; 3] = [0xFF, 0x4A, 0x1C];

/// Textura mřížky bodů: `cols` x `rows` bodů po `cell` pixelech. `lit` = svítící vrstva
/// (barva `color` s jasnějším středem na černé), jinak tmavé pozadí se zhasnutými body.
pub fn led_texture(cols: u32, rows: u32, cell: u32, lit: bool, color: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (cols * cell, rows * cell);
    let mut tile = vec![0u8; (cell * cell * 3) as usize];
    let c = (cell as f32 - 1.0) / 2.0;
    let r = cell as f32 * 0.40;
    for y in 0..cell {
        for x in 0..cell {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            // měkký okraj bodu v šířce 1 px
            let k = (r - d + 0.5).clamp(0.0, 1.0);
            let px: [f32; 3] = if lit {
                let core = (1.0 - d / r).clamp(0.0, 1.0) * 0.35;
                color.map(|c| (c as f32 + (255.0 - c as f32) * core * 1.3).min(255.0) * k)
            } else {
                let bg = color.map(|c| 4.0 + c as f32 * 0.065);
                let dot = color.map(|c| 6.0 + c as f32 * 0.15);
                [0, 1, 2].map(|i| bg[i] + (dot[i] - bg[i]) * k)
            };
            let i = ((y * cell + x) * 3) as usize;
            for ch in 0..3 {
                tile[i + ch] = px[ch].round() as u8;
            }
        }
    }
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let (s, d) = ((((y % cell) * cell + x % cell) * 3) as usize, ((y * w + x) * 3) as usize);
            rgb[d..d + 3].copy_from_slice(&tile[s..s + 3]);
        }
    }
    (w, h, rgb)
}

/// Kam panel kouká, tedy odkud se na něj divák dívá.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    /// Vnitřní panel: divák stojí ve voze a dívá se dopředu (i zadní vnější panel: divák
    /// stojí za vozem).
    Inner,
    /// Čelní panel: divák stojí před vozem.
    Front,
    /// Boční panel na pravém boku (u dveří).
    Right,
    /// Boční panel na levém boku.
    Left,
}

impl Facing {
    pub fn parse(v: &str) -> Result<Facing, String> {
        Ok(match v.to_ascii_lowercase().as_str() {
            "inner" | "rear" => Facing::Inner,
            "front" => Facing::Front,
            "right" => Facing::Right,
            "left" => Facing::Left,
            _ => return Err(format!("--facing: inner | front | right | left | rear, ne '{v}'")),
        })
    }

    /// Směr, kterým divákovi roste text (jeho „doprava"), v souřadnicích o3d.
    pub fn right(self) -> [f32; 3] {
        match self {
            Facing::Inner => [1.0, 0.0, 0.0],
            Facing::Front => [-1.0, 0.0, 0.0],
            Facing::Right => [0.0, 0.0, 1.0],
            Facing::Left => [0.0, 0.0, -1.0],
        }
    }

    /// Směr od panelu k divákovi.
    pub fn toward_viewer(self) -> [f32; 3] {
        // (normála = right x up míří od diváka)
        let r = self.right();
        [r[2], 0.0, -r[0]]
    }
}

/// Obdélník v souřadnicích o3d (x doprava, y nahoru, z dopředu): střed, natočení a rozměr.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub c: [f32; 3],
    pub facing: Facing,
    pub w: f32,
    pub h: f32,
}

impl Quad {
    /// Tentýž obdélník posunutý o `d` metrů k divákovi.
    pub fn toward_viewer(self, d: f32) -> Quad {
        let t = self.facing.toward_viewer();
        Quad { c: [self.c[0] + t[0] * d, self.c[1] + t[1] * d, self.c[2] + t[2] * d], ..self }
    }
}

/// `.o3d` verze 1 (hlavička 84 19 01, 16bit počty): jeden obdélník s jedním materiálem.
/// u roste divákovi doprava, v = 0 nahoře.
pub fn o3d_quad(q: Quad, texture: &str) -> Vec<u8> {
    let r = q.facing.right();
    let t = q.facing.toward_viewer();
    let at = |sx: f32, sy: f32| [q.c[0] + r[0] * sx * q.w / 2.0, q.c[1] + sy * q.h / 2.0, q.c[2] + r[2] * sx * q.w / 2.0];
    let verts = [(at(1.0, -1.0), 1.0, 1.0), (at(1.0, 1.0), 1.0, 0.0), (at(-1.0, 1.0), 0.0, 0.0), (at(-1.0, -1.0), 0.0, 1.0)];
    let mut out = vec![0x84, 0x19, 0x01];
    let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
    out.push(0x17);
    out.extend_from_slice(&4u16.to_le_bytes());
    for (p, u, v) in verts {
        // normála míří od diváka, stejně jako u `BUSE_LCD\BUSE_pozadi.o3d` vozu SOR_NB12
        for val in [p[0], p[1], p[2], -t[0], -t[1], -t[2], u, v] {
            f(&mut out, val);
        }
    }
    out.push(0x49);
    out.extend_from_slice(&2u16.to_le_bytes());
    for tri in [[2u16, 1, 0], [3, 2, 0]] {
        for i in tri {
            out.extend_from_slice(&i.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.push(0x26);
    out.extend_from_slice(&1u16.to_le_bytes());
    // difúzní RGBA, spekulární RGB, emisní RGB, lesk
    for val in [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        f(&mut out, val);
    }
    out.push(texture.len() as u8);
    out.extend_from_slice(texture.as_bytes());
    out.push(0x79);
    for (i, _) in [0; 16].iter().enumerate() {
        f(&mut out, if i % 5 == 0 { 1.0 } else { 0.0 });
    }
    out
}

/// Jména souborů a proměnných, která se propisují do skriptu, model.cfg a `.opl`.
#[derive(Debug, Clone)]
pub struct Names {
    /// Prefix proměnných a souborů (výchozí `BSLED`; `BUSE_*` už vůz SOR_NB12 používá jinak).
    pub prefix: String,
    pub width: usize,
    pub strips: usize,
    /// Výška panelu v řádcích (vnitřní 8 nebo 16, vnější 19): tolik má skriptová textura.
    pub height: usize,
    /// Index `[scripttexture]` panelu v model.cfg.
    pub st_index: usize,
    pub power_var: String,
}

impl Names {
    pub fn frame(&self) -> String {
        format!("{}_frame", self.prefix)
    }

    pub fn row(&self, strip: usize, hi: bool) -> String {
        format!("{}_r{strip}_{}", self.prefix, if hi { "hi" } else { "lo" })
    }

    pub fn varlist(&self) -> String {
        format!("{0}_frame\r\n{0}_frame_last\r\n{0}_font\r\n{0}_ready\r\n", self.prefix)
    }

    pub fn stringvarlist(&self) -> String {
        (0..self.strips).flat_map(|s| [self.row(s, true), self.row(s, false)]).map(|n| n + "\r\n").collect()
    }

    /// Skript vozu: při změně `<prefix>_frame` překreslí skriptovou texturu nibble fontem.
    pub fn osc(&self) -> String {
        let (p, i, zeros) = (&self.prefix, self.st_index, "0".repeat(self.width));
        let mut s = String::new();
        let _ = write!(
            s,
            "'############################################################\r\n\
             '  BUSE BS120 / BS190 - vnitrni LED panel (vygenerovano gbuse-convert)\r\n\
             '\r\n\
             '  Logiku panelu pocita plugin buse_panel (DLL nebo Lua). Snimek posila v string\r\n\
             '  promennych {p}_r*_hi / {p}_r*_lo: jeden hex znak na sloupec, hi = horni 4 radky\r\n\
             '  pruhu, lo = spodni 4. Font {NIB_FONT} ma 16 glyfu 1x4 px, takze jeden STTextOut\r\n\
             '  nakresli 4 radky cele sirky panelu. Prekresluje se jen pri zmene {p}_frame.\r\n\
             '\r\n\
             '  Skriptova textura: index {i}, {w} x {h}.\r\n\
             '############################################################\r\n\r\n\
             {{macro:{p}_init}}\r\n\
             '\tPlugin smi do stringu psat nejvys tolik znaku, kolik uz maji: proto presne {w} nul.\r\n",
            w = self.width,
            h = self.height
        );
        for st in 0..self.strips {
            for hi in [true, false] {
                let _ = write!(s, "\t\"{zeros}\" (S.$.{})\r\n", self.row(st, hi));
            }
        }
        let _ = write!(
            s,
            "\t0 (S.L.{p}_frame)\r\n\t-1 (S.L.{p}_frame_last)\r\n\t-1 (S.L.{p}_font)\r\n\t0 (S.L.{p}_ready)\r\n{{end}}\r\n\r\n\
             {{macro:{p}_frame}}\r\n\
             \t(L.L.{p}_ready) 0 =\r\n\
             \t{{if}}\r\n\
             \t\t{i} (M.V.STNewTex)\r\n\
             \t\t\"{NIB_FONT}\" (M.V.GetFontIndex) (S.L.{p}_font)\r\n\
             \t\t1 (S.L.{p}_ready)\r\n\
             \t\t-1 (S.L.{p}_frame_last)\r\n\
             \t{{endif}}\r\n\r\n\
             \t(L.L.{p}_frame) (L.L.{p}_frame_last) = !\r\n\
             \t(L.L.{p}_font) 0 >= &&\r\n\
             \t{{if}}\r\n\
             \t\t(L.L.{p}_frame) (S.L.{p}_frame_last)\r\n\
             \t\t{i} (M.V.STLock)\r\n\
             '\t\tbarva: vsechny slozky 255, alfa pixelu jde z masky fontu\r\n\
             \t\t{i} 255 255 255 255 (M.V.STSetColor)\r\n"
        );
        for st in 0..self.strips {
            for (hi, y) in [(true, st * 8), (false, st * 8 + 4)] {
                if y >= self.height {
                    continue;
                }
                // STTextOut(index, x, y, font, mode, letter-spacing, text); mode 0 = cela bunka glyfu
                let _ = write!(s, "\t\t{i} 0 {y} (L.L.{p}_font) 0 0 (L.$.{}) (M.V.STTextOut)\r\n", self.row(st, hi));
            }
        }
        let _ = write!(s, "\t\t{i} (M.V.STUnlock)\r\n\t{{endif}}\r\n{{end}}\r\n");
        s
    }

    /// Blok pro model.cfg: deklarace skriptové textury (patří za poslední existující).
    pub fn cfg_scripttexture(&self) -> String {
        format!(
            "\r\n{}: BUSE LED panel ({} x {})\r\n[scripttexture]\r\n{}\r\n{}\r\n",
            self.st_index,
            self.width,
            self.height,
            self.width,
            self.height
        )
    }

    /// Blok pro model.cfg: pozadí + svítící vrstva (stejný zápis jako herman_celni_LED).
    pub fn cfg_meshes(&self, model_dir: &str) -> String {
        let p = &self.prefix;
        format!(
            "\r\n########################################\r\n\
             BUSE BS120 / BS190 LED panel (gbuse-convert)\r\n\
             ########################################\r\n\r\n\
             [mesh]\r\n{model_dir}\\{p}_bg.o3d\r\n\r\n\
             [matl]\r\n{p}_bg.png\r\n0\r\n\r\n\
             [mesh]\r\n{model_dir}\\{p}_led.o3d\r\n\r\n\
             [illumination_interior]\r\n-1\r\n-1\r\n-1\r\n-1\r\n\r\n\
             [matl]\r\n{p}_led.png\r\n0\r\n\r\n\
             [matl_texadress_clamp]\r\n\r\n\
             [matl_transmap]\r\n\\S:{i}\r\n\r\n\
             [matl_alpha]\r\n2\r\n\r\n\
             [matl_change]\r\n{p}_led.png\r\n0\r\n{power}\r\n\r\n\
             [matl_item]\r\n\r\n\
             [matl_nightmap]\r\n{p}_led.png\r\n\r\n",
            i = self.st_index,
            power = self.power_var
        )
    }
}

fn lua_bytes(raw: &[u8]) -> String {
    let mut s = String::with_capacity(raw.len() * 4 + 2);
    s.push('"');
    for &b in raw {
        match b {
            b'"' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7E => s.push(b as char),
            _ => {
                let _ = write!(s, "\\{b:03}");
            }
        }
    }
    s.push('"');
    s
}

/// `buse_db.lua`: stejná data jako `.hex`, jako Lua tabulka (Lua v openOMSI nemá `io`).
pub fn db_lua(db: &Db) -> String {
    let mut s = String::from("-- Vygenerováno gbuse-convert z databáze gBUSE1. Needitovat ručně.\nreturn {\n");
    let _ = writeln!(s, "  name = {},", lua_bytes(db.name.as_deref().unwrap_or("").as_bytes()));
    let _ = writeln!(s, "  size = {},", db.image.len());
    // šířka panelu z hlavičky databáze (0x86 + 1 = 135); bez ní engine vezme 135
    if let Some(w) = db.panel_width() {
        let _ = writeln!(s, "  width = {w},");
    }
    s.push_str("  -- fonts[font][kód] = sloupce glyfu zleva, bit 7 = horní řádek\n  fonts = {\n");
    for font in db.fonts() {
        let _ = writeln!(s, "    [0x{:02X}] = {{", font.id);
        for code in font.codes() {
            let g = font.glyph(code).unwrap();
            let cols: Vec<String> = g.cols.iter().map(|c| c.to_string()).collect();
            let _ = writeln!(s, "      [0x{code:02X}] = {{{}}},", cols.join(","));
        }
        s.push_str("    },\n");
    }
    s.push_str("  },\n  -- charmap[kód] = Unicode codepoint\n  charmap = {");
    for code in 0x80..=0xDFu8 {
        if let Some(ch) = db.code_to_char(code) {
            let _ = write!(s, "[0x{code:02X}]={},", ch as u32);
        }
    }
    s.push_str("},\n");
    for (name, rows) in [("lin", &db.lin), ("zst", &db.zst), ("cil", &db.cil)] {
        let _ = writeln!(s, "  {name} = {{");
        for r in rows {
            let _ = writeln!(s, "    [{}] = {},", lua_bytes(r.id.as_bytes()), lua_bytes(&r.raw));
        }
        s.push_str("  },\n");
        // pořadí záznamů (při párování názvů vyhrává první shoda)
        let ids: Vec<String> = rows.iter().map(|r| lua_bytes(r.id.as_bytes())).collect();
        let _ = writeln!(s, "  {name}_order = {{{}}},", ids.join(","));
    }
    s.push_str("  dop = {\n");
    for (id, raw) in &db.dop {
        let _ = writeln!(s, "    [{id}] = {},", lua_bytes(raw));
    }
    s.push_str("  },\n  cyk = {\n");
    for c in &db.cyk {
        let _ = write!(s, "    [{}] = {{ pages = {{", c.id);
        for p in &c.pages {
            let _ = write!(
                s,
                "{{width={},dop={},var={},mode={},time={},brk={}}},",
                p.width, p.dop, p.var, p.mode, p.time, p.brk
            );
        }
        let names: Vec<String> = c.names.iter().map(|n| format!("{n:?}")).collect();
        let _ = writeln!(s, "}}, names = {{{}}} }},", names.join(","));
    }
    s.push_str("  },\n}\n");
    s
}

/// Lua řetězec v uvozovkách (UTF-8 zůstává, escapují se jen řídicí znaky).
pub fn lua_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\{:03}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `config.lua`: text buse_panel.cfg jako Lua řetězec (parsuje ho engine.lua).
pub fn config_lua(cfg_text: &str) -> Result<String, String> {
    if cfg_text.contains("]==]") {
        return Err("buse_panel.cfg nesmí obsahovat ]==]".into());
    }
    Ok(format!(
        "-- Vygenerováno gbuse-convert z buse_panel.cfg; stejné klíče, stejný formát.\nreturn [==[\n{}]==]\n",
        cfg_text.trim_start_matches('\u{FEFF}')
    ))
}

/// `zst_map.lua`: normalizovaný název -> ZST id.
pub fn zst_map_lua(map: &std::collections::HashMap<String, String>) -> String {
    let mut rows: Vec<_> = map.iter().collect();
    rows.sort();
    let mut s = String::from("-- Vygenerováno gbuse-convert ze zst_map.csv (normalizovaný název -> ZST id).\nreturn {\n");
    for (name, id) in rows {
        let _ = writeln!(s, "  [{}] = {},", lua_str(name), lua_str(id));
    }
    s.push_str("}\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nibble_font_bitmap_matches_digits() {
        let rgb = nib_font_rgb();
        let lit = |x: u32, y: u32| rgb[((y * NIB_BMP_W + x) * 3) as usize] == 255;
        // 'A' = 1010: řádky 0 a 2 svítí
        assert_eq!([0, 1, 2, 3].map(|r| lit(nib_x(10), 1 + r)), [true, false, true, false]);
        assert_eq!([0, 1, 2, 3].map(|r| lit(nib_x(1), 1 + r)), [false, false, false, true]);
        // mezi glyfy a kolem nich je tma
        assert!((0..NIB_BMP_H).all(|y| !lit(0, y) && !lit(2, y)));
        let bmp = bmp24(NIB_BMP_W, NIB_BMP_H, &rgb);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 104 * 6);
        let oft = nib_font_oft("BUSE_nib.bmp");
        assert_eq!(oft.matches("[char]").count(), 16);
        assert!(oft.contains("[newfont]\r\nBUSE_nib\r\nBUSE_nib.bmp\r\nBUSE_nib.bmp\r\n4\r\n0\r\n"));
        assert!(oft.contains("[char]\r\nF\r\n31\r\n32\r\n1\r\n"));
    }

    #[test]
    fn o3d_has_expected_layout() {
        let o = o3d_quad(Quad { c: [0.0, 2.5, 4.3], facing: Facing::Inner, w: 0.7, h: 0.05 }, "BSLED_led.png");
        // vnitřní panel: u roste s x, LED vrstva blíž k divákovi má menší z
        let x = |i: usize| f32::from_le_bytes(o[6 + i * 32..10 + i * 32].try_into().unwrap());
        assert!(x(0) > x(2));
        assert!(Quad { c: [0.0, 2.5, 4.3], facing: Facing::Inner, w: 0.7, h: 0.05 }.toward_viewer(0.01).c[2] < 4.3);
        assert!(Quad { c: [0.0, 2.5, 5.7], facing: Facing::Front, w: 1.4, h: 0.19 }.toward_viewer(0.01).c[2] > 5.7);
        assert!(Quad { c: [1.16, 2.4, 3.3], facing: Facing::Right, w: 1.1, h: 0.19 }.toward_viewer(0.01).c[0] > 1.16);
        assert_eq!(&o[..6], [0x84, 0x19, 0x01, 0x17, 4, 0]);
        // 3 hlavička + (1+2+4*32) vrcholy + (1+2+2*8) trojúhelníky + (1+2+44+1+13) materiál + (1+64) matice
        assert_eq!(o.len(), 3 + 131 + 19 + 61 + 65);
    }
}
