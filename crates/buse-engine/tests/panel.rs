//! Fáze 2: cykly a efekty (pole vedle sebe, běžící text, nasouvání, STOP, změna vstupu
//! uprostřed posuvu, neznámý font). Čas běží po 10 ms, takže testy jsou deterministické.

use buse_engine::text::render_parts;
use buse_engine::{Config, Db, Frame, Inputs, Log, Panel, RenderOpts, StopRef};

// (the operator's databases are not part of the repository: see docs/BUSE_PANELS.md)
const ROOT: &str = env!("BUSE_DATA");
const DT: f64 = 0.010;
/// Šířka panelu z hlavičky brněnské databáze (BS 120.0A: 8 x 135 bodů).
const W: usize = 135;

fn load_db() -> Db {
    Db::from_hex(&std::fs::read_to_string(format!("{ROOT}/data/ADledA.hex")).unwrap()).unwrap()
}

fn cfg(pairs: &[(&str, &str)]) -> Config {
    let mut c = Config::default();
    for (k, v) in pairs {
        assert_eq!(c.set(k, v), Ok(true), "{k}");
    }
    c
}

fn inputs<'a>(line: &'a str, dest: &'a str, stop: &'a str) -> Inputs<'a> {
    Inputs {
        line,
        dest: StopRef { id: dest, name: "" },
        next_stop: StopRef { id: stop, name: "" },
        time_s: Some(12 * 3600 + 34 * 60),
        ..Inputs::default()
    }
}

/// Posune panel o `secs` a vrátí všechny změněné snímky s časem (v ms od začátku běhu).
fn run(p: &mut Panel, inp: &Inputs, secs: f64) -> Vec<(u32, Frame)> {
    let mut out = Vec::new();
    for i in 0..(secs / DT).round() as u32 {
        if let Some(f) = p.tick(DT, inp) {
            out.push(((i + 1) * 10, f.clone()));
        }
    }
    out
}

fn page(db: &Db, dop: u8, raw: &[u8], opts: &RenderOpts) -> Vec<u8> {
    let mut out = Vec::new();
    render_parts(db, &[db.dop(dop).unwrap_or(&[]), raw], opts, &mut out, &mut Log::new());
    out
}

/// Panel o šířce `w` s texty v oknech `(x, šířka okna, sloupce, na střed)`.
fn layout(w: usize, parts: &[(usize, usize, &[u8], bool)]) -> Vec<u8> {
    let mut out = vec![0; w];
    for &(x, fw, cols, center) in parts {
        let len = cols.len().min(fw);
        let pad = if center { (fw - len) / 2 } else { 0 };
        out[x + pad..x + pad + len].copy_from_slice(&cols[..len]);
    }
    out
}

/// Stránka „linka + cíl" cyklu 0: linka na střed prvních 22 sloupců, cíl na střed dalších 112.
fn line_dest(db: &Db, c: &Config, lin: &str, zst: &str) -> Vec<u8> {
    let line = page(db, 5, &db.lin(lin).unwrap().raw, &c.render);
    let dest = page(db, 2, &db.zst(zst).unwrap().raw, &c.render);
    layout(W, &[(0, 22, &line, true), (22, 112, &dest, true)])
}

/// Stránka „zastávka" cyklu 6: značka zastávky + název na střed celého panelu.
fn stop_page(db: &Db, c: &Config, zst: &str) -> Vec<u8> {
    let stop = page(db, 1, &db.zst(zst).unwrap().raw, &c.render);
    layout(W, &[(0, W, &stop, true)])
}

#[test]
fn width_and_fields_come_from_the_database() {
    let db = load_db();
    assert_eq!(db.panel_width(), Some(W), "hlavička: poslední sloupec 0x86");
    // cyklus 0: linka (22) + cíl (112) vedle sebe, mimořádná informace přes celý panel
    let c0 = db.cycle(0).unwrap();
    let widths: Vec<(u8, bool)> = c0.pages.iter().map(|p| (p.width, p.brk)).collect();
    assert_eq!(widths, [(0x16, false), (0x70, false), (0x87, false), (0, true)]);
    assert_eq!(c0.names, ["linka", "cil", "mim.inf.", ""]);
    // cyklus 6: zastávka, konec stránky, informace, konec stránky
    let c6 = db.cycle(6).unwrap();
    assert_eq!(c6.pages.iter().map(|p| p.brk).collect::<Vec<_>>(), [false, true, false, true]);
    assert_eq!(c6.names, ["zastávka", "", "mim.inf.", ""]);
    // šířky polí sedí s daty: nejširší linka má právě 22 sloupců, cíl se šipkou se vejde do 112
    let r = Config::default().render;
    let widest_line = db.lin.iter().map(|l| page(&db, 5, &l.raw, &r).len()).max().unwrap();
    let widest_dest = db.zst.iter().map(|z| page(&db, 2, &z.raw, &r).len()).max().unwrap();
    let widest_stop = db.zst.iter().map(|z| page(&db, 1, &z.raw, &r).len()).max().unwrap();
    assert_eq!(widest_line, 22);
    assert!(widest_dest <= 112, "{widest_dest}");
    assert!(widest_stop <= W, "{widest_stop}");
    // panel si šířku vezme z databáze sám
    let mut p = Panel::new(db, Config::default());
    assert_eq!(p.config().width, W);
    assert_eq!(p.tick(0.0, &inputs("53", "1146", "")).unwrap().strips[0].len(), W);
}

