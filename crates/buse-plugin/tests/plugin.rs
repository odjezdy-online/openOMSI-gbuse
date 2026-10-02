//! Plugin jako celek přes jeho FFI exporty, ve stejném pořadí volání jako hra:
//! systémové proměnné -> proměnné vozu -> string proměnné.

use buse_panel::core::{parse_cfg, Opl};
use buse_panel::{AccessStringVariable, AccessSystemVariable, AccessVariable, PluginFinalize, PluginStart};

// (the operator's databases are not part of the repository: see docs/BUSE_PANELS.md)
const ROOT: &str = env!("BUSE_DATA");

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

struct Game {
    opl: Opl,
    vars: Vec<f32>,
    strings: Vec<Vec<u16>>,
    time: f32,
}

impl Game {
    fn set_str(&mut self, name: &str, v: &str) {
        let i = self.opl.strings.iter().position(|n| n == name).unwrap();
        self.strings[i] = wide(v);
    }

    fn str(&self, name: &str) -> String {
        let i = self.opl.strings.iter().position(|n| n == name).unwrap();
        String::from_utf16_lossy(&self.strings[i][..self.strings[i].len() - 1])
    }

    fn set_var(&mut self, name: &str, v: f32) {
        let i = self.opl.vars.iter().position(|n| n == name).unwrap();
        self.vars[i] = v;
    }

    fn var(&self, name: &str) -> f32 {
        self.vars[self.opl.vars.iter().position(|n| n == name).unwrap()]
    }

    /// Jeden snímek hry (dt v sekundách).
    fn frame(&mut self, dt: f32) {
        self.time += dt;
        let mut w = 0u8;
        unsafe {
            for (i, name) in self.opl.system.iter().enumerate() {
                let mut v = if name == "Time" { self.time } else { dt };
                AccessSystemVariable(i as u16, &mut v, &mut w);
            }
            for (i, v) in self.vars.iter_mut().enumerate() {
                w = 0;
                let mut x = *v;
                AccessVariable(i as u16, &mut x, &mut w);
                if w != 0 {
                    *v = x;
                }
            }
            for (i, s) in self.strings.iter_mut().enumerate() {
                w = 0;
                let before = s.len();
                AccessStringVariable(i as u16, s.as_mut_ptr(), &mut w);
                // plugin nesmí psát za buffer: délka (včetně nuly) se nemění
                assert_eq!(s.len(), before);
                assert_eq!(*s.last().unwrap(), 0, "ukončovací nula zůstala");
                if w != 0 {
                    let n = s.iter().position(|&c| c == 0).unwrap();
                    s.truncate(n + 1);
                }
            }
        }
    }
}

