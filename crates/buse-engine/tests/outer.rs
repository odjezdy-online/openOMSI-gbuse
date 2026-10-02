//! Vnější panely (databáze gBUSE0): okna polí, středění, dvouřádkové cíle, inverze, volný text.

use buse_engine::outer::{FIELD_DEST, FIELD_LINE, FIELD_STOP};
use buse_engine::{AnyDb, AnyPanel, Config, Inputs, OuterConfig, OuterDb, OuterPanel, StopRef};

// (the operator's databases are not part of the repository: see docs/BUSE_PANELS.md)
const ROOT: &str = env!("BUSE_DATA");

fn load(name: &str) -> OuterDb {
    OuterDb::from_hex(&std::fs::read_to_string(format!("{ROOT}/data/{name}")).unwrap()).unwrap()
}

fn show(db: OuterDb, line: &str, dest: (&str, &str), stop: (&str, &str)) -> Vec<String> {
    let inp = Inputs { line, dest: StopRef { id: dest.0, name: dest.1 }, next_stop: StopRef { id: stop.0, name: stop.1 }, ..Inputs::default() };
    let mut p = OuterPanel::new(db, OuterConfig::default());
    p.tick(0.0, &inp).expect("první snímek");
    p.ascii().lines().map(str::to_string).collect()
}

/// Rozsah sloupců, ve kterých v daných řádcích něco svítí.
fn lit_cols(rows: &[String], from: usize, to: usize) -> Option<(usize, usize)> {
    let cols: Vec<usize> = rows[from..to].iter().flat_map(|r| r.char_indices().filter(|c| c.1 == '#').map(|c| c.0)).collect();
    Some((*cols.iter().min()?, *cols.iter().max()?))
}

#[test]
fn header_gives_the_fields_of_each_panel() {
    let cel = load("ADcel0.hex");
    assert_eq!((cel.width, cel.rows), (140, 19));
    let (line, dest) = (cel.windows[FIELD_LINE].unwrap(), cel.windows[FIELD_DEST].unwrap());
    assert_eq!((line.bottom, line.top, line.left, line.right), (0, 18, 0, 27));
    assert_eq!((dest.bottom, dest.top, dest.left, dest.right), (0, 18, 28, 139));
    assert!(cel.windows[FIELD_STOP].is_none());
    assert_eq!((cel.lin.len(), cel.cil.len()), (336, 1000));
    // boční panel: cíl ve spodních devíti řádcích, nácestné zastávky (DRU) v horních deseti
    let bok = load("ADbok4.hex");
    assert_eq!((bok.width, bok.rows), (112, 19));
    let (dest, stop) = (bok.windows[FIELD_DEST].unwrap(), bok.windows[FIELD_STOP].unwrap());
    assert_eq!((dest.bottom, dest.top), (0, 8));
    assert_eq!((stop.bottom, stop.top, stop.left, stop.right), (9, 18, 28, 111));
    assert_eq!(bok.dru.len(), 737);
    // fonty do výšky 28 řádků se načtou všechny
    assert_eq!(cel.fonts().count(), 16);
    assert_eq!(cel.font(0xE8).unwrap().glyph(b'5').unwrap().h, 19);
}

#[test]
fn line_and_two_line_destination_are_centred_in_their_fields() {
    // CIL 105: `E3 B1 C0 ŽIDENICE 0A E3 B1 CA GEISLEROVA` - dva řádky, druhý o 10 řádků níž
    let rows = show(load("ADcel0.hex"), "53", ("105", ""), ("", ""));
    assert_eq!(rows.len(), 19);
    let line = lit_cols(&rows, 0, 19).unwrap();
    assert!(line.0 < 28, "linka je v poli 0..27");
    // číslo linky: jen sloupce 0..27, na střed
    let l: Vec<String> = rows.iter().map(|r| r[..28].to_string()).collect();
    let (a, b) = lit_cols(&l, 0, 19).unwrap();
    assert!((a as i32 - (27 - b as i32)).abs() <= 1, "linka na střed pole: {a}..{b}");
    // cíl: horní řádek v řádcích 0..10, dolní 10..19, oba na střed pole 28..139
    let d: Vec<String> = rows.iter().map(|r| r[28..].to_string()).collect();
    for (from, to) in [(0, 10), (10, 19)] {
        let (a, b) = lit_cols(&d, from, to).unwrap();
        assert!((a as i32 - (111 - b as i32)).abs() <= 1, "řádky {from}..{to} na střed: {a}..{b}");
    }
    assert!(lit_cols(&d, 10, 12).is_none() || lit_cols(&d, 9, 10).is_none(), "mezi řádky textu je mezera");
}

