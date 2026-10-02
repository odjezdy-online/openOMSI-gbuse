//! Jádro pluginu nezávislé na FFI: konfigurace, mapování proměnných vozu, snímek -> stringy.

use buse_engine::config::parse_key_values;
use buse_engine::names::{fix_mojibake, normalize};
use buse_engine::{AnyDb, AnyPanel, Config, Inputs, StopRef};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CFG_NAME: &str = "buse_panel.cfg";
pub const OPL_NAME: &str = "buse_panel.opl";
pub const LOG_NAME: &str = "buse_panel.log";
const LOG_LIMIT: u32 = 400;

/// Seznamy proměnných z `.opl` (index = pořadí v seznamu, jak ho posílá hra).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Opl {
    pub dll: String,
    pub vars: Vec<String>,
    pub strings: Vec<String>,
    pub system: Vec<String>,
    pub triggers: Vec<String>,
}

impl Opl {
    pub fn parse(text: &str) -> Opl {
        let mut opl = Opl::default();
        let mut lines = text.lines().map(|l| l.trim_start_matches('\u{FEFF}').trim());
        while let Some(line) = lines.next() {
            let list = match line.to_ascii_lowercase().as_str() {
                "[dll]" => {
                    opl.dll = lines.next().unwrap_or("").to_string();
                    continue;
                }
                "[varlist]" => &mut opl.vars,
                "[stringvarlist]" => &mut opl.strings,
                "[systemvarlist]" => &mut opl.system,
                "[triggers]" => &mut opl.triggers,
                _ => continue,
            };
            let n: usize = lines.next().and_then(|l| l.parse().ok()).unwrap_or(0);
            for _ in 0..n {
                match lines.next() {
                    Some(name) => list.push(name.to_string()),
                    None => break,
                }
            }
        }
        opl
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("BUSE LED panel\r\n[dll]\r\n{}\r\n", self.dll);
        for (tag, list) in [
            ("varlist", &self.vars),
            ("stringvarlist", &self.strings),
            ("systemvarlist", &self.system),
            ("triggers", &self.triggers),
        ] {
            if !list.is_empty() {
                s.push_str(&format!("\r\n[{tag}]\r\n{}\r\n", list.len()));
                for name in list {
                    s.push_str(name);
                    s.push_str("\r\n");
                }
            }
        }
        s
    }
}

fn find(list: &[String], name: &str) -> Option<u16> {
    list.iter().position(|n| n.eq_ignore_ascii_case(name)).map(|i| i as u16)
}

/// Zdroj vstupu: `str:JMENO`, `num:JMENO` nebo `num:JMENO/DELITEL`.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Str(String),
    Num(String, f32),
}

impl Source {
    pub fn parse(v: &str) -> Result<Option<Source>, String> {
        let v = v.trim();
        if v.is_empty() || v.eq_ignore_ascii_case("none") {
            return Ok(None);
        }
        let (kind, rest) = v.split_once(':').ok_or(format!("čekám str:JMENO nebo num:JMENO, ne '{v}'"))?;
        match kind.trim().to_ascii_lowercase().as_str() {
            "str" => Ok(Some(Source::Str(rest.trim().to_string()))),
            "num" => {
                let (name, div) = match rest.split_once('/') {
                    Some((n, d)) => (n, d.trim().parse::<f32>().map_err(|_| format!("dělitel '{d}'"))?),
                    None => (rest, 1.0),
                };
                Ok(Some(Source::Num(name.trim().to_string(), if div == 0.0 { 1.0 } else { div })))
            }
            other => Err(format!("neznámý typ zdroje '{other}'")),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Source::Str(n) | Source::Num(n, _) => n,
        }
    }
}

/// Role vstupů panelu (pořadí = index do `Core::values`).
pub const N_ROLES: usize = 11;
pub const ROLES: [&str; N_ROLES] =
    ["line", "dest_name", "dest_code", "stop_name", "stop_code", "stop_pressed", "request", "zone", "info", "via", "variant"];
