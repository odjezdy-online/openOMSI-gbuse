//! buse-sim: simulace panelu v čase, vypisuje ASCII snímky při každé změně.
//!
//!   buse-sim data/ADledA.hex --line 105 --dest 1146 --stop 1494 --time 12:34 --duration 20
//!   buse-sim data/ADledA.hex --line 1 --dest-name "Hlavní nádraží" --at 5 stop=1163 --at 8 press=1 --at 12 press=0
//!   buse-sim data/ADledA.hex --set rows=2 --set use_cyk=false --cfg buse_panel.cfg
//!
//! Události `--at T klíč=hodnota`: line, dest, dest_name, stop, stop_name, press, request, zone, info.

use buse_engine::{Config, Inputs, Panel, StopRef};
use buse_tools::load_db;
use std::process::ExitCode;

const USAGE: &str = "použití: buse-sim <db.hex> [--line N] [--dest ID] [--dest-name S] [--stop ID] [--stop-name S] \
[--time HH:MM] [--zone S] [--info S] [--duration SEKUNDY] [--step MS] [--cfg FILE] [--set klíč=hodnota]... \
[--at T klíč=hodnota]... [--quiet] [--nibbles]";

#[derive(Default, Clone)]
struct State {
    line: String,
    dest: String,
    dest_name: String,
    stop: String,
    stop_name: String,
    zone: String,
    info: String,
    press: bool,
    request: Option<bool>,
}

impl State {
    fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        match key {
            "line" => self.line = v.into(),
            "dest" => self.dest = v.into(),
            "dest_name" => self.dest_name = v.into(),
            "stop" => self.stop = v.into(),
            "stop_name" => self.stop_name = v.into(),
            "zone" => self.zone = v.into(),
            "info" => self.info = v.into(),
            "press" => self.press = v == "1" || v == "true",
            "request" => self.request = if v.is_empty() { None } else { Some(v == "1" || v == "true") },
            _ => return Err(format!("neznámý vstup '{key}'")),
        }
        Ok(())
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let (mut file, mut cfg, mut st) = (None, Config::default(), State::default());
    let (mut duration, mut step, mut time, mut quiet, mut nibbles) = (20.0f64, 10.0f64, Some(12 * 3600 + 34 * 60), false, false);
    let mut events: Vec<(f64, String, String)> = Vec::new();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a}: chybí hodnota\n{USAGE}"));
        match a.as_str() {
            "--line" | "--dest" | "--dest-name" | "--stop" | "--stop-name" | "--zone" | "--info" => {
                st.set(&a[2..].replace('-', "_"), &val()?)?
            }
            "--time" => {
                let v = val()?;
                time = match v.split_once(':') {
                    Some((h, m)) => Some(
                        h.parse::<u32>().map_err(|_| "--time HH:MM")? * 3600
                            + m.parse::<u32>().map_err(|_| "--time HH:MM")? * 60,
                    ),
                    None => None,
                };
            }
            "--duration" => duration = val()?.parse().map_err(|_| "--duration: číslo")?,
            "--step" => step = val()?.parse().map_err(|_| "--step: číslo")?,
            "--cfg" => {
                let p = val()?;
                let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
                let (c, warn, _) = Config::parse(&text);
                cfg = c;
                warn.iter().for_each(|w| eprintln!("[cfg] {w}"));
            }
            "--set" => {
                let v = val()?;
                let (k, v) = v.split_once('=').ok_or("--set klíč=hodnota")?;
                if !cfg.set(k.trim(), v.trim())? {
                    return Err(format!("--set: neznámý klíč '{k}'"));
                }
            }
            "--at" => {
                let t: f64 = val()?.parse().map_err(|_| "--at T klíč=hodnota")?;
                let v = val()?;
                let (k, v) = v.split_once('=').ok_or("--at T klíč=hodnota")?;
                events.push((t, k.to_string(), v.to_string()));
            }
            "--quiet" => quiet = true,
            "--nibbles" => nibbles = true,
            "-h" | "--help" => return Err(USAGE.into()),
            _ if file.is_none() => file = Some(a.clone()),
            _ => return Err(format!("neznámý argument {a}\n{USAGE}")),
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let db = load_db(&file.ok_or(USAGE)?)?;
    println!("{}", db.describe());
    let mut panel = Panel::new(db, cfg);
    let (mut t, mut frames, mut next_event) = (0.0f64, 0u32, 0usize);
    let steps = (duration * 1000.0 / step).round() as u64;
    for i in 0..=steps {
        while next_event < events.len() && events[next_event].0 <= t + 1e-9 {
            let (et, k, v) = &events[next_event];
            println!("--- t={et:.3}s událost {k}={v}");
            st.set(k, v)?;
            next_event += 1;
        }
        let inp = Inputs {
            line: &st.line,
            dest: StopRef { id: &st.dest, name: &st.dest_name },
            next_stop: StopRef { id: &st.stop, name: &st.stop_name },
            stop_pressed: st.press,
            request_stop: st.request,
            time_s: time.map(|s| s + t as u32),
            zone: &st.zone,
            info: &st.info,
        };
        let dt = if i == 0 { 0.0 } else { step / 1000.0 };
        if let Some(frame) = panel.tick(dt, &inp) {
            frames += 1;
            if !quiet {
                println!("t={t:8.3}s  snímek {frames}");
                println!("{}", frame.ascii());
                if nibbles {
                    for s in 0..frame.strips.len() {
                        println!("r{s}_hi {}\nr{s}_lo {}", frame.nibble_row(s, true), frame.nibble_row(s, false));
                    }
                }
            }
        }
        for m in panel.log().take() {
            println!("[engine] {m}");
        }
        t += step / 1000.0;
    }
    println!("konec: {duration} s, {frames} změněných snímků z {} kroků", steps + 1);
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
