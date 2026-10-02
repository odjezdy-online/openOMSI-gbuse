//! Golden test: `render_text` musí bit po bitu sedět s `reference/golden_render.json`.

use buse_engine::{render_text, Db, Log, RenderOpts};

// (the operator's databases are not part of the repository: see docs/BUSE_PANELS.md)
const ROOT: &str = env!("BUSE_DATA");

fn load_db() -> Db {
    let hex = std::fs::read_to_string(format!("{ROOT}/data/ADledA.hex")).unwrap();
    Db::from_hex(&hex).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
}

#[test]
fn golden_render_matches_reference() {
    let db = load_db();
    let json = std::fs::read_to_string(format!("{ROOT}/reference/golden_render.json")).unwrap();
    let gold: serde_json::Value = serde_json::from_str(&json).unwrap();
    let items = gold["items"].as_array().unwrap();
    assert_eq!(items.len(), 1037);
    let opts = RenderOpts::reference();
    let mut log = Log::new();
    let mut bad = Vec::new();
    for it in items {
        let raw = unhex(it["raw"].as_str().unwrap());
        let want: Vec<u8> =
            it["cols"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
        if render_text(&db, &raw, &opts, &mut log) != want {
            bad.push(format!("{} {}", it["table"], it["id"]));
        }
    }
    assert!(bad.is_empty(), "{} z {} nesedí: {:?}", bad.len(), items.len(), &bad[..bad.len().min(10)]);
    println!("golden: {0}/{0} sedí; hlášky enginu: {1:?}", items.len(), log.take());
}

#[test]
fn database_matches_reference_json() {
    let db = load_db();
    let json = std::fs::read_to_string(format!("{ROOT}/reference/ADledA.json")).unwrap();
    let r: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(db.image.len() as u64, r["size"].as_u64().unwrap());
    assert_eq!(db.name.as_deref(), r["name"].as_str());
    let secs = r["sections"].as_array().unwrap();
    assert_eq!(db.sections.len(), secs.len());
    for (s, want) in db.sections.iter().zip(secs) {
        assert_eq!(s.tag, want["tag"].as_str().unwrap());
        assert_eq!(s.header as u64, want["header"].as_u64().unwrap());
        assert_eq!(s.data as u64, want["data"].as_u64().unwrap());
        assert_eq!(s.version, want["version"].as_str().unwrap());
    }
    let fonts = r["fonts"].as_object().unwrap();
    assert_eq!(db.fonts().count(), fonts.len());
    for (fid, glyphs) in fonts {
        let font = db.font(u8::from_str_radix(fid, 16).unwrap()).unwrap();
        let glyphs = glyphs.as_object().unwrap();
        assert_eq!(font.codes().count(), glyphs.len(), "font {fid}");
        for (code, g) in glyphs {
            let got = font.glyph(u8::from_str_radix(code, 16).unwrap()).unwrap();
            let cols: Vec<u16> =
                g["cols"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u16).collect();
            assert_eq!(got.cols, &cols[..], "font {fid} glyf {code}");
            assert_eq!(got.h as u64, g["h"].as_u64().unwrap());
        }
    }
    for (code, ch) in r["charmap"].as_object().unwrap() {
        let code = u8::from_str_radix(code, 16).unwrap();
        if code >= 0x80 {
            assert_eq!(db.code_to_char(code).unwrap().to_string(), ch.as_str().unwrap());
        }
    }
    for (tab, rows) in [("LIN", &db.lin), ("ZST", &db.zst)] {
        let want = r[tab].as_array().unwrap();
        assert_eq!(rows.len(), want.len(), "{tab}");
        for (row, w) in rows.iter().zip(want) {
            assert_eq!(row.id, w["id"].as_str().unwrap());
            assert_eq!(row.raw, unhex(w["raw"].as_str().unwrap()));
            let text = buse_engine::text::plain(&db, &row.raw);
            assert_eq!(text, w["text"].as_str().unwrap(), "{tab} {}", row.id);
        }
    }
    assert_eq!((db.lin.len(), db.zst.len()), (337, 693));
    let dop = r["DOP"].as_array().unwrap();
    assert_eq!(db.dop.len(), dop.len());
    for ((id, raw), w) in db.dop.iter().zip(dop) {
        assert_eq!(id.to_string(), w["id"].as_str().unwrap());
        assert_eq!(*raw, unhex(w["raw"].as_str().unwrap()));
    }
    let cyk = r["CYK"].as_array().unwrap();
    assert_eq!(db.cyk.len(), cyk.len());
    for (c, w) in db.cyk.iter().zip(cyk) {
        assert_eq!(c.id as u64, w["id"].as_u64().unwrap());
        assert_eq!(c.raw, unhex(w["raw"].as_str().unwrap()));
        let pages = w["pages"].as_array().unwrap();
        assert_eq!(c.pages.len(), pages.len());
        for (p, wp) in c.pages.iter().zip(pages) {
            assert_eq!(p.brk, wp["break"].as_bool().unwrap());
            if p.brk {
                continue;
            }
            assert_eq!(p.width as u64, wp["width"].as_u64().unwrap());
            assert_eq!(p.dop as u64, wp["dop"].as_u64().unwrap());
            assert_eq!(p.var as u64, wp["var"].as_u64().unwrap());
            assert_eq!(p.mode as u64, wp["mode"].as_u64().unwrap());
            assert_eq!(p.time as u64, wp["time"].as_u64().unwrap());
        }
        let names: Vec<&str> =
            w["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
        assert_eq!(c.names, names, "cyklus {}", c.id);
    }
}

#[test]
fn bin_roundtrip() {
    let db = load_db();
    let db2 = Db::from_bin(&db.to_bin()).unwrap();
    assert_eq!(db.zst, db2.zst);
    assert!(Db::from_bin(b"nope").is_err());
}