const R_LINE: usize = 0;
const R_DEST_NAME: usize = 1;
const R_DEST_CODE: usize = 2;
const R_STOP_NAME: usize = 3;
const R_STOP_CODE: usize = 4;
const R_PRESSED: usize = 5;
const R_REQUEST: usize = 6;
const R_ZONE: usize = 7;
const R_INFO: usize = 8;
/// Nácestné zastávky oddělené `|`: plugin je po jedné střídá v poli zastávky vnějšího panelu.
const R_VIA: usize = 9;
/// Číslo provedení panelu ve voze (např. `sv_matrix_typ`): vybírá databázi `database_N`.
const R_VARIANT: usize = 10;

/// Nastavení pluginu (klíče, které engine nezná).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginCfg {
    pub database: String,
    pub zst_map: String,
    pub sources: [Option<Source>; N_ROLES],
    /// `database_N = soubor`: databáze pro provedení N (vstup `in_variant`).
    pub database_alt: Vec<(u32, String)>,
    /// Doba jedné nácestné zastávky v poli zastávky (ms) a text před seznamem.
    pub via_step_ms: f64,
    pub via_header: String,
    pub power: Option<Source>,
    pub out_frame: String,
    pub out_prefix: String,
    pub fix_encoding: bool,
    pub zone_text: String,
    pub info_text: String,
    /// Jen pro test z oddílu 3.1: zapíše do string proměnné o znak víc, než má buffer.
    pub debug_overrun: bool,
}

impl Default for PluginCfg {
    fn default() -> Self {
        let src = |s: &str| Source::parse(s).ok().flatten();
        PluginCfg {
            database: "ADledA.hex".into(),
            zst_map: "zst_map.csv".into(),
            sources: [
                src("str:Matrix_Nr"),
                src("str:IBIS_terminus_name"),
                None,
                src("str:IBIS_busstop_name"),
                None,
                src("num:haltewunsch"),
                None,
                None,
                None,
                None,
                None,
            ],
            database_alt: Vec::new(),
            via_step_ms: 1200.0,
            via_header: "P\u{159}es zast\u{e1}vky:".into(),
            power: src("num:elec_busbar_main"),
            out_frame: "BSLED_frame".into(),
            out_prefix: "BSLED_r".into(),
            fix_encoding: true,
            zone_text: String::new(),
            info_text: String::new(),
            debug_overrun: false,
        }
    }
}

