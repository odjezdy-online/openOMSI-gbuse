//! Which vehicle a player drives, independent of where it lies: the fingerprint of its
//! `.bus` (or `.ovh`) file. Two players with the same bus in differently named folders
//! (`Vehicles/MAN_NewLionsCity` and `Vehicles/MAN NLC 2.1`) have the same file, so the game
//! finds the bus of another player under its own name instead of showing a stand-in.
//!
//! The fingerprint is the first 16 hex digits of the file's SHA-256. The vehicles of this
//! machine are fingerprinted in the background when a LAN session starts: every `.bus` and
//! `.ovh` of every content root's `Vehicles` folder (a big installation, 6000 files, takes
//! half a minute). Until that is done a bus not found by its path shows a stand-in; the
//! game tries again after a while.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The fingerprint of a vehicle file's contents (16 hex digits).
pub fn of_bytes(data: &[u8]) -> String {
    let h = Sha256::digest(data);
    h.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The fingerprint of the vehicle file at `path` (cached; empty when it cannot be read).
pub fn of_file(path: &Path) -> String {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(f) = cache.lock().ok().and_then(|c| c.get(path).cloned()) {
        return f;
    }
    let f = omsi_cfg::vfs::read(path).map(|d| of_bytes(&d)).unwrap_or_default();
    if let Ok(mut c) = cache.lock() {
        c.insert(path.to_path_buf(), f.clone());
    }
    f
}

/// Is `s` a fingerprint as `of_bytes` writes it?
pub fn is_fingerprint(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Start fingerprinting this machine's vehicles in the background (a LAN session calls it
/// when it starts: a big installation takes half a minute, which the game must not wait).
pub fn warm_up(root: &Path) {
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let _ = INDEX.set(build_index(&root));
    });
}

/// The vehicle file of this machine with that fingerprint. None as well while the
/// fingerprints are still being taken (the caller tries again later).
pub fn find(fingerprint: &str, root: &Path) -> Option<PathBuf> {
    if !is_fingerprint(fingerprint) {
        return None;
    }
    let Some(index) = INDEX.get() else {
        warm_up(root);
        return None;
    };
    index.get(&fingerprint.to_ascii_lowercase()).cloned()
}

fn build_index(root: &Path) -> HashMap<String, PathBuf> {
    let t0 = std::time::Instant::now();
    let mut roots = omsi_cfg::content_roots();
    if !roots.iter().any(|r| r == root) {
        roots.push(root.to_path_buf());
    }
    let mut index = HashMap::new();
    for r in roots {
        let Some(vehicles) = omsi_cfg::find_in_roots("Vehicles").filter(|(rr, _)| *rr == r).map(|(_, p)| p).or_else(|| Some(r.join("Vehicles"))) else { continue };
        // Vehicles/<folder>/<file>.bus (and one level deeper, as some packs have it)
        let mut dirs = vec![(vehicles, 0)];
        while let Some((dir, depth)) = dirs.pop() {
            for (name, is_dir) in omsi_cfg::vfs::list_dir(&dir).unwrap_or_default() {
                let p = dir.join(&name);
                if is_dir {
                    if depth < 2 {
                        dirs.push((p, depth + 1));
                    }
                    continue;
                }
                let lower = name.to_string_lossy().to_ascii_lowercase();
                if !(lower.ends_with(".bus") || lower.ends_with(".ovh")) {
                    continue;
                }
                let f = of_file(&p);
                if !f.is_empty() {
                    index.entry(f).or_insert(p);
                }
            }
        }
    }
    log::info!("vehicles fingerprinted: {} files in {:.1} s", index.len(), t0.elapsed().as_secs_f64());
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_is_sixteen_hex_digits_of_the_contents() {
        let a = of_bytes(b"[friendlyname]\r\nSOR\r\nNB 12\r\n");
        assert!(is_fingerprint(&a));
        assert_eq!(a, of_bytes(b"[friendlyname]\r\nSOR\r\nNB 12\r\n"));
        assert_ne!(a, of_bytes(b"[friendlyname]\r\nSOR\r\nNB 18\r\n"));
        assert!(!is_fingerprint("Vehicles/x.bus"));
    }
}