#[test]
fn inverse_text_takes_the_destination_field() {
    // CIL 982: `ESC w ESC h 22 | E7 C0 FB 0C | E5 B1 C1 CVIČNÁ JÍZDA 0C | ESC i` - inverzní nápis
    let rows = show(load("ADcel0.hex"), "53", ("982", ""), ("", ""));
    let field: Vec<&str> = rows.iter().map(|r| &r[28..]).collect();
    let lit = field.iter().map(|r| r.matches('#').count()).sum::<usize>();
    assert!(lit > 112 * 19 * 6 / 10, "pole cíle je po inverzi převážně rozsvícené ({lit} bodů)");
    assert!(field[0].chars().all(|c| c == '#'), "horní řádek pole svítí celý");
    // linka vlevo zůstala normální
    assert!(rows[0][..28].contains('#') && rows[0][..28].contains('.'));
}

#[test]
fn side_panel_puts_via_stops_above_the_destination() {
    // DRU 1001 `-Achtelky-` nahoře (řádky 0..9), cíl volným textem dole (řádky 10..18)
    let rows = show(load("ADbok4.hex"), "53", ("", "Bystrc"), ("1001", ""));
    let right: Vec<String> = rows.iter().map(|r| r[28..].to_string()).collect();
    assert!(lit_cols(&right, 0, 10).is_some(), "nácestná zastávka nahoře");
    assert!(lit_cols(&right, 10, 19).is_some(), "cíl dole");
    // obojí na střed pole 28..111
    for (from, to) in [(0, 10), (10, 19)] {
        let (a, b) = lit_cols(&right, from, to).unwrap();
        assert!((a as i32 - (83 - b as i32)).abs() <= 1, "řádky {from}..{to}: {a}..{b}");
    }
}

#[test]
fn names_outside_the_database_are_fitted_by_themselves() {
    // krátký název: jeden řádek velkým fontem; dlouhý: dva řádky; linka mimo LIN: volný text
    let short = show(load("ADcel0.hex"), "N99", ("", "Bystrc"), ("", ""));
    assert!(lit_cols(&short, 0, 19).unwrap().0 < 28, "linka N99 se nakreslí i bez záznamu v LIN");
    let d: Vec<String> = short.iter().map(|r| r[28..].to_string()).collect();
    let rows_lit = d.iter().filter(|r| r.contains('#')).count();
    assert!(rows_lit >= 12, "krátký název velkým fontem ({rows_lit} řádků)");
    let long = show(load("ADcel0.hex"), "1", ("", "Bystrc Pod Mniší horou přístaviště"), ("", ""));
    let d: Vec<String> = long.iter().map(|r| r[28..].to_string()).collect();
    assert!(lit_cols(&d, 0, 9).is_some() && lit_cols(&d, 11, 19).is_some(), "dlouhý název na dva řádky");
    let (a, b) = lit_cols(&d, 0, 19).unwrap();
    assert!(a < 112 && b < 112);
}

#[test]
fn any_panel_picks_the_engine_from_the_database() {
    let bytes = std::fs::read(format!("{ROOT}/data/ADzad7.hex")).unwrap();
    let db = AnyDb::from_bytes(&bytes).unwrap();
    assert!(db.is_outer());
    // zadní panel: databáze má okna jako čelní, konfigurace ho ořízne na 28 sloupců
    let mut cfg = Config::default();
    assert_eq!(cfg.set("width", "28"), Ok(true));
    assert_eq!(db.size(&cfg), (28, 19));
    let mut p = AnyPanel::new(db, cfg);
    let inp = Inputs { line: "53", ..Inputs::default() };
    let f = p.tick(0.0, &inp).unwrap().clone();
    assert_eq!((f.width, f.strips.len()), (28, 3));
    assert!(f.strips[0].iter().any(|&c| c != 0));
    assert!(f.strips[2].iter().all(|&c| c & 0x1F == 0), "pod 19. řádkem nic nesvítí");
    assert!(p.tick(1.0, &inp).is_none(), "statický text se nemění");
    // vnitřní databáze dá vnitřní panel
    let inner = AnyDb::from_bytes(&std::fs::read(format!("{ROOT}/data/ADledA.hex")).unwrap()).unwrap();
    assert!(!inner.is_outer());
    assert_eq!(inner.size(&Config::default()), (135, 8));
}

#[test]
fn every_outer_database_renders_every_text() {
    // žádný text žádné databáze nesmí engine shodit ani zacyklit
    let dir = std::fs::read_dir(format!("{ROOT}/data")).unwrap();
    let mut n = 0;
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".hex") {
            continue;
        }
        let Ok(AnyDb::Outer(db)) = AnyDb::from_bytes(&std::fs::read(e.path()).unwrap()) else { continue };
        let ids: Vec<String> = db.cil.iter().map(|r| r.id.clone()).collect();
        let line = db.lin.first().map(|r| r.id.clone()).unwrap_or_default();
        let mut p = OuterPanel::new(db, OuterConfig { step_ms: 50.0, ..OuterConfig::default() });
        for id in ids {
            let inp = Inputs { line: &line, dest: StopRef { id: &id, name: "" }, ..Inputs::default() };
            p.tick(0.0, &inp);
            for _ in 0..40 {
                p.tick(0.05, &inp);
            }
            n += 1;
        }
    }
    assert!(n > 1200, "{n} cílů");
}