impl PluginCfg {
    pub fn set(&mut self, key: &str, v: &str) -> Result<bool, String> {
        let flag = |v: &str| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on" | "ano" | "auto");
        if let Some(role) = key.strip_prefix("in_") {
            if role == "power" {
                self.power = Source::parse(v)?;
                return Ok(true);
            }
            return match ROLES.iter().position(|r| *r == role) {
                Some(i) => {
                    self.sources[i] = Source::parse(v)?;
                    Ok(true)
                }
                None => Ok(false),
            };
        }
        if let Some(n) = key.strip_prefix("database_").and_then(|n| n.parse::<u32>().ok()) {
            self.database_alt.retain(|a| a.0 != n);
            if !v.is_empty() {
                self.database_alt.push((n, v.to_string()));
            }
            return Ok(true);
        }
        match key {
            "via_step_ms" => self.via_step_ms = v.parse::<f64>().map_err(|_| format!("\u{10d}\u{ed}slo, ne '{v}'"))?.max(100.0),
            "via_header" => self.via_header = v.to_string(),
            "database" => self.database = v.to_string(),
            "zst_map" => self.zst_map = v.to_string(),
            "out_frame" => self.out_frame = v.to_string(),
            "out_prefix" => self.out_prefix = v.to_string(),
            "fix_encoding" => self.fix_encoding = flag(v),
            "zone_text" => self.zone_text = v.to_string(),
            "info_text" => self.info_text = v.to_string(),
            "debug_overrun" => self.debug_overrun = flag(v),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Jména výstupních string proměnných pro `rows` pruhů: `<prefix>0_hi`, `<prefix>0_lo`, ...
    pub fn row_names(&self, rows: usize) -> Vec<String> {
        (0..rows).flat_map(|r| ["hi", "lo"].map(|h| format!("{}{r}_{h}", self.out_prefix))).collect()
    }

    /// `.opl` odpovídající této konfiguraci (generuje ho converter).
    pub fn opl(&self, rows: usize, dll: &str) -> Opl {
        let mut opl = Opl { dll: dll.to_string(), ..Opl::default() };
        opl.vars.push(self.out_frame.clone());
        opl.strings = self.row_names(rows);
        for src in self.sources.iter().chain([&self.power]).flatten() {
            let list = match src {
                Source::Str(_) => &mut opl.strings,
                Source::Num(..) => &mut opl.vars,
            };
            if find(list, src.name()).is_none() {
                list.push(src.name().to_string());
            }
        }
        opl.system = vec!["Time".into(), "Timegap".into()];
        opl
    }
}

/// Načte `klíč = hodnota` do konfigurace enginu a pluginu; vrací i hlášky.
pub fn parse_cfg(text: &str) -> (Config, PluginCfg, Vec<String>) {
    let (mut cfg, mut pcfg, mut warn) = (Config::default(), PluginCfg::default(), Vec::new());
    for (k, v) in parse_key_values(text) {
        match cfg.set(&k, &v) {
            Ok(true) => {}
            Err(e) => warn.push(format!("{k}: {e}")),
            Ok(false) => match pcfg.set(&k, &v) {
                Ok(true) => {}
                Ok(false) => warn.push(format!("neznámý klíč '{k}'")),
                Err(e) => warn.push(format!("{k}: {e}")),
            },
        }
    }
    (cfg, pcfg, warn)
}

/// `zst_map.csv`: řádky `omsi_name;zst_id`; `#` komentář; prázdné id = bez mapování.
pub fn parse_zst_map(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{FEFF}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split(';');
        if let (Some(name), Some(id)) = (it.next(), it.next()) {
            let (name, id) = (normalize(name), id.trim());
            if !name.is_empty() && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                map.insert(name, id.to_string());
            }
        }
    }
    map
}

struct Logger {
    file: Option<std::fs::File>,
    lines: u32,
}