#[test]
fn line_and_destination_stand_side_by_side() {
    let db = load_db();
    let c = cfg(&[("slide", "none")]);
    let first = line_dest(&db, &c, "053", "1146");
    let time = page(&db, 3, b"12:34", &c.render);
    let zone = {
        let mut raw = Vec::new();
        buse_engine::names::encode_text(&db, "101", &mut raw);
        page(&db, 4, &raw, &c.render)
    };
    let mut p = Panel::new(db, c);
    let mut inp = inputs("53", "1146", "");
    inp.zone = "101";
    assert_eq!(p.tick(0.0, &inp).unwrap().strips[0], first, "linka a cíl jsou vidět současně");
    // po 4 s stránka zóna (67 sloupců) + čas (67), obojí na střed svého pole (jako v gBUSE1)
    let frames = run(&mut p, &inp, 4.1);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, 4000);
    assert_eq!(frames[0].1.strips[0], layout(W, &[(0, 67, &zone, true), (67, 67, &time, true)]), "{}", frames[0].1.ascii());
    // nová minuta se přepíše na místě, stránka nezačíná znovu
    inp.time_s = Some(12 * 3600 + 35 * 60);
    let f = p.tick(DT, &inp).unwrap().clone();
    assert_ne!(f.strips[0][67..], frames[0].1.strips[0][67..]);
    assert_eq!(f.strips[0][..67], frames[0].1.strips[0][..67]);
    let back = run(&mut p, &inp, 4.0);
    assert_eq!(back.len(), 1, "stránka zóna + čas trvá 4 s od svého začátku");
    assert_eq!(back[0].1.strips[0], first);
}

#[test]
fn alignment_can_be_forced() {
    let db = load_db();
    let stop = page(&db, 1, &db.zst("1001").unwrap().raw, &Config::default().render);
    for (align, center) in [("left", false), ("center", true), ("auto", true)] {
        let mut p = Panel::new(db.clone(), cfg(&[("slide", "none"), ("align", align)]));
        let mut inp = inputs("53", "1146", "");
        p.tick(0.0, &inp);
        inp.next_stop.id = "1001";
        let f = p.tick(DT, &inp).unwrap().clone();
        assert_eq!(f.strips[0], layout(W, &[(0, W, &stop, center)]), "align = {align}");
    }
}

#[test]
fn long_stop_scrolls_one_column_per_50ms_with_gap() {
    // 1494 Bystrc Pod Mniší horou (font E0) + prefix DOP 1 má 96 sloupců: na 135 se vejde,
    // na 80 běží
    let db = load_db();
    let c = cfg(&[("slide", "none"), ("width", "80")]);
    let want = page(&db, 1, &db.zst("1494").unwrap().raw, &c.render);
    assert_eq!(want.len(), 96);
    let period = want.len() + c.scroll_gap;
    let mut p = Panel::new(db, c);
    let mut inp = inputs("105", "1146", "");
    p.tick(0.0, &inp);
    inp.next_stop.id = "1494";
    // změna zastávky -> stránka „zastávka"; bez nasouvání je hned vidět začátek textu
    let first = p.tick(0.0, &inp).expect("změna zastávky musí změnit snímek").clone();
    assert_eq!(first.strips[0], want[..80]);
    let frames = run(&mut p, &inp, (period as f64 + 5.0) * 0.050);
    // každých 50 ms posun o 1 sloupec, za koncem textu mezera 16 sloupců a text znovu
    for (k, (t, f)) in frames.iter().enumerate().take(period - 1) {
        assert_eq!(*t, (k as u32 + 1) * 50, "snímek {k}");
        let off = k + 1;
        let expect: Vec<u8> = (0..80)
            .map(|x| {
                let i = (off + x) % period;
                if i < want.len() { want[i] } else { 0 }
            })
            .collect();
        assert_eq!(f.strips[0], expect, "posun {off}");
    }
    // po jednom celém průběhu (a uplynutí 4 s stránky) se panel vrátí do klidového cyklu
    assert_eq!(frames.len(), period);
    assert_eq!(frames[period - 1].0, period as u32 * 50);
    assert_ne!(frames[period - 1].1.strips[0][..5], want[..5]);
}

