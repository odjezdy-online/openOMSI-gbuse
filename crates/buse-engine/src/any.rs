//! Jedno rozhraní pro oba druhy panelů: vnitřní LED (databáze gBUSE1) a vnější (gBUSE0).
//! Druh se pozná z prvního bajtu obrazu databáze: `AA` / `AB` = gBUSE1, jinak gBUSE0.

use crate::config::Config;
use crate::db::{Db, DbError, BIN_MAGIC};
use crate::hex::parse_intel_hex;
use crate::outer::{OuterConfig, OuterDb, OuterPanel};
use crate::panel::{Frame, Inputs, Panel};
use crate::text::Log;

#[derive(Debug, Clone)]
pub enum AnyDb {
    Inner(Db),
    Outer(OuterDb),
}

impl AnyDb {
    /// Obsah souboru databáze: Intel HEX z gBUSE0 / gBUSE1, nebo `buse_db.bin` (jen vnitřní).
    pub fn from_bytes(bytes: &[u8]) -> Result<AnyDb, DbError> {
        if bytes.starts_with(BIN_MAGIC) {
            return Db::from_bin(bytes).map(AnyDb::Inner);
        }
        let image = parse_intel_hex(&String::from_utf8_lossy(bytes))?;
        match image.first() {
            Some(0xAA | 0xAB) => Db::from_image(image).map(AnyDb::Inner),
            _ => OuterDb::from_image(image).map(AnyDb::Outer),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            AnyDb::Inner(d) => d.describe(),
            AnyDb::Outer(d) => d.describe(),
        }
    }

    pub fn is_outer(&self) -> bool {
        matches!(self, AnyDb::Outer(_))
    }

    /// Rozměr panelu `(sloupce, řádky)` pro danou konfiguraci (šířka `auto` z databáze).
    pub fn size(&self, cfg: &Config) -> (usize, usize) {
        match self {
            AnyDb::Inner(d) => {
                let mut c = cfg.clone();
                c.bind(d);
                (c.width, c.rows * 8)
            }
            AnyDb::Outer(d) => (if cfg.width == 0 { d.width } else { cfg.width.min(d.width) }, d.rows),
        }
    }
}

pub enum AnyPanel {
    Inner(Box<Panel>),
    Outer(Box<OuterPanel>),
}

impl AnyPanel {
    pub fn new(db: AnyDb, cfg: Config) -> AnyPanel {
        match db {
            AnyDb::Inner(d) => AnyPanel::Inner(Box::new(Panel::new(d, cfg))),
            AnyDb::Outer(d) => {
                let ocfg = OuterConfig { step_ms: cfg.outer_step_ms, stop_dashes: cfg.outer_stop_dashes, width: cfg.width, expand_dest: cfg.outer_expand_dest, ..OuterConfig::default() };
                AnyPanel::Outer(Box::new(OuterPanel::new(d, ocfg)))
            }
        }
    }

    pub fn tick(&mut self, dt: f64, inp: &Inputs) -> Option<&Frame> {
        match self {
            AnyPanel::Inner(p) => p.tick(dt, inp),
            AnyPanel::Outer(p) => p.tick(dt, inp),
        }
    }

    pub fn frame(&self) -> &Frame {
        match self {
            AnyPanel::Inner(p) => p.frame(),
            AnyPanel::Outer(p) => p.frame(),
        }
    }

    pub fn frame_no(&self) -> u32 {
        match self {
            AnyPanel::Inner(p) => p.frame_no(),
            AnyPanel::Outer(p) => p.frame_no(),
        }
    }

    pub fn log(&mut self) -> &mut Log {
        match self {
            AnyPanel::Inner(p) => p.log(),
            AnyPanel::Outer(p) => p.log(),
        }
    }

    /// Šířka snímku ve sloupcích a počet osmiřádkových pruhů.
    pub fn size(&self) -> (usize, usize) {
        let f = self.frame();
        (f.width, f.strips.len())
    }
}
