//! The BUSE panels (inner LED panel, front, side and rear panels fed by gBUSE databases)
//! built into the game: what the `buse_panel` plugin does for OMSI 2 through its DLL, the
//! game does itself, on every platform and with no library to load.
//!
//! The package is the plugin's own: a folder under `plugins` with a `buse_panel.cfg` (and
//! one more in each subfolder that is a panel of its own), the databases beside them, and
//! the plugin's `.opl` with the lists of the bus's variables. The `.opl` is found by its
//! `[dll]`, whose file name begins with `buse_panel`; that plugin is then not loaded as a
//! library (see `plugins::load`), the panels of its folder are driven here instead - with
//! the same engine (`buse-engine`) and the same reading of the files (`buse_panel::core`).
//! A database or configuration saved while the game runs is taken up within two seconds.

use buse_panel::core::{Opl, Panels};
use omsi_plugin::PluginIo;
use std::path::{Path, PathBuf};

/// Is this the `.opl` of the BUSE panel plugin (by the library it names)?
pub(crate) fn is_buse_dll(dll: &str) -> bool {
    dll.replace('\\', "/").rsplit('/').next().is_some_and(|f| f.to_ascii_lowercase().starts_with("buse_panel"))
}

/// The panel folders of the content roots' `plugins` folders.
pub(crate) struct Buse {
    sets: Vec<Panels>,
}

impl Buse {
    /// Every folder a BUSE `.opl` under `dirs` points to, once (the 32-bit and the 64-bit
    /// library of one folder have an `.opl` each, with the same lists).
    pub(crate) fn load(dirs: &[PathBuf]) -> Buse {
        let mut sets: Vec<Panels> = Vec::new();
        for dir in dirs {
            for path in omsi_plugin::find_opls(dir) {
                let Ok(bytes) = std::fs::read(&path) else { continue };
                let opl = Opl::parse(&String::from_utf8_lossy(&bytes));
                if !is_buse_dll(&opl.dll) {
                    continue;
                }
                // the library's folder: relative to `plugins`, as OMSI reads `[dll]`
                let Some(folder) = omsi_plugin::resolve_path(dir, &opl.dll).and_then(|p| p.parent().map(Path::to_path_buf)).or_else(|| {
                    // (a package without the library: the folder the path names)
                    let rel = opl.dll.replace('\\', "/");
                    let rel = rel.rsplit_once('/').map(|r| r.0).unwrap_or("");
                    omsi_plugin::resolve_path(dir, rel).filter(|p| p.is_dir())
                }) else {
                    continue;
                };
                if sets.iter().any(|s| s.dir == folder) {
                    continue;
                }
                let set = Panels::load_with(&folder, Some(opl));
                if set.cores.is_empty() {
                    log::warn!("BUSE panels: {} has no panel that could be loaded (see buse_panel.log there)", folder.display());
                    continue;
                }
                log::info!("BUSE panels: {} panel(s) of {} are drawn by the game itself", set.cores.len(), folder.display());
                sets.push(set);
            }
        }
        Buse { sets }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// One frame, as OMSI drives a plugin: the system variables, then the bus's variables
    /// and its string variables, each by its place in the `.opl`'s list.
    pub(crate) fn frame(&mut self, io: &mut dyn PluginIo) {
        for set in &mut self.sets {
            let Some(opl) = set.opl.clone() else { continue };
            for (i, name) in opl.system.iter().enumerate() {
                if let Some(v) = io.system(name) {
                    set.system_var(i as u16, v);
                }
            }
            if !io.has_vehicle() {
                continue;
            }
            for (i, name) in opl.vars.iter().enumerate() {
                let Some(was) = io.var(name) else { continue };
                let mut v = was;
                if set.variable(i as u16, &mut v) {
                    io.set_var(name, v);
                }
            }
            for (i, name) in opl.strings.iter().enumerate() {
                let Some(text) = io.string(name) else { continue };
                let mut buf: Vec<u16> = text.encode_utf16().collect();
                if let Some((n, _)) = set.string_var(i as u16, &mut buf) {
                    io.set_string(name, &String::from_utf16_lossy(&buf[..n.min(buf.len())]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugins_opl_is_known_by_its_library() {
        assert!(is_buse_dll("buse\\buse_panel.dll"));
        assert!(is_buse_dll("buse/buse_panel_x64.dll"));
        assert!(is_buse_dll("BUSE_PANEL.DLL"));
        assert!(!is_buse_dll("AUXI/AUXI.dll"));
        assert!(!is_buse_dll("buse_panel/other.dll"));
    }
}