#[test]
fn page_change_slides_in_from_bottom() {
    let db = load_db();
    let c = cfg(&[]);
    let first = line_dest(&db, &c, "053", "1001");
    let stop = stop_page(&db, &c, "1163");
    let mut p = Panel::new(db, c);
    let mut inp = inputs("53", "1001", "");
    assert_eq!(p.tick(0.0, &inp).unwrap().strips[0], first, "první stránka cyklu 0 je linka + cíl");
    run(&mut p, &inp, 1.0);
    // změna zastávky: stránka „zastávka" (režim 03) vyjede zespodu, 8 kroků po 40 ms;
    // krok k: starý obsah o k řádků výš
    inp.next_stop.id = "1163";
    assert!(p.tick(DT, &inp).is_none(), "nasouvání začne až dalším krokem");
    let frames = run(&mut p, &inp, 0.4);
    assert_eq!(frames.len(), 8, "časy: {:?}", frames.iter().map(|f| f.0).collect::<Vec<_>>());
    for (k, (t, f)) in frames.iter().enumerate() {
        let shift = k as u32 + 1;
        assert_eq!(*t, shift * 40, "krok {shift}");
        let expect: Vec<u8> = first
            .iter()
            .zip(&stop)
            .map(|(&o, &n)| if shift == 8 { n } else { (o << shift) | (n >> (8 - shift)) })
            .collect();
        assert_eq!(f.strips[0], expect, "krok {shift}");
    }
    assert_eq!(frames[7].1.strips[0], stop);
}

#[test]
fn stop_press_shows_stop_page_and_holds_it() {
    let db = load_db();
    let c = cfg(&[("slide", "none")]);
    let stop = stop_page(&db, &c, "1001");
    let first = line_dest(&db, &c, "053", "1146");
    let mut p = Panel::new(db, c);
    let mut inp = inputs("053", "1146", "1001");
    assert_eq!(p.tick(0.0, &inp).unwrap().strips[0], first);
    run(&mut p, &inp, 1.0);
    inp.stop_pressed = true;
    assert_eq!(p.tick(DT, &inp).unwrap().strips[0], stop, "STOP -> stránka zastávka");
    // drží, dokud STOP svítí (déle než 4 s stránky)
    assert!(run(&mut p, &inp, 10.0).is_empty(), "při svítícím STOP se stránka nemění");
    inp.stop_pressed = false;
    let after = run(&mut p, &inp, 0.1);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].1.strips[0], first, "po zhasnutí STOP zpět do klidového cyklu");
}

#[test]
fn stop_change_shows_stop_page_then_returns() {
    let db = load_db();
    let c = cfg(&[("slide", "none")]);
    let stop = stop_page(&db, &c, "1163");
    let mut p = Panel::new(db, c);
    let mut inp = inputs("105", "1146", "1001");
    p.tick(0.0, &inp);
    run(&mut p, &inp, 2.0);
    inp.next_stop.id = "1163";
    assert_eq!(p.tick(DT, &inp).unwrap().strips[0], stop);
    // cyklus 6: zastávka 4 s, pak (prázdná) mim. inf. -> zpět na linku + cíl
    let frames = run(&mut p, &inp, 4.2);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, 4000);
}

#[test]
fn two_rows_show_the_idle_cycle_above_the_stop() {
    // dvouřádek (BS 120.0K): nahoře klidový cyklus, dole zastávka
    let db = load_db();
    let c = cfg(&[("rows", "2"), ("slide", "none")]);
    let top = line_dest(&db, &c, "053", "1146");
    let top2 = line_dest(&db, &c, "001", "1146");
    let bottom = stop_page(&db, &c, "1001");
    let mut p = Panel::new(db, c);
    let mut inp = inputs("53", "1146", "1001");
    let first = p.tick(0.0, &inp).unwrap().clone();
    assert_eq!(first.strips.len(), 2);
    assert_eq!(first.strips[0], top);
    assert_eq!(first.strips[1], bottom);
    // změna linky se nahoře projeví hned, spodní řádek zůstává
    inp.line = "1";
    let f = p.tick(DT, &inp).expect("změna linky se projeví hned").clone();
    assert_eq!(f.strips[0], top2);
    assert_eq!(f.strips[1], bottom);
}

