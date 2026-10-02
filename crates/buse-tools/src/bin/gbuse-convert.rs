//! gbuse-convert: z databáze gBUSE (`.hex`) vyrobí všechno, co panel ve hře potřebuje.
//! Vnitřní LED panel (gBUSE1) i vnější panely - čelní, boční, zadní (gBUSE0); druh se pozná
//! z databáze.
//!
//!   gbuse-convert data/ADledA.hex -o out
//!       [--cfg buse_panel.cfg]            vlastní konfigurace (jinak se vygeneruje výchozí)
//!       [--hof soubor.hof]...             zst_map.csv z názvů zastávek a cílů (vnitřní panel)
//!       [--vehicle "…\Vehicles\SOR_NB12\SORNB12.bus"]   kopie vozu s panelem (originál se jen čte)
//!       [--suffix BSLED] [--pos X Y Z] [--size W H] [--led-size W H] [--lua]
//!
//! Další panel do téhož vozu: spustit znovu nad už vygenerovanou kopií vozu (`--vehicle
//! out\Vehicles\…\X_BSLED.bus --base "…\Vehicles\SOR_NB12"`) s jiným `--name`, jiným prefixem
//! proměnných v konfiguraci (`--prefix BSCEL`) a s polohou a natočením panelu:
//!
//!       --name buse_cel                   složka pluginu `plugins\buse_cel` (výchozí `buse`)
//!       --prefix BSCEL                    prefix proměnných a souborů (místo out_frame / out_prefix z cfg)
//!       --facing front|right|left|rear    odkud se na panel divák dívá (výchozí inner)
//!       --pitch 0.0102                    rozteč bodů v metrech (BS 210: 10,2 mm; BS 120: 4,7 mm)
//!       --color D7F23A                    barva svítícího bodu
//!       --width 28                        oříznutí šířky (zadní panel)
//!       --bus-name SORNB12_BUSE           jméno výsledného .bus (bez přípony)
//!       --friendly " + BS210"             co se připíše za jméno vozu v nabídce
//!
//! Výstup je strom se stejným rozložením jako složka hry (`plugins\`, `Fonts\`, `Vehicles\…`)
//! a `files.txt` se seznamem všech vytvořených souborů. Do složky hry se nic nezapisuje.

use buse_engine::{AnyDb, Config};
use buse_panel::core::{parse_cfg, PluginCfg, Source};
use buse_tools::assets::{self, Facing, Names, Quad};
use buse_tools::hof::{self, Hof};
use buse_tools::{vehicle, write_png};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "použití: gbuse-convert <db.hex> -o OUT [--cfg FILE] [--hof FILE]... [--vehicle FILE.bus] [--base DIR] \
[--suffix BSLED] [--name buse] [--prefix BSLED] [--pos X Y Z] [--size W H] [--led-size W H] [--facing inner|front|right|left|rear] \
[--pitch M] [--color RRGGBB] [--width N] [--bus-name NAME] [--friendly TEXT] [--map zst_map.csv] [--cell PX] [--lua] [--dll-path buse\\buse_panel.dll]";

const DEFAULT_CFG: &str = include_str!("../../assets/buse_panel.cfg");

struct Out {
    root: PathBuf,
    files: Vec<String>,
}

impl Out {
    fn write(&mut self, rel: &str, data: &[u8]) -> Result<PathBuf, String> {
        let path = self.root.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        self.files.push(rel.replace('/', "\\"));
        Ok(path)
    }

    fn png(&mut self, rel: &str, (w, h, rgb): (u32, u32, Vec<u8>)) -> Result<(), String> {
        let path = self.write(rel, &[])?;
        write_png(&path, w, h, png::ColorType::Rgb, &rgb)
    }
}

fn floats<const N: usize>(it: &mut std::slice::Iter<String>, what: &str) -> Result<[f32; N], String> {
    let mut out = [0.0; N];
    for v in &mut out {
        *v = it.next().and_then(|s| s.parse().ok()).ok_or(format!("{what}: čekám {N} čísel"))?;
    }
    Ok(out)
}

