//! Engine vnitřního LED panelu BUSE BS120 / BS190 napájený databází z gBUSE1.
//!
//! Čistá knihovna bez I/O; čas se předává zvenku (`Panel::tick(dt, ..)`), takže je
//! deterministická. Formát databáze a úroveň jistoty jednotlivých částí: `PROMPT.md`, oddíl 2.

pub mod any;
pub mod config;
pub mod db;
pub mod hex;
pub mod names;
pub mod outer;
pub mod panel;
pub mod text;

pub use any::{AnyDb, AnyPanel};
pub use config::{Align, Config, Field, PageSpec, Slide, Var, MAX_FIELDS};
pub use db::{Cycle, CyclePage, Db, DbError, Font, Glyph, Record, Section};
pub use outer::{OuterConfig, OuterDb, OuterPanel};
pub use panel::{Frame, Inputs, Panel, StopRef};
pub use text::{render_text, Log, MissingGlyph, PictAlias, Placeholder, RenderOpts};