#[test]
fn line_change_does_not_restart_a_scrolling_stop() {
    // změna linky během běžící stránky zastávky posuv nepřeruší
    let db = load_db();
    let mut p = Panel::new(db, cfg(&[("slide", "none"), ("width", "80")]));
    let mut inp = inputs("105", "1146", "");
    p.tick(0.0, &inp);
    inp.next_stop.id = "1494";
    p.tick(DT, &inp);
    let a = run(&mut p, &inp, 0.5);
    inp.line = "1";
    let b = run(&mut p, &inp, 0.5);
    assert_eq!((a.len(), b.len()), (10, 10));
    assert_eq!(b[0].1.strips[0][..79], a[9].1.strips[0][1..]);
}

#[test]
fn unknown_font_e8_at_hlavni_nadrazi() {
    let db = load_db();
    let raw = db.zst("1146").unwrap().raw.clone();
    assert!(raw.windows(2).any(|w| w == [0xE8, 0xF8]), "1146 obsahuje E8 F8");
    // výchozí chování ze zadání: neznámý font -> E1, hláška jen jednou
    let mut log = Log::new();
    let plain = RenderOpts { pict_alias: Vec::new(), ..RenderOpts::default() };
    let a = buse_engine::render_text(&db, &raw, &plain, &mut log);
    let _ = buse_engine::render_text(&db, &raw, &plain, &mut log);
    let msgs = log.take();
    assert_eq!(msgs.iter().filter(|m| m.contains("E8")).count(), 1, "{msgs:?}");
    let f8: Vec<u8> = db.font(0xE1).unwrap().glyph(0xF8).unwrap().cols.iter().map(|&c| c as u8).collect();
    assert!(a.ends_with(&f8), "bez aliasu se kreslí E1:F8");
    // hypotéza: E8 F8 = piktogram vlaku (E1:F6)
    let b = buse_engine::render_text(&db, &raw, &Config::default().render, &mut log);
    let f6: Vec<u8> = db.font(0xE1).unwrap().glyph(0xF6).unwrap().cols.iter().map(|&c| c as u8).collect();
    assert!(b.ends_with(&f6));
    // a celý panel s touhle zastávkou nespadne
    let mut p = Panel::new(db, Config::default());
    let inp = inputs("1", "1146", "1146");
    assert!(run(&mut p, &inp, 30.0).len() > 10);
}

#[test]
fn missing_glyph_is_a_box_and_never_panics() {
    let db = load_db();
    let mut log = Log::new();
    let opts = RenderOpts { missing_glyph: buse_engine::MissingGlyph::Box, ..RenderOpts::default() };
    // 0x23 (#) žádný font nemá
    assert!(db.fonts().all(|f| f.glyph(0x23).is_none()));
    let cols = buse_engine::render_text(&db, &[0xE1, 0xB1, 0x41, 0x23, 0x0D], &opts, &mut log);
    assert!(cols.ends_with(&[0, 0x7F, 0x41, 0x41, 0x41, 0x7F]));
    // náhodné bajty včetně řídicích kódů nesmí shodit engine
    let mut seed = 12345u32;
    for _ in 0..2000 {
        let raw: Vec<u8> = (0..40)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        let _ = buse_engine::render_text(&db, &raw, &opts, &mut log);
        let _ = buse_engine::render_text(&db, &raw, &RenderOpts::reference(), &mut log);
    }
}