impl Logger {
    fn line(&mut self, msg: &str) {
        if self.lines >= LOG_LIMIT {
            return;
        }
        self.lines += 1;
        if let Some(f) = &mut self.file {
            let _ = writeln!(f, "{msg}");
            if self.lines == LOG_LIMIT {
                let _ = writeln!(f, "(další hlášky se už nezapisují)");
            }
            let _ = f.flush();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Slot {
    None,
    Role(usize, f32),
    Power,
    Frame,
    Row(usize),
}

/// Jeden textový vstup: poslední UTF-16 podoba (kvůli detekci změny) a dekódovaný text.
#[derive(Default)]
struct Text {
    raw: Vec<u16>,
    text: String,
    /// ZST id z `zst_map.csv` (jen pro názvy zastávek a cílů).
    mapped: String,
}

pub struct Core {
    panel: AnyPanel,
    pcfg: PluginCfg,
    log: Logger,
    var_slots: Vec<Slot>,
    str_slots: Vec<Slot>,
    sys_time: Option<u16>,
    sys_gap: Option<u16>,
    sys_last: u16,
    time: Option<f32>,
    gap: Option<f32>,
    clock: std::time::Instant,
    texts: [Text; N_ROLES],
    nums: [f32; N_ROLES],
    num_text: [String; N_ROLES],
    /// Provedení, pro které je načtená databáze.
    variant: u32,
    /// Střídání nácestných zastávek: seznam, pozice (0 = nadpis), čas na pozici, zobrazený text.
    via_list: String,
    via_pos: usize,
    via_acc: f64,
    via_text: String,
    power: f32,
    has_power: bool,
    zst_map: HashMap<String, String>,
    /// Výstupní řádky (2 na pruh: horní a spodní čtveřice), přesně `width` znaků.
    rows: Vec<Vec<u16>>,
    frame_no: f32,
    blank: bool,
    width: usize,
    warned_short: bool,
    /// Hra už aspoň jednou předala proměnné vozu (hráč sedí ve voze).
    fed: bool,
    pub ticks: u64,
    /// Složka pluginu a soubory, ze kterých se načetl (konfigurace, databáze, mapa názvů), s
    /// časem poslední změny: když se některý změní (uložení v gBUSE), plugin se načte znovu.
    dir: PathBuf,
    watched: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    checked: std::time::Instant,
}

/// Panely jednoho pluginu nad společným `.opl`: každá složka s `buse_panel.cfg` (složka
/// pluginu a její podsložky) je jeden panel. Používá je DLL (`lib.rs`) i hra, která má
/// panely vestavěné (openOMSI: `omsi-app/src/buse.rs`).
pub struct Panels {
    pub dir: PathBuf,
    pub opl: Option<Opl>,
    pub cores: Vec<Core>,
}

impl Panels {
    pub fn load(dir: &Path) -> Panels {
        let opl = Core::find_opl(dir).map(|(_, o)| o);
        Panels::load_with(dir, opl)
    }

    pub fn load_with(dir: &Path, opl: Option<Opl>) -> Panels {
        let cores = Core::panel_dirs(dir).iter().filter_map(|d| Core::load_panel(d, opl.as_ref(), 0).ok()).collect();
        Panels { dir: dir.to_path_buf(), opl, cores }
    }

    /// Jednou za snímek: uložená databáze nebo konfigurace (gBUSE, editor) se projeví bez
    /// restartu hry, panel se načte znovu; stejně tak změna provedení panelu ve voze. Když
    /// nový soubor nejde přečíst (rozepsané ukládání), zůstává ten starý.
    pub fn housekeeping(&mut self) {
        for core in &mut self.cores {
            let variant = core.wanted_variant();
            if variant.is_none() && !core.changed_on_disk() {
                continue;
            }
            let (dir, frame_no) = (core.dir().to_path_buf(), core.frame_no());
            // (Core::load_panel si chybu zapíše do logu; příště se zkusí znovu jen při další změně)
            if let Ok(mut new) = Core::load_panel(&dir, self.opl.as_ref(), variant.unwrap_or(core.variant())) {
                new.set_frame_no(frame_no);
                new.log_line(if variant.is_some() { "vůz hlásí jiné provedení panelu: načteno znovu" } else { "soubory pluginu se změnily: načteno znovu" });
                *core = new;
            }
        }
    }

    pub fn system_var(&mut self, index: u16, value: f32) {
        if index == 0 {
            self.housekeeping();
        }
        for c in &mut self.cores {
            c.system_var(index, value);
        }
    }

    /// `true`, když některý panel hodnotu přepsal (každou proměnnou píše nejvýš jeden: jeho
    /// čítač snímků).
    pub fn variable(&mut self, index: u16, value: &mut f32) -> bool {
        self.cores.iter_mut().fold(false, |w, c| c.variable(index, value) || w)
    }

    /// Vstupní string čtou všechny panely, výstupní řádek píše ten, komu patří: vrací počet
    /// zapsaných znaků a jestli panel chce zkušební přetečení (`debug_overrun`).
    pub fn string_var(&mut self, index: u16, buf: &mut [u16]) -> Option<(usize, bool)> {
        let mut out = None;
        for c in &mut self.cores {
            if let Some(n) = c.string_var(index, buf) {
                out = Some((n, c.debug_overrun()));
            }
        }
        out
    }
}

fn modified(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Core {
    /// Načte konfiguraci, `.opl`, databázi a mapu zastávek ze složky pluginu.
    pub fn load(dir: &Path) -> Result<Core, String> {
        Core::load_panel(dir, None, 0)
    }

    /// Složky panelů pluginu: složka sama (má-li `buse_panel.cfg`) a její podsložky s
    /// `buse_panel.cfg`, podle jména. Jeden plugin (jedno `.opl`) tak obslouží víc panelů.
    pub fn panel_dirs(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if dir.join(CFG_NAME).is_file() {
            out.push(dir.to_path_buf());
        }
        let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join(CFG_NAME).is_file()).collect())
            .unwrap_or_default();
        subs.sort();
        out.extend(subs);
        out
    }

    /// `.opl` pluginu: vedle DLL, nebo o složku výš (`plugins\buse_panel.opl`).
    pub fn find_opl(dir: &Path) -> Option<(PathBuf, Opl)> {
        [dir.join(OPL_NAME), dir.join("..").join(OPL_NAME)]
            .into_iter()
            .find_map(|p| std::fs::read(&p).ok().map(|b| (p, Opl::parse(&String::from_utf8_lossy(&b)))))
    }

    /// Panel ze složky `dir`; `opl` = seznamy proměnných pluginu (jinak se hledají u složky),
    /// `variant` = provedení panelu (databáze `database_N`, když ji konfigurace má).
    pub fn load_panel(dir: &Path, opl: Option<&Opl>, variant: u32) -> Result<Core, String> {
        let mut log = Logger {
            file: std::fs::File::create(dir.join(LOG_NAME)).ok(),
            lines: 0,
        };
        log.line(&format!("buse_panel {} ({}-bit), složka {}", env!("CARGO_PKG_VERSION"), usize::BITS, dir.display()));
        let res = Core::load_inner(dir, opl, variant, &mut log);
        match res {
            Ok(mut core) => {
                core.log = log;
                Ok(core)
            }
            Err(e) => {
                log.line(&format!("CHYBA: {e}"));
                Err(e)
            }
        }
    }

    fn load_inner(dir: &Path, shared: Option<&Opl>, variant: u32, log: &mut Logger) -> Result<Core, String> {
        let read = |p: &Path| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
        let cfg_path = dir.join(CFG_NAME);
        let (cfg, pcfg, warn) = match read(&cfg_path) {
            Ok(text) => parse_cfg(&text),
            Err(e) => {
                log.line(&format!("{}: {e}; použity výchozí hodnoty", cfg_path.display()));
                (Config::default(), PluginCfg::default(), Vec::new())
            }
        };
        for w in &warn {
            log.line(&format!("cfg: {w}"));
        }
        // .opl: vedle DLL, nebo o složku výš (plugins\buse_panel.opl + plugins\buse\buse_panel.dll)
        let opl = match shared {
            Some(o) => o.clone(),
            None => Core::find_opl(dir)
                .map(|(p, o)| {
                    log.line(&format!("opl: {} ({} proměnných, {} stringů, {} systémových)", p.display(), o.vars.len(), o.strings.len(), o.system.len()));
                    o
                })
                .ok_or(format!("{OPL_NAME} nenalezen ve složce pluginu ani o úroveň výš"))?,
        };
        let db_file = pcfg.database_alt.iter().find(|a| a.0 == variant).map_or(&pcfg.database, |a| &a.1);
        if variant != 0 {
            log.line(&format!("provedení {variant}: databáze {db_file}"));
        }
        let db_path = dir.join(db_file);
        let bytes = std::fs::read(&db_path).map_err(|e| format!("{}: {e}", db_path.display()))?;
        // vnitřní panel (gBUSE1) i vnější (gBUSE0): druh se pozná z databáze
        let db = AnyDb::from_bytes(&bytes).map_err(|e| format!("{}: {e}", db_path.display()))?;
        log.line(&db.describe());
        let map_name = pcfg.zst_map.clone();
        let zst_map = match read(&dir.join(&pcfg.zst_map)) {
            Ok(t) => {
                let m = parse_zst_map(&t);
                log.line(&format!("{}: {} mapovaných názvů", pcfg.zst_map, m.len()));
                m
            }
            Err(_) => {
                log.line(&format!("{}: nenalezen, názvy se párují jen přímo proti ZST", pcfg.zst_map));
                HashMap::new()
            }
        };

        // šířka `auto` = z hlavičky databáze; řádky pro skript musí mít přesně tolik znaků
        let (width, height) = db.size(&cfg);
        let strips = height.div_ceil(8);
        let mut var_slots = vec![Slot::None; opl.vars.len()];
        let mut str_slots = vec![Slot::None; opl.strings.len()];
        match find(&opl.vars, &pcfg.out_frame) {
            Some(i) => var_slots[i as usize] = Slot::Frame,
            None => log.line(&format!("opl: chybí výstupní proměnná {} ve [varlist]", pcfg.out_frame)),
        }
        for (k, name) in pcfg.row_names(strips).iter().enumerate() {
            match find(&opl.strings, name) {
                Some(i) => str_slots[i as usize] = Slot::Row(k),
                None => log.line(&format!("opl: chybí výstupní string {name} ve [stringvarlist]")),
            }
        }
        let mut bind = |src: &Option<Source>, slot: fn(f32) -> Slot, what: &str, log: &mut Logger| {
            let Some(src) = src else { return false };
            let (list, slots, div) = match src {
                Source::Str(_) => (&opl.strings, &mut str_slots, 1.0),
                Source::Num(_, d) => (&opl.vars, &mut var_slots, *d),
            };
            match find(list, src.name()) {
                Some(i) => {
                    slots[i as usize] = slot(div);
                    log.line(&format!("vstup {what} <- {}", src.name()));
                    true
                }
                None => {
                    log.line(&format!("vstup {what}: proměnná {} není v .opl, vstup se nepoužije", src.name()));
                    false
                }
            }
        };
        // role se do slotu zapisuje přes pomocné funkce (fn pointer nemůže zachytit index)
        const ROLE_SLOTS: [fn(f32) -> Slot; N_ROLES] = [
            |d| Slot::Role(0, d),
            |d| Slot::Role(1, d),
            |d| Slot::Role(2, d),
            |d| Slot::Role(3, d),
            |d| Slot::Role(4, d),
            |d| Slot::Role(5, d),
            |d| Slot::Role(6, d),
            |d| Slot::Role(7, d),
            |d| Slot::Role(8, d),
            |d| Slot::Role(9, d),
            |d| Slot::Role(10, d),
        ];
        for (i, src) in pcfg.sources.iter().enumerate() {
            bind(src, ROLE_SLOTS[i], ROLES[i], log);
        }
        let has_power = bind(&pcfg.power, |_| Slot::Power, "power", log);
        let (sys_time, sys_gap) = (find(&opl.system, "Time"), find(&opl.system, "Timegap"));
        if sys_gap.is_none() {
            log.line("opl: [systemvarlist] nemá Timegap, čas panelu poběží podle hodin systému");
        }
        let panel = AnyPanel::new(db, cfg);
        let mut core = Core {
            panel,
            log: Logger { file: None, lines: 0 },
            var_slots,
            str_slots,
            sys_time,
            sys_gap,
            sys_last: opl.system.len().saturating_sub(1) as u16,
            time: None,
            gap: None,
            clock: std::time::Instant::now(),
            texts: Default::default(),
            nums: [0.0; N_ROLES],
            num_text: Default::default(),
            variant,
            via_list: String::new(),
            via_pos: 0,
            via_acc: 0.0,
            via_text: String::new(),
            power: 1.0,
            has_power,
            zst_map,
            rows: vec![vec![b'0' as u16; width]; strips * 2],
            frame_no: 0.0,
            blank: false,
            width,
            warned_short: false,
            fed: false,
            ticks: 0,
            dir: dir.to_path_buf(),
            watched: [cfg_path.clone(), db_path.clone(), dir.join(&map_name)].into_iter().map(|p| (modified(&p), p)).map(|(m, p)| (p, m)).collect(),
            checked: std::time::Instant::now(),
            pcfg,
        };
        core.flush_engine_log();
        Ok(core)
    }

    pub fn log_line(&mut self, msg: &str) {
        self.log.line(msg);
    }

    fn flush_engine_log(&mut self) {
        if self.panel.log().has_messages() {
            for m in self.panel.log().take() {
                self.log.line(&format!("engine: {m}"));
            }
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Po novém načtení čítač snímků pokračuje, aby skript vozu poznal změnu.
    pub fn set_frame_no(&mut self, v: f32) {
        self.frame_no = v;
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn variant(&self) -> u32 {
        self.variant
    }

    /// Provedení, které vůz hlásí (`in_variant`), když pro něj nebo pro to načtené má
    /// konfigurace vlastní databázi a liší se od načteného.
    pub fn wanted_variant(&self) -> Option<u32> {
        self.pcfg.sources[R_VARIANT].as_ref()?;
        if !self.fed {
            return None;
        }
        let v = self.nums[R_VARIANT].max(0.0) as u32;
        let has = |n: u32| self.pcfg.database_alt.iter().any(|a| a.0 == n);
        (v != self.variant && (has(v) || has(self.variant))).then_some(v)
    }

    /// Posune střídání nácestných zastávek o `dt` sekund: nadpis, zastávky po jedné, mezera.
    fn step_via(&mut self, dt: f64) {
        if self.pcfg.sources[R_VIA].is_none() {
            return;
        }
        let list = &self.texts[R_VIA].text;
        if *list != self.via_list {
            self.via_list.clone_from(list);
            self.via_pos = 0;
            self.via_acc = 0.0;
        } else {
            self.via_acc += dt * 1000.0;
            if self.via_acc >= self.pcfg.via_step_ms {
                self.via_acc = 0.0;
                self.via_pos += 1;
            }
        }
        let items: Vec<&str> = self.via_list.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
        let head = usize::from(!self.pcfg.via_header.is_empty());
        // pozice: [nadpis] zastávky… a jedna prázdná, pak znovu
        if self.via_pos > head + items.len() {
            self.via_pos = 0;
        }
        let now = if items.is_empty() {
            ""
        } else if self.via_pos < head {
            self.pcfg.via_header.as_str()
        } else {
            items.get(self.via_pos - head).copied().unwrap_or("")
        };
        if now != self.via_text {
            self.via_text.clear();
            self.via_text.push_str(now);
        }
    }

    /// Změnil se na disku některý ze souborů pluginu? Dívá se nejvýš jednou za dvě sekundy.
    pub fn changed_on_disk(&mut self) -> bool {
        if self.checked.elapsed().as_secs_f32() < 2.0 {
            return false;
        }
        self.checked = std::time::Instant::now();
        self.watched.iter().any(|(p, was)| modified(p) != *was)
    }

    pub fn frame_no(&self) -> f32 {
        self.frame_no
    }

    /// Aktuální výstupní řádek `k` (0 = r0_hi, 1 = r0_lo, 2 = r1_hi, ...).
    pub fn row(&self, k: usize) -> String {
        String::from_utf16_lossy(&self.rows[k])
    }

    /// `AccessSystemVariable`: uloží čas; poslední systémová proměnná snímku spustí `tick`.
    pub fn system_var(&mut self, index: u16, value: f32) {
        if Some(index) == self.sys_time {
            self.time = Some(value);
        }
        if Some(index) == self.sys_gap {
            self.gap = Some(value);
        }
        if index == self.sys_last {
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.ticks += 1;
        if !self.fed {
            // systémové proměnné chodí před proměnnými vozu: první snímek ještě nejsou vstupy
            return;
        }
        let dt = match self.gap {
            Some(g) => g as f64,
            None => {
                let now = std::time::Instant::now();
                let dt = now.duration_since(self.clock).as_secs_f64();
                self.clock = now;
                dt
            }
        };
        let blank = self.has_power && self.power < 0.5;
        self.step_via(dt);
        let via = self.pcfg.sources[R_VIA].is_some();
        let t = &self.texts;
        let text = |role: usize| -> &str {
            if t[role].text.is_empty() { &self.num_text[role] } else { &t[role].text }
        };
        fn pick<'a>(own: &'a str, fixed: &'a str) -> &'a str {
            if own.is_empty() {
                fixed
            } else {
                own
            }
        }
        let inp = Inputs {
            line: text(R_LINE),
            dest: StopRef {
                id: pick(&t[R_DEST_NAME].mapped, &self.num_text[R_DEST_CODE]),
                name: &t[R_DEST_NAME].text,
            },
            // s `in_via` nese pole zastávky právě střídanou nácestnou zastávku
            next_stop: StopRef {
                id: if via { "" } else { pick(&t[R_STOP_NAME].mapped, &self.num_text[R_STOP_CODE]) },
                name: if via { &self.via_text } else { &t[R_STOP_NAME].text },
            },
            stop_pressed: self.nums[R_PRESSED] > 0.5,
            request_stop: self.pcfg.sources[R_REQUEST].as_ref().map(|_| self.nums[R_REQUEST] > 0.5),
            time_s: self.time.map(|t| t.max(0.0) as u32),
            zone: pick(text(R_ZONE), &self.pcfg.zone_text),
            info: pick(text(R_INFO), &self.pcfg.info_text),
        };
        let mut changed = blank != self.blank;
        self.blank = blank;
        if self.panel.tick(dt, &inp).is_some() {
            changed = true;
        }
        if changed {
            let frame = self.panel.frame();
            for (k, row) in self.rows.iter_mut().enumerate() {
                if blank {
                    row.fill(b'0' as u16);
                } else {
                    frame.nibble_row_utf16(k / 2, k % 2 == 0, row);
                }
            }
            self.frame_no += 1.0;
            if self.frame_no > 1.0e6 {
                self.frame_no = 1.0;
            }
        }
        self.flush_engine_log();
    }

    /// `AccessVariable`: vrací `true`, když hodnotu přepsal.
    pub fn variable(&mut self, index: u16, value: &mut f32) -> bool {
        self.fed = true;
        match self.var_slots.get(index as usize).copied().unwrap_or(Slot::None) {
            Slot::Frame => {
                if *value != self.frame_no {
                    *value = self.frame_no;
                    return true;
                }
            }
            Slot::Power => self.power = *value,
            Slot::Role(role, div) => {
                let v = *value / div;
                if v != self.nums[role] || self.num_text[role].is_empty() {
                    self.nums[role] = v;
                    // číselný vstup jako text (linka, kód cíle / zastávky); 0 = nic
                    use std::fmt::Write;
                    self.num_text[role].clear();
                    if v >= 1.0 {
                        let _ = write!(self.num_text[role], "{}", v as u32);
                    }
                }
            }
            _ => {}
        }
        false
    }

    /// `AccessStringVariable`: `buf` = text bez ukončovací nuly (hra dává buffer délka + 1).
    /// Vrací počet zapsaných znaků, když text přepsal.
    pub fn string_var(&mut self, index: u16, buf: &mut [u16]) -> Option<usize> {
        match self.str_slots.get(index as usize).copied().unwrap_or(Slot::None) {
            Slot::Row(k) => {
                let want = &self.rows[k];
                if buf.len() < want.len() {
                    if !self.warned_short {
                        self.warned_short = true;
                        let msg = format!(
                            "string proměnná řádku {k} má {} znaků místo {}: skript vozu ji v init nenaplnil; zapisuji jen co se vejde",
                            buf.len(),
                            want.len()
                        );
                        self.log.line(&msg);
                    }
                    let n = buf.len();
                    if buf[..] != want[..n] {
                        buf.copy_from_slice(&want[..n]);
                        return Some(n);
                    }
                    return None;
                }
                if buf[..want.len()] != want[..] || buf.len() != want.len() {
                    buf[..want.len()].copy_from_slice(want);
                    return Some(want.len());
                }
                None
            }
            Slot::Role(role, _) => {
                let t = &mut self.texts[role];
                if t.raw != buf {
                    t.raw.clear();
                    t.raw.extend_from_slice(buf);
                    let s = String::from_utf16_lossy(buf);
                    let s = if self.pcfg.fix_encoding { fix_mojibake(s.trim()).into_owned() } else { s.trim().to_string() };
                    t.mapped.clear();
                    if role == R_DEST_NAME || role == R_STOP_NAME {
                        if let Some(id) = self.zst_map.get(&normalize(&s)) {
                            t.mapped.push_str(id);
                        }
                    }
                    t.text = s;
                }
                None
            }
            _ => None,
        }
    }

    pub fn debug_overrun(&self) -> bool {
        self.pcfg.debug_overrun
    }
}
