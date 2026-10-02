//! Lua port enginu (lua/buse) proti Rust enginu: golden testy, simulace snímek po snímku
//! a `main.lua` se stub tabulkou `omsi`. Lua 5.4 běží přes mlua (vendored), bez instalace.

use buse_engine::{Config, Db, Inputs, Panel, StopRef};
use buse_tools::assets::{config_lua, db_lua, lua_str, zst_map_lua};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

// (the operator's databases are not part of the repository: see docs/BUSE_PANELS.md)
const ROOT: &str = env!("BUSE_DATA");
const LUA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../buse-plugin/lua/buse");

fn load_db() -> Db {
    Db::from_hex(&std::fs::read_to_string(format!("{ROOT}/data/ADledA.hex")).unwrap()).unwrap()
}

fn lua_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Dočasná složka s `buse_db.lua` vygenerovaným converterem.
fn stage(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("buse_lua_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("buse_db.lua"), db_lua(&load_db())).unwrap();
    dir
}

fn run_script(script: &str, globals: &[(&str, String)]) -> mlua::Lua {
    let lua = mlua::Lua::new();
    let path = format!("{}/?.lua", lua_path(Path::new(LUA_DIR)));
    lua.load(format!("package.path = {} .. ';' .. package.path", lua_str(&path))).exec().unwrap();
    for (k, v) in globals {
        lua.globals().set(*k, v.as_str()).unwrap();
    }
    let file = Path::new(LUA_DIR).join("tests").join(script);
    let code = std::fs::read_to_string(&file).unwrap();
    if let Err(e) = lua.load(code).set_name(format!("@{script}")).exec() {
        panic!("{script}: {e}");
    }
    lua
}

#[test]
fn lua_golden_1037() {
    let dir = stage("golden");
    let lua = run_script(
        "run_golden.lua",
        &[
            ("BUSE_DB_PATH", lua_path(&dir.join("buse_db.lua"))),
            ("GOLDEN_PATH", lua_path(Path::new(&format!("{ROOT}/reference/golden_render.json")))),
        ],
    );
    let res: mlua::Table = lua.globals().get("GOLDEN_RESULT").unwrap();
    assert_eq!(res.get::<i64>("total").unwrap(), 1037);
    assert_eq!(res.get::<mlua::Table>("bad").unwrap().raw_len(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[derive(Clone, Default)]
struct Step {
    dt: f64,
    line: String,
    dest: (String, String),
    stop: (String, String),
    pressed: bool,
    request: Option<bool>,
    time: Option<u32>,
    zone: String,
    info: String,
}

/// Scénář: seznam (čas v sekundách, změna vstupu) + délka a krok.
fn scenario(len_s: f64, dt: f64, start: Step, events: &[(f64, fn(&mut Step))]) -> Vec<Step> {
    let (mut out, mut cur, mut t, mut next) = (Vec::new(), start, 0.0, 0);
    let t0 = cur.time;
    cur.dt = 0.0;
    while t <= len_s {
        while next < events.len() && events[next].0 <= t {
            (events[next].1)(&mut cur);
            next += 1;
        }
        cur.time = t0.map(|s| s + t as u32);
        out.push(cur.clone());
        // nepravidelný krok jako ve hře (každý sedmý snímek dvojnásobný)
        cur.dt = if out.len() % 7 == 0 { dt * 2.0 } else { dt };
        t += cur.dt;
    }
    out
}

fn case_lua(name: &str, cfg_text: &str, steps: &[Step]) -> (String, usize) {
    let (cfg, warn, rest) = Config::parse(cfg_text);
    assert!(warn.is_empty() && rest.is_empty(), "{name}: {warn:?} {rest:?}");
    let mut panel = Panel::new(load_db(), cfg);
    let mut s = format!("  {{ name = {}, cfg = {}, steps = {{\n", lua_str(name), lua_str(cfg_text));
    let mut frames = 0;
    for st in steps {
        let inp = Inputs {
            line: &st.line,
            dest: StopRef { id: &st.dest.0, name: &st.dest.1 },
            next_stop: StopRef { id: &st.stop.0, name: &st.stop.1 },
            stop_pressed: st.pressed,
            request_stop: st.request,
            time_s: st.time,
            zone: &st.zone,
            info: &st.info,
        };
        let _ = write!(
            s,
            "    {{ dt = {:?}, line = {}, dest_id = {}, dest_name = {}, stop_id = {}, stop_name = {}, pressed = {}, zone = {}, info = {}",
            st.dt,
            lua_str(&st.line),
            lua_str(&st.dest.0),
            lua_str(&st.dest.1),
            lua_str(&st.stop.0),
            lua_str(&st.stop.1),
            st.pressed,
            lua_str(&st.zone),
            lua_str(&st.info)
        );
        if let Some(r) = st.request {
            let _ = write!(s, ", request = {r}");
        }
        if let Some(t) = st.time {
            let _ = write!(s, ", time = {t}");
        }
        if let Some(f) = panel.tick(st.dt, &inp) {
            frames += 1;
            let rows = |hi: bool| -> String {
                (0..f.strips.len()).map(|i| lua_str(&f.nibble_row(i, hi))).collect::<Vec<_>>().join(", ")
            };
            let _ = write!(s, ", hi = {{ {} }}, lo = {{ {} }}", rows(true), rows(false));
        }
        s.push_str(" },\n");
    }
    let log: Vec<String> = panel.log().take().iter().map(|m| lua_str(m)).collect();
    let _ = writeln!(s, "  }}, log = {{ {} }} }},", log.join(", "));
    (s, frames)
}

#[test]
fn lua_panel_matches_rust_frame_by_frame() {
    let start = Step {
        line: "53".into(),
        dest: ("1146".into(), String::new()),
        stop: ("1001".into(), String::new()),
        time: Some(12 * 3600 + 34 * 60 + 50),
        ..Step::default()
    };
    let events: Vec<(f64, fn(&mut Step))> = vec![
        (6.0, |s| s.stop = ("1494".into(), String::new())),
        (9.0, |s| s.pressed = true),
        (15.0, |s| s.pressed = false),
        (17.0, |s| s.line = "N99".into()),
        (19.0, |s| s.dest = (String::new(), "Česká".into())),
        (21.0, |s| s.stop = (String::new(), "Nová Zastávka & spol.".into())),
        (24.0, |s| s.info = "Výluka: Čeština ěščřžýáíé ŮÚ ©".into()),
        (30.0, |s| s.zone = "101".into()),
        (33.0, |s| s.request = Some(true)),
        (34.0, |s| s.stop = ("1163".into(), String::new())),
        (38.0, |s| s.request = Some(false)),
        (39.0, |s| s.stop = ("1337".into(), String::new())),
        (44.0, |s| {
            s.line.clear();
            s.dest = Default::default();
            s.stop = Default::default();
            s.info.clear();
            s.zone.clear();
            s.time = None;
        }),
        (47.0, |s| s.line = "1".into()),
    ];
    let steps = scenario(52.0, 0.0166, start.clone(), &events);
    let cases = [
        ("BS120 výchozí", ""),
        ("BS120 úzký, bez nasouvání, záložní cyklus", "width = 80\nslide = none\nuse_cyk = false\nfallback_pages = line:1, dest:2, info:0, zone:1, time:1.5\nfallback_stop_pages = stop:3@1\nalign = center\n"),
        ("BS190", "rows = 2\nwidth = 64\nscroll_step_ms = 30\nscroll_gap = 5\nslide_steps = 4\nslide_step_ms = 25\nbs190_bottom = stop:2, time:2, info:1\n"),
        ("hypotézy vypnuté", "placeholder = dop:1\nplaceholder_flag = none\nplaceholder_dest = blank:3\nmissing_glyph = box\npict_alias = none\ncycle_idle = 0, 2, 9\ncycle_stop = 6\nvar_map = 01=line, 08=stop, 09=dest, 0D=time\nscroll_start_delay_ms = 300\nscroll_full_pass = false\nstop_on_press = false\nhold_ms = 1500\n"),
        ("jiné fonty a symbol", "placeholder = symbol:E3:AF\nplaceholder_else = glyph\nfallback_line_font = E0\nfallback_stop_font = E2\ndefault_font = E3\nstop_on_change = false\ncyk_effect_slides = false\ncyk_time_unit_ms = 700\nwidth = 96\nmissing_glyph = skip\n"),
    ];
    let mut text = String::from("return {\n");
    let mut total = 0;
    for (name, cfg) in cases {
        let (case, frames) = case_lua(name, cfg, &steps);
        assert!(frames > 30, "{name}: jen {frames} snímků");
        total += frames;
        text.push_str(&case);
    }
    text.push_str("}\n");
    let dir = stage("sim");
    std::fs::write(dir.join("sim_cases.lua"), &text).unwrap();
    let lua = run_script(
        "run_sim.lua",
        &[("BUSE_DB_PATH", lua_path(&dir.join("buse_db.lua"))), ("SIM_PATH", lua_path(&dir.join("sim_cases.lua")))],
    );
    let res: mlua::Table = lua.globals().get("SIM_RESULT").unwrap();
    assert_eq!(res.get::<usize>("cases").unwrap(), cases.len());
    assert_eq!(res.get::<usize>("frames").unwrap(), total);
    assert_eq!(res.get::<usize>("steps").unwrap(), steps.len() * cases.len());
    println!("Lua vs Rust: {} scénářů, {} kroků, {total} snímků shodných", cases.len(), steps.len() * cases.len());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lua_main_with_omsi_stub() {
    let dir = stage("main");
    for f in ["main.lua", "engine.lua"] {
        std::fs::copy(Path::new(LUA_DIR).join(f), dir.join(f)).unwrap();
    }
    std::fs::write(dir.join("config.lua"), config_lua("slide = none\nin_line = str:Matrix_Nr\n").unwrap()).unwrap();
    let map = buse_panel::core::parse_zst_map("Hlavák;1146\n");
    std::fs::write(dir.join("zst_map.lua"), zst_map_lua(&map)).unwrap();
    let lua = run_script("run_main.lua", &[("PLUGIN_DIR", lua_path(&dir))]);
    let res: mlua::Table = lua.globals().get("MAIN_RESULT").unwrap();
    assert!(res.get::<f64>("frames").unwrap() >= 3.0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lua_helpers_match_rust() {
    // normalizace, skóre, oprava kódování a parser konfigurace musí dávat totéž co Rust
    let lua = mlua::Lua::new();
    let path = format!("{}/?.lua", lua_path(Path::new(LUA_DIR)));
    lua.load(format!("package.path = {} .. ';' .. package.path; E = require('engine')", lua_str(&path))).exec().unwrap();
    let names = ["Nám. Míru", "Žel. st. Řečkovice", "  Česká  ", "Hlavní nádraží, nást. 3", "Kr.Pole nádraží", "ÚAN Zvonařka (x)", "Èerná Pole", "Øeèkovice", "Štefánikova čtvrť", ""];
    for a in names {
        let n: String = lua.load(format!("return E.normalize({})", lua_str(a))).eval().unwrap();
        assert_eq!(n, buse_engine::names::normalize(a), "normalize {a:?}");
        let m: String = lua.load(format!("return E.fix_mojibake({})", lua_str(a))).eval().unwrap();
        assert_eq!(m, buse_engine::names::fix_mojibake(a), "fix_mojibake {a:?}");
        for b in names {
            let (na, nb) = (buse_engine::names::normalize(a), buse_engine::names::normalize(b));
            let s: u32 = lua.load(format!("return E.match_score({}, {})", lua_str(&na), lua_str(&nb))).eval().unwrap();
            assert_eq!(s, buse_engine::names::match_score(&na, &nb), "match_score {na:?} {nb:?}");
        }
    }
    let bad: usize = lua.load("local _, w = E.parse_config('width = x\\nslide = bogus\\nrows = 2\\nfoo = 1\\n') return #w").eval().unwrap();
    let (cfg, warn, rest) = Config::parse("width = x\nslide = bogus\nrows = 2\nfoo = 1\n");
    assert_eq!((bad, cfg.rows, rest.len()), (warn.len(), 2, 1));
}