#[test]
fn ampersand_is_hidden_unless_a_symbol_is_asked_for() {
    let db = load_db();
    // výchozí: `&` se nekreslí (žádný font databáze ho nemá) a `C0` nic neznamená
    let c = cfg(&[("slide", "none")]);
    for id in ["1001", "1163"] {
        let no_amp: Vec<u8> = db.zst(id).unwrap().raw.iter().copied().filter(|&b| b != 0x26).collect();
        assert_eq!(page(&db, 0, &db.zst(id).unwrap().raw, &c.render), buse_engine::render_text(&db, &no_amp, &c.render, &mut Log::new()));
    }
    // na přání: symbol za `&` jen u záznamů s příznakem C0 (1001 ho má, 1163 ne)
    let c = cfg(&[("slide", "none"), ("placeholder", "symbol:E1:F1"), ("placeholder_flag", "C0")]);
    let sign: Vec<u8> = db.font(0xE1).unwrap().glyph(0xF1).unwrap().cols.iter().map(|&c| c as u8).collect();
    let with = page(&db, 0, &db.zst("1001").unwrap().raw, &c.render);
    let without = page(&db, 0, &db.zst("1163").unwrap().raw, &c.render);
    assert_eq!(with[..5], sign[..]);
    let no_amp: Vec<u8> = db.zst("1163").unwrap().raw.iter().copied().filter(|&b| b != 0x26).collect();
    assert_eq!(without, buse_engine::render_text(&db, &no_amp, &c.render, &mut Log::new()));
    // vstup „zastávka na znamení" má přednost před příznakem
    let all = RenderOpts { placeholder_flag: None, ..c.render.clone() };
    let want = page(&db, 1, &db.zst("1163").unwrap().raw, &all);
    let mut p = Panel::new(db, c);
    let mut inp = inputs("105", "1146", "");
    p.tick(0.0, &inp);
    inp.next_stop.id = "1163";
    inp.request_stop = Some(true);
    let f = p.tick(DT, &inp).unwrap().clone();
    assert_eq!(f.strips[0], layout(W, &[(0, W, &want, true)]), "{}", f.ascii());
    // DOP 1 = značka + mezera, pak symbol za `&`
    assert_eq!(want[..5], sign[..]);
    assert_eq!(want[8..13], sign[..]);
}

#[test]
fn fallback_name_and_fallback_cycle() {
    let db = load_db();
    let mut p = Panel::new(db, cfg(&[("use_cyk", "false"), ("slide", "none"), ("fallback_pages", "line:1, dest:1, time:1")]));
    let mut inp = inputs("N99", "", "");
    inp.dest.name = "Nová Zastávka";
    let frames = [p.tick(0.0, &inp).unwrap().clone()].into_iter().chain(run(&mut p, &inp, 3.0).into_iter().map(|f| f.1)).collect::<Vec<_>>();
    assert_eq!(frames.len(), 4, "linka, cíl, čas, linka");
    assert!(frames.iter().take(3).all(|f| f.strips[0].iter().any(|&c| c != 0)));
    assert_eq!(frames[0], frames[3]);
    // název, který v ZST je, se najde podle jména (bez id) a vykreslí stejně jako přes id
    let db = load_db();
    let by_id = {
        let mut p = Panel::new(db.clone(), cfg(&[("use_cyk", "false"), ("fallback_pages", "dest:1")]));
        p.tick(0.0, &inputs("", "1146", "")).unwrap().clone()
    };
    let mut p = Panel::new(db, cfg(&[("use_cyk", "false"), ("fallback_pages", "dest:1")]));
    let mut inp = inputs("", "", "");
    inp.dest.name = "Hlavní nádraží";
    assert_eq!(*p.tick(0.0, &inp).unwrap(), by_id);
}

#[test]
fn deterministic_and_quiet_when_nothing_changes() {
    let db = load_db();
    let inp = inputs("105", "1146", "1494");
    let mut a = Panel::new(db.clone(), Config::default());
    let mut b = Panel::new(db, Config::default());
    let fa = run(&mut a, &inp, 20.0);
    let fb = run(&mut b, &inp, 20.0);
    assert_eq!(fa, fb);
    // snímek se vrací jen při změně: statická stránka 4 s = 1 snímek + 8 kroků nasunutí
    assert!(fa.len() < 200, "{} snímků za 20 s", fa.len());
    let f = a.frame();
    assert_eq!(f.nibble_row(0, true).len(), W);
    assert!(f.nibble_row(0, false).bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn nibble_rows_encode_top_and_bottom_halves() {
    let f = Frame { width: 3, strips: vec![vec![0x80, 0x1F, 0xA5]] };
    assert_eq!(f.nibble_row(0, true), "81A");
    assert_eq!(f.nibble_row(0, false), "0F5");
    let mut buf = [0u16; 3];
    f.nibble_row_utf16(0, false, &mut buf);
    assert_eq!(buf, [b'0' as u16, b'F' as u16, b'5' as u16]);
}

#[test]
fn database_facts() {
    let db = load_db();
    let c = Config::default();
    let widest = db.zst.iter().map(|r| (page(&db, 1, &r.raw, &c.render).len(), &r.id)).max().unwrap();
    println!("nejširší stránka zastávky: {} sloupců (ZST {})", widest.0, widest.1);
    assert_eq!(widest.0, 111);
    let c0 = db.zst.iter().filter(|r| buse_engine::text::has_flag(&r.raw, 0xC0)).count();
    assert_eq!(c0, 474);
    // LIN 105 z příkladu v zadání v téhle databázi není -> linka se kreslí fallbackem
    assert!(db.lin("105").is_none() && db.lin("053").is_some());
}