#[test]
fn plugin_end_to_end() {
    // složka pluginu: cfg + opl + databáze + mapa
    let dir = std::env::temp_dir().join(format!("buse_plugin_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg_text = "database = ADledA.hex\nslide = none\nin_line = str:Matrix_Nr\n\
                    in_dest_name = str:IBIS_terminus_name\nin_stop_name = str:IBIS_busstop_name\n\
                    in_stop_pressed = num:haltewunsch\nin_power = num:elec_busbar_main\n";
    let (cfg, pcfg, warn) = parse_cfg(cfg_text);
    assert!(warn.is_empty(), "{warn:?}");
    let opl = pcfg.opl(cfg.rows, "buse\\buse_panel.dll");
    assert_eq!(Opl::parse(&opl.to_text()), opl, "opl se po zápisu načte stejně");
    std::fs::write(dir.join("buse_panel.cfg"), cfg_text).unwrap();
    std::fs::write(dir.join("buse_panel.opl"), opl.to_text()).unwrap();
    std::fs::copy(format!("{ROOT}/data/ADledA.hex"), dir.join("ADledA.hex")).unwrap();
    std::fs::write(dir.join("zst_map.csv"), "# omsi_name;zst_id\nHlavák;1146\nNeznámá;\n").unwrap();
    std::env::set_var("BUSE_PANEL_DIR", &dir);

    PluginStart(std::ptr::null_mut());
    let log = std::fs::read_to_string(dir.join("buse_panel.log")).unwrap();
    assert!(log.contains("T0A0011_050603") && !log.contains("CHYBA"), "{log}");

    // skript vozu v {init} naplní výstupní stringy 135 nulami
    let zeros = "0".repeat(135);
    let mut game = Game {
        vars: vec![0.0; opl.vars.len()],
        strings: opl.strings.iter().map(|n| wide(if n.starts_with("BSLED_r") { &zeros } else { "" })).collect(),
        opl,
        time: 45_000.0,
    };
    game.set_var("elec_busbar_main", 1.0);
    game.set_str("Matrix_Nr", "53");
    game.set_str("IBIS_terminus_name", "Hlavák");
    game.set_str("IBIS_busstop_name", "Èeská"); // Windows-1250 čtené jako 1252
    game.frame(0.016);
    game.frame(0.016);
    let f1 = game.var("BSLED_frame");
    assert!(f1 >= 1.0);
    let (hi, lo) = (game.str("BSLED_r0_hi"), game.str("BSLED_r0_lo"));
    assert_eq!((hi.len(), lo.len()), (135, 135));
    assert!(hi.bytes().all(|b| b.is_ascii_hexdigit()) && lo != "0".repeat(135), "linka 53 svítí: {lo}");

    // stejný snímek z enginu napřímo
    let db = buse_engine::Db::from_hex(&std::fs::read_to_string(format!("{ROOT}/data/ADledA.hex")).unwrap()).unwrap();
    let mut panel = buse_engine::Panel::new(db, cfg);
    let inp = buse_engine::Inputs {
        line: "53",
        dest: buse_engine::StopRef { id: "1146", name: "Hlavák" },
        next_stop: buse_engine::StopRef { id: "", name: "Česká" },
        time_s: Some(45_000),
        ..Default::default()
    };
    let want = panel.tick(0.0, &inp).unwrap().clone();
    assert_eq!((hi, lo), (want.nibble_row(0, true), want.nibble_row(0, false)));

    // beze změny se čítač snímků nemění
    for _ in 0..20 {
        game.frame(0.016);
    }
    assert_eq!(game.var("BSLED_frame"), f1);

    // po 4 s stránka „cíl": mapa Hlavák -> 1146 Hlavní nádraží
    for _ in 0..260 {
        game.frame(0.016);
    }
    let mut inp2 = inp;
    let mut last = want.clone();
    for _ in 0..280 {
        if let Some(f) = panel.tick(0.016f32 as f64, &inp2) {
            last = f.clone();
        }
    }
    assert_ne!(last, want);
    assert_eq!(game.str("BSLED_r0_hi"), last.nibble_row(0, true), "plugin a engine běží stejně");
    assert!(game.var("BSLED_frame") > f1);

    // STOP -> stránka zastávky (fallback název s opraveným kódováním)
    game.set_var("haltewunsch", 1.0);
    game.frame(0.016);
    game.frame(0.016);
    panel.tick(0.016f32 as f64, &inp2);
    inp2.stop_pressed = true;
    let stop = panel.tick(0.016f32 as f64, &inp2).map(|f| f.clone()).unwrap_or_else(|| panel.frame().clone());
    assert_eq!(game.str("BSLED_r0_lo"), stop.nibble_row(0, false));

    // vypnutý hlavní vypínač -> panel zhasne
    game.set_var("elec_busbar_main", 0.0);
    game.frame(0.016);
    game.frame(0.016);
    assert_eq!(game.str("BSLED_r0_hi"), "0".repeat(135));
    assert_eq!(game.str("BSLED_r0_lo"), "0".repeat(135));

    // neinicializovaný (krátký) string: plugin zapíše jen to, co se vejde, a zaloguje to
    game.set_var("elec_busbar_main", 1.0);
    game.set_str("BSLED_r0_hi", "0000");
    game.frame(0.016);
    game.frame(0.016);
    assert_eq!(game.str("BSLED_r0_hi").len(), 4);

    PluginFinalize();
    let log = std::fs::read_to_string(dir.join("buse_panel.log")).unwrap();
    assert!(log.contains("má 4 znaků místo 135"), "{log}");
    assert!(log.contains("PluginFinalize"));
    println!("{log}");
    let _ = std::fs::remove_dir_all(&dir);
}
