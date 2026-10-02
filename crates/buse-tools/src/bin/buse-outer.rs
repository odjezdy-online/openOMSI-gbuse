//! Náhled vnějšího panelu (čelní, boční, zadní) z databáze gBUSE0 v terminálu.
//!
//!   buse-outer ADcel0.hex --line 53 --dest 105
//!   buse-outer ADbok4.hex --line 53 --dest-name "Hlavní nádraží" --stop 1001
//!   buse-outer ADcel0.hex --dest 982 --duration 6       (animované texty: kroky v čase)
//!   buse-outer ADcel0.hex --list cil                    (výpis tabulky)

use buse_engine::{Inputs, OuterConfig, OuterDb, OuterPanel, StopRef};

const USAGE: &str = "použití: buse-outer <db.hex> [--line N] [--dest ID] [--dest-name S] [--stop ID] [--stop-name S] \
[--duration SEKUNDY] [--step-ms MS] [--list lin|cil|dru] [--info]";

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let (mut file, mut list, mut info, mut duration) = (None, None, false, 0.0f64);
    let (mut line, mut dest, mut dest_name, mut stop, mut stop_name) = (String::new(), String::new(), String::new(), String::new(), String::new());
    let mut cfg = OuterConfig::default();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a}: chybí hodnota\n{USAGE}"));
        match a.as_str() {
            "--line" => line = val()?,
            "--dest" => dest = val()?,
            "--dest-name" => dest_name = val()?,
            "--stop" => stop = val()?,
            "--stop-name" => stop_name = val()?,
            "--duration" => duration = val()?.parse().map_err(|_| "--duration: číslo")?,
            "--step-ms" => cfg.step_ms = val()?.parse().map_err(|_| "--step-ms: číslo")?,
            "--list" => list = Some(val()?),
            "--info" => info = true,
            _ if file.is_none() => file = Some(a.clone()),
            _ => return Err(format!("neznámý parametr {a}\n{USAGE}")),
        }
    }
    let file = file.ok_or(USAGE)?;
    let bytes = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
    let db = OuterDb::from_hex(&String::from_utf8_lossy(&bytes)).map_err(|e| format!("{file}: {e}"))?;
    println!("{}", db.describe());
    if info {
        for (name, f) in ["linka", "cíl", "zastávka"].iter().zip(db.formats) {
            println!("formát pole {name}: font E{:X}, mezera {}, řádek {}, druhý řádek {}", f.font & 0xF, f.spacing, f.y, f.y2);
        }
    }
    if let Some(tab) = list {
        let rows = match tab.as_str() {
            "lin" => &db.lin,
            "cil" => &db.cil,
            "dru" => &db.dru,
            _ => return Err(format!("--list: lin | cil | dru\n{USAGE}")),
        };
        for r in rows {
            let hex: Vec<String> = r.raw.iter().map(|b| format!("{b:02x}")).collect();
            println!("{} {}", r.id, hex.join(" "));
        }
        return Ok(());
    }
    let inp = Inputs {
        line: &line,
        dest: StopRef { id: &dest, name: &dest_name },
        next_stop: StopRef { id: &stop, name: &stop_name },
        ..Inputs::default()
    };
    let mut panel = OuterPanel::new(db, cfg);
    panel.tick(0.0, &inp);
    println!("t=0.000s\n{}", panel.ascii());
    let mut t = 0.0;
    while t < duration {
        t += 0.05;
        if panel.tick(0.05, &inp).is_some() {
            println!("t={t:.3}s\n{}", panel.ascii());
        }
    }
    for m in panel.log().take() {
        eprintln!("[engine] {m}");
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