fn parse_color(v: &str) -> Result<[u8; 3], String> {
    let v = v.trim_start_matches('#');
    let n = u32::from_str_radix(v, 16).ok().filter(|_| v.len() == 6).ok_or(format!("--color: čekám RRGGBB, ne '{v}'"))?;
    Ok([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Nastaví (nebo přidá) klíč v textu konfigurace.
fn set_key(text: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let name = l.split(['#', ';']).next().unwrap_or("").split('=').next().unwrap_or("").trim();
            if name.eq_ignore_ascii_case(key) && l.contains('=') {
                found = true;
                format!("{key} = {value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("{key} = {value}"));
    }
    out.join("\r\n") + "\r\n"
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let (mut file, mut out_dir, mut cfg_file, mut bus, mut base) = (None, None, None, None, None::<String>);
    let (mut hofs, mut suffix, mut lua, mut cell) = (Vec::new(), None::<String>, false, 8u32);
    let (mut name, mut dll_path, mut prefix_arg, mut bus_name) = ("buse".to_string(), None::<String>, None::<String>, None::<String>);
    let (mut facing, mut pitch, mut color, mut width) = (Facing::Inner, None::<f32>, None::<[u8; 3]>, None::<String>);
    let mut friendly = None::<String>;
    let mut map_file = None::<String>;
    // výchozí poloha: pod stávajícím LCD ve voze SOR_NB12 (BUSE_pozadi.o3d: x -0.348..0.376,
    // y 2.557..2.745, z 4.325); rozměr krytu BS 120 je 72 x 9 cm
    let (mut pos, mut size, mut led_size) = ([0.014f32, 2.507, 4.3252], None::<[f32; 2]>, None::<[f32; 2]>);
    while let Some(a) = it.next() {
        let val = |it: &mut std::slice::Iter<String>| it.next().cloned().ok_or(format!("{a}: chybí hodnota\n{USAGE}"));
        match a.as_str() {
            "-o" | "--out" => out_dir = Some(val(&mut it)?),
            "--cfg" => cfg_file = Some(val(&mut it)?),
            "--hof" => hofs.push(val(&mut it)?),
            "--vehicle" => bus = Some(val(&mut it)?),
            "--base" => base = Some(val(&mut it)?),
            "--suffix" => suffix = Some(val(&mut it)?),
            "--name" => name = val(&mut it)?,
            "--prefix" => prefix_arg = Some(val(&mut it)?),
            "--bus-name" => bus_name = Some(val(&mut it)?),
            "--dll-path" => dll_path = Some(val(&mut it)?),
            "--cell" => cell = val(&mut it)?.parse().map_err(|_| "--cell: číslo")?,
            "--pos" => pos = floats(&mut it, "--pos")?,
            "--size" => size = Some(floats(&mut it, "--size")?),
            "--led-size" => led_size = Some(floats(&mut it, "--led-size")?),
            "--facing" => facing = Facing::parse(&val(&mut it)?)?,
            "--pitch" => pitch = Some(val(&mut it)?.parse().map_err(|_| "--pitch: číslo")?),
            "--color" => color = Some(parse_color(&val(&mut it)?)?),
            "--width" => width = Some(val(&mut it)?),
            "--friendly" => friendly = Some(val(&mut it)?),
            "--map" => map_file = Some(val(&mut it)?),
            "--lua" => lua = true,
            "-h" | "--help" => return Err(USAGE.into()),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return Err(format!("neznámý argument {a}\n{USAGE}")),
        }
    }
    let file = file.ok_or(USAGE)?;
    let mut out = Out { root: PathBuf::from(out_dir.ok_or(USAGE)?), files: Vec::new() };
    let raw_db = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
    let db = AnyDb::from_bytes(&raw_db).map_err(|e| format!("{file}: {e}"))?;
    println!("{}", db.describe());

    // --- konfigurace
    let cfg_text = match &cfg_file {
        Some(p) => String::from_utf8_lossy(&std::fs::read(p).map_err(|e| format!("{p}: {e}"))?).into_owned(),
        None => DEFAULT_CFG.to_string(),
    };
    let db_name = Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or("db.hex".into());
    let mut cfg_text = set_key(&cfg_text, "database", &db_name);
    if let Some(p) = &prefix_arg {
        cfg_text = set_key(&cfg_text, "out_frame", &format!("{p}_frame"));
        cfg_text = set_key(&cfg_text, "out_prefix", &format!("{p}_r"));
    }
    if let Some(w) = &width {
        cfg_text = set_key(&cfg_text, "width", w);
    }
    let (cfg, pcfg, warn): (Config, PluginCfg, _) = parse_cfg(&cfg_text);
    for w in &warn {
        eprintln!("[cfg] {w}");
    }
    if !pcfg.out_prefix.ends_with("_r") || !pcfg.out_frame.ends_with("_frame") {
        return Err("out_frame musí končit _frame a out_prefix _r (skript vozu z nich odvozuje jména)".into());
    }
    let prefix = pcfg.out_frame.trim_end_matches("_frame").to_string();
    if pcfg.out_prefix.trim_end_matches("_r") != prefix {
        return Err("out_frame a out_prefix musí mít stejný prefix".into());
    }
    let suffix = suffix.unwrap_or_else(|| prefix.clone());
    // rozměr panelu: šířka `auto` z databáze (vnitřní 135 x 8, čelní 140 x 19 …)
    let (width, height) = db.size(&cfg);
    let strips = height.div_ceil(8);
    println!("panel {width} x {height} bodů, {strips} pruhů po 8 řádcích, prefix {prefix}");

    // --- plugin: cfg, databáze, opl
    let plug = format!("plugins/{name}");
    let dll_path = dll_path.unwrap_or_else(|| format!("{name}\\buse_panel.dll"));
    out.write(&format!("{plug}/buse_panel.cfg"), cfg_text.as_bytes())?;
    out.write(&format!("{plug}/{db_name}"), &raw_db)?;
    // (.opl vedle DLL; první panel ho má o složku výš, jak to bylo)
    let opl_rel = if name == "buse" { "plugins/buse_panel.opl".to_string() } else { format!("{plug}/buse_panel.opl") };
    out.write(&opl_rel, pcfg.opl(strips, &dll_path).to_text().as_bytes())?;
    // hotová mapa názvů (z hof2hex): platí pro vnitřní i vnější panel
    let ready_map = match &map_file {
        Some(p) => Some(String::from_utf8_lossy(&std::fs::read(p).map_err(|e| format!("{p}: {e}"))?).into_owned()),
        None => None,
    };
    if let AnyDb::Inner(d) = &db {
        // (buse_db.bin se nepíše: plugin čte .hex přímo, takže úprava v gBUSE se projeví sama)
        if lua {
            out.write(&format!("{plug}/buse_db.lua"), assets::db_lua(d).as_bytes())?;
            out.write(&format!("{plug}/config.lua"), assets::config_lua(&cfg_text)?.as_bytes())?;
        }
        // --- mapa zastávek
        let mut parsed = Vec::new();
        for h in &hofs {
            let bytes = std::fs::read(h).map_err(|e| format!("{h}: {e}"))?;
            let name = Path::new(h).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            parsed.push((name, Hof::parse(&hof::decode(&bytes))));
        }
        let (csv, stats) = hof::zst_map_csv(d, &parsed);
        let csv = ready_map.clone().unwrap_or(csv);
        out.write(&format!("{plug}/zst_map.csv"), csv.as_bytes())?;
        if lua {
            let map = buse_panel::core::parse_zst_map(&csv);
            out.write(&format!("{plug}/zst_map.lua"), assets::zst_map_lua(&map).as_bytes())?;
        }
        println!(
            "zst_map.csv: {} názvů z {} HOF, přesná shoda {}, kandidát k ručnímu doplnění {}, bez shody {}",
            stats.names,
            parsed.len(),
            stats.exact,
            stats.candidates,
            stats.names - stats.exact - stats.candidates
        );
    } else {
        // vnější panel: čísla cílů z OMSI se s tabulkou CIL nepárují, názvy se kreslí volným textem
        let csv = ready_map.clone().unwrap_or_else(|| "# vnější panel: mapa názvů -> číslo cíle (omsi_name;id); bez ní se názvy kreslí volným textem\r\n".to_string());
        out.write(&format!("{plug}/zst_map.csv"), csv.as_bytes())?;
        if lua {
            eprintln!("[lua] vnější panely zatím umí jen DLL plugin; Lua soubory se pro {name} negenerují");
        }
    }

    // --- font
    out.write("Fonts/BUSE_nib.bmp", &assets::bmp24(assets::NIB_BMP_W, assets::NIB_BMP_H, &assets::nib_font_rgb()))?;
    out.write("Fonts/BUSE_nib.oft", assets::nib_font_oft("BUSE_nib.bmp").as_bytes())?;

    // --- vůz
    let power_var = match &pcfg.power {
        Some(Source::Num(name, _)) => name.clone(),
        _ => "elec_busbar_main".to_string(),
    };
    let (veh_rel, bus_info) = match &bus {
        Some(b) => {
            let p = Path::new(b);
            let dir = p.parent().ok_or("--vehicle: cesta k .bus")?;
            let name = dir.file_name().ok_or("--vehicle: složka vozu")?.to_string_lossy().into_owned();
            (format!("Vehicles/{name}"), Some((p.to_path_buf(), dir.to_path_buf())))
        }
        None => ("Vehicles/_BUSE_panel_example".to_string(), None),
    };
    let mut names = Names { prefix: prefix.clone(), width, strips, height, st_index: 0, power_var };
    // rozteč bodů: BS 120.0A 4,7 mm (LED 3 mm), terčové BS 210 10,2 mm (terč 9 mm) - certifikát
    // infopanelů BUSE pro IDS JMK
    let pitch = pitch.unwrap_or(if db.is_outer() { 0.0102 } else { 0.0047 });
    let color = color.unwrap_or(if db.is_outer() { [0xD7, 0xF2, 0x3A] } else { assets::LED_RED });
    let led = led_size.unwrap_or([width as f32 * pitch, height as f32 * pitch]);
    let size = size.unwrap_or(if db.is_outer() { [led[0] + 4.0 * pitch, led[1] + 2.0 * pitch] } else { [0.72, if height > 8 { 0.09 + 8.0 * pitch } else { 0.09 }] });
    let bg = Quad { c: pos, facing, w: size[0], h: size[1] };
    // svítící vrstva leží PŘED pozadím, o 1,5 mm blíž k divákovi (jinak ji kryt schová a panel
    // vypadá zhasnutý)
    let led_quad = Quad { w: led[0], h: led[1], ..bg }.toward_viewer(0.0015);
    let tex_dir = format!("{veh_rel}/Texture");
    out.png(&format!("{tex_dir}/{prefix}_led.png"), assets::led_texture(width as u32, height as u32, cell, true, color))?;
    // pozadí: stejná rozteč bodů jako svítící pole, body jen pro vzhled krytu
    let bg_cols = (bg.w / (led[0] / width as f32)).round().max(1.0) as u32;
    let bg_rows = (bg.h / (led[1] / height as f32)).round().max(1.0) as u32;
    out.png(&format!("{tex_dir}/{prefix}_bg.png"), assets::led_texture(bg_cols, bg_rows, cell, false, color))?;

    if let Some((bus_path, veh_dir)) = &bus_info {
        // soubory vozu: z jeho složky, a co v ní není (další běh nad vygenerovanou kopií), z --base
        let read = |rel: &Path| {
            let first = veh_dir.join(rel);
            std::fs::read(&first)
                .or_else(|e| match &base {
                    Some(b) => std::fs::read(Path::new(b).join(rel)),
                    None => Err(e),
                })
                .map(|b| vehicle::latin1(&b))
                .map_err(|e| format!("{}: {e}", first.display()))
        };
        let bus_text = std::fs::read(bus_path).map(|b| vehicle::latin1(&b)).map_err(|e| format!("{}: {e}", bus_path.display()))?;
        let stem = bus_path.file_stem().unwrap().to_string_lossy().into_owned();
        // model.cfg
        let bus_lines: Vec<&str> = bus_text.lines().map(|s| s.trim()).collect();
        let model_rel = bus_lines
            .iter()
            .position(|s| s.eq_ignore_ascii_case("[model]"))
            .and_then(|i| bus_lines.get(i + 1))
            .map(|s| s.to_string())
            .ok_or("[model] v .bus není")?;
        let model_text = read(Path::new(&model_rel))?;
        names.st_index = vehicle::scripttexture_count(&model_text);
        let model_dir_rel = Path::new(&model_rel).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let model_stem = Path::new(&model_rel).file_stem().unwrap().to_string_lossy().into_owned();
        let new_model_rel = format!("{model_dir_rel}\\{model_stem}_{suffix}.cfg");
        out.write(
            &format!("{veh_rel}/{new_model_rel}"),
            &vehicle::to_latin1(&vehicle::patch_model_cfg(&model_text, &names, &prefix)?),
        )?;
        out.write(&format!("{veh_rel}/{model_dir_rel}/{prefix}/{prefix}_bg.o3d"), &assets::o3d_quad(bg, &format!("{prefix}_bg.png")))?;
        out.write(&format!("{veh_rel}/{model_dir_rel}/{prefix}/{prefix}_led.o3d"), &assets::o3d_quad(led_quad, &format!("{prefix}_led.png")))?;
        // skripty
        let script_dir = format!("Script\\{prefix}");
        let mut replaced = Vec::new();
        for s in vehicle::list_entries(&bus_text, "[script]").ok_or("[script] v .bus není")? {
            let Ok(text) = read(Path::new(&s)) else { continue };
            if vehicle::is_main_script(&text) {
                let st = Path::new(&s).file_stem().unwrap().to_string_lossy().into_owned();
                let new_rel = format!("{script_dir}\\{st}_{suffix}.osc");
                out.write(&format!("{veh_rel}/{new_rel}"), &vehicle::to_latin1(&vehicle::patch_main_script(&text, &prefix)?))?;
                println!("hlavní skript {s} -> {new_rel}");
                replaced.push((s, new_rel));
            }
        }
        if replaced.is_empty() {
            return Err("žádný skript vozu nemá {init} a {frame}".into());
        }
        out.write(&format!("{veh_rel}/{script_dir}/buse_panel.osc"), names.osc().as_bytes())?;
        out.write(&format!("{veh_rel}/{script_dir}/buse_panel_varlist.txt"), names.varlist().as_bytes())?;
        out.write(&format!("{veh_rel}/{script_dir}/buse_panel_stringvarlist.txt"), names.stringvarlist().as_bytes())?;
        let patched = vehicle::patch_bus(
            &bus_text,
            &vehicle::BusPatch {
                model_cfg: &new_model_rel,
                replaced_scripts: &replaced,
                panel_script: &format!("{script_dir}\\buse_panel.osc"),
                varlist: &format!("{script_dir}\\buse_panel_varlist.txt"),
                stringvarlist: &format!("{script_dir}\\buse_panel_stringvarlist.txt"),
                name_suffix: friendly.as_deref().unwrap_or(if db.is_outer() { "" } else { " + BS120" }),
            },
        )?;
        let bus_out = bus_name.unwrap_or_else(|| format!("{stem}_{suffix}"));
        out.write(&format!("{veh_rel}/{bus_out}.bus"), &vehicle::to_latin1(&patched))?;
        println!("vůz: {veh_rel}/{bus_out}.bus, skriptová textura index {}, model {new_model_rel}", names.st_index);
        // proměnné, které plugin čte, musí vůz mít
        let mut have = String::new();
        for tag in ["[varnamelist]", "[stringvarnamelist]"] {
            for f in vehicle::list_entries(&bus_text, tag).unwrap_or_default() {
                if let Ok(t) = read(Path::new(&f)) {
                    have.push_str(&t.to_ascii_lowercase());
                    have.push('\n');
                }
            }
        }
        for src in pcfg.sources.iter().chain([&pcfg.power]).flatten() {
            let found = have.lines().any(|l| l.trim() == src.name().to_ascii_lowercase());
            println!("vstup {:<24} {}", src.name(), if found { "vůz proměnnou má" } else { "VŮZ PROMĚNNOU NEMÁ (uprav buse_panel.cfg)" });
        }
    } else {
        // bez konkrétního vozu: ukázkové soubory k ručnímu vložení
        names.st_index = 0;
        let d = format!("{veh_rel}/Model/{prefix}");
        out.write(&format!("{d}/{prefix}_bg.o3d"), &assets::o3d_quad(bg, &format!("{prefix}_bg.png")))?;
        out.write(&format!("{d}/{prefix}_led.o3d"), &assets::o3d_quad(led_quad, &format!("{prefix}_led.png")))?;
        let snippet = format!(
            "Ukazka pro model.cfg vozu. Index skriptove textury (zde {}) = pocet [scripttexture]\r\nbloku, ktere uz model ma; stejne cislo musi byt v buse_panel.osc a za \\S:.\r\n{}{}",
            names.st_index,
            names.cfg_scripttexture(),
            names.cfg_meshes(&prefix)
        );
        out.write(&format!("{veh_rel}/Model/model_snippet_{prefix}.cfg"), snippet.as_bytes())?;
        let sd = format!("{veh_rel}/Script/{prefix}");
        out.write(&format!("{sd}/buse_panel.osc"), names.osc().as_bytes())?;
        out.write(&format!("{sd}/buse_panel_varlist.txt"), names.varlist().as_bytes())?;
        out.write(&format!("{sd}/buse_panel_stringvarlist.txt"), names.stringvarlist().as_bytes())?;
    }

    // seznam souborů: při dalším běhu do téže složky se přidává k tomu, co už v ní je
    let list_path = out.root.join("files.txt");
    let mut list: Vec<String> = std::fs::read_to_string(&list_path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim_start_matches('\u{FEFF}').trim().to_string())
        .filter(|l| !l.is_empty() && out.root.join(l).is_file())
        .collect();
    list.extend(out.files.clone());
    list.sort();
    list.dedup();
    let text = list.join("\r\n") + "\r\n";
    std::fs::write(&list_path, text).map_err(|e| e.to_string())?;
    println!("{} souborů -> {} (seznam ve files.txt)", list.len(), out.root.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
