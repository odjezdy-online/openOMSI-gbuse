//! OMSI 2 / openOMSI plugin (`.opl` + DLL) panelů BUSE: vnitřní LED panel i vnější panely.
//!
//! Rozhraní podle `docs/PLUGINS.md` z openOMSI (popis originálu): exporty `PluginStart`,
//! `PluginFinalize`, `AccessVariable`, `AccessStringVariable`, `AccessSystemVariable`,
//! `AccessTrigger`, všechny `stdcall` s nedekorovanými jmény. Hra je volá každý snímek v pořadí
//! systémové proměnné -> proměnné vozu -> string proměnné -> triggery.
//!
//! Plugin čte vstupy (linka, cíl, zastávka, STOP, čas) a zapisuje snímek panelu jako
//! „nibble rows" do string proměnných; skript vozu je přenese do skriptové textury.
//!
//! Jeden plugin obslouží víc panelů: každá složka s `buse_panel.cfg` (složka pluginu a její
//! podsložky) je jeden panel, všechny sdílejí seznamy proměnných jednoho `.opl`.
//!
//! Ve složce pluginu leží 32bitová i 64bitová knihovna, každá se svým `.opl`. Hra načte tu,
//! kterou umí (OMSI 2 32bitovou, openOMSI na 64bitových Windows 64bitovou). Když běží obě
//! (openOMSI s `omsi-plugin-host32.exe`), 32bitová to pozná podle pojmenovaného mutexu
//! 64bitové a nedělá nic.

#![allow(unknown_lints, linker_messages)]

pub mod core;

use crate::core::Panels;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Panely pluginu (viz `core::Panels`) a jestli je kreslí tahle knihovna.
struct Hub {
    panels: Panels,
    /// Běží i 64bitová knihovna téže složky: tahle (32bitová) nic nedělá.
    idle: bool,
    checked: std::time::Instant,
}

static STATE: Mutex<Option<Hub>> = Mutex::new(None);

fn with_hub<R>(f: impl FnOnce(&mut Hub) -> R) -> Option<R> {
    // žádný panic nesmí přes hranici FFI; otrávený zámek = plugin se pro zbytek běhu vypne
    catch_unwind(AssertUnwindSafe(|| {
        let mut guard = STATE.lock().ok()?;
        guard.as_mut().filter(|h| !h.idle).map(f)
    }))
    .ok()
    .flatten()
}

#[cfg(windows)]
fn module_dir() -> Option<PathBuf> {
    extern "system" {
        fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }
    const FROM_ADDRESS: u32 = 4;
    const UNCHANGED_REFCOUNT: u32 = 2;
    let mut module = std::ptr::null_mut();
    let mut buf = [0u16; 1024];
    // SAFETY: adresa je funkce v tomto modulu; buffer má uvedenou velikost.
    let n = unsafe {
        if GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, module_dir as *const c_void, &mut module) == 0 {
            return None;
        }
        GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize
    };
    if n == 0 || n >= buf.len() {
        return None;
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..n])).parent().map(PathBuf::from)
}

#[cfg(not(windows))]
fn module_dir() -> Option<PathBuf> {
    None
}

/// Složka pluginu: `BUSE_PANEL_DIR`, jinak složka DLL, jinak `plugins/buse`.
fn plugin_dir() -> PathBuf {
    std::env::var_os("BUSE_PANEL_DIR")
        .map(PathBuf::from)
        .or_else(module_dir)
        .unwrap_or_else(|| PathBuf::from("plugins").join("buse"))
}

/// Jméno mutexu, kterým 64bitová knihovna dává vědět, že panely složky `dir` kreslí ona.
#[cfg(windows)]
fn mutex_name(dir: &Path) -> Vec<u16> {
    // (FNV-1a cesty malými písmeny: stejné pro obě knihovny v téže složce)
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in dir.to_string_lossy().to_lowercase().replace('/', "\\").trim_end_matches('\\').bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("Local\\buse_panel_x64_{h:016x}\0").encode_utf16().collect()
}

/// 64bitová knihovna: založí mutex (drží ho do konce procesu).
#[cfg(all(windows, target_pointer_width = "64"))]
fn announce(dir: &Path) {
    extern "system" {
        fn CreateMutexW(attrs: *const c_void, owner: i32, name: *const u16) -> *mut c_void;
    }
    let name = mutex_name(dir);
    // SAFETY: jméno je ukončené nulou; vrácený handle se schválně nezavírá.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
    }
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
fn announce(_dir: &Path) {}

/// 32bitová knihovna: běží ve stejné relaci i 64bitová knihovna téže složky?
#[cfg(all(windows, target_pointer_width = "32"))]
fn superseded(dir: &Path) -> bool {
    extern "system" {
        fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let name = mutex_name(dir);
    // SAFETY: jméno je ukončené nulou; otevřený handle se hned zavře.
    unsafe {
        let h = OpenMutexW(SYNCHRONIZE, 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        CloseHandle(h);
        true
    }
}

#[cfg(not(all(windows, target_pointer_width = "32")))]
fn superseded(_dir: &Path) -> bool {
    false
}

impl Hub {
    fn load(dir: &Path) -> Hub {
        Hub { panels: Panels::load(dir), idle: false, checked: std::time::Instant::now() }
    }

    /// Každé dvě sekundy: neběží i 64bitová knihovna téže složky?
    fn check_superseded(&mut self) {
        if self.checked.elapsed().as_secs_f32() < 2.0 {
            return;
        }
        self.checked = std::time::Instant::now();
        if superseded(&self.panels.dir) {
            self.idle = true;
            for core in &mut self.panels.cores {
                core.log_line("panely kreslí 64bitová knihovna pluginu; tahle (32bitová) nedělá nic");
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn PluginStart(_owner: *mut c_void) {
    let _ = catch_unwind(|| {
        let dir = plugin_dir();
        announce(&dir);
        let hub = Hub::load(&dir);
        if let Ok(mut guard) = STATE.lock() {
            *guard = Some(hub);
        }
    });
}

#[no_mangle]
pub extern "system" fn PluginFinalize() {
    let _ = catch_unwind(|| {
        if let Ok(mut guard) = STATE.lock() {
            if let Some(hub) = guard.as_mut() {
                for core in &mut hub.panels.cores {
                    let msg = format!("PluginFinalize: {} snímků hry, {} snímků panelu", core.ticks, core.frame_no());
                    core.log_line(&msg);
                }
            }
            *guard = None;
        }
    });
}

/// # Safety
/// `value` a `write` ukazují na Single a Boolean, jak je předává OMSI.
#[no_mangle]
pub unsafe extern "system" fn AccessVariable(index: u16, value: *mut f32, write: *mut u8) {
    if value.is_null() || write.is_null() {
        return;
    }
    let mut v = *value;
    // (každou proměnnou píše nejvýš jeden panel: jeho čítač snímků)
    let wrote = with_hub(|h| h.panels.variable(index, &mut v));
    if wrote == Some(true) {
        *value = v;
        *write = 1;
    }
}

/// # Safety
/// Jako `AccessVariable`. Zápisy do systémových proměnných plugin nedělá.
#[no_mangle]
pub unsafe extern "system" fn AccessSystemVariable(index: u16, value: *mut f32, write: *mut u8) {
    if value.is_null() {
        return;
    }
    let v = *value;
    with_hub(|h| {
        if index == 0 {
            h.check_superseded();
        }
        if !h.idle {
            h.panels.system_var(index, v);
        }
    });
    if !write.is_null() {
        *write = 0;
    }
}

/// # Safety
/// `text` je buffer o délce textu + 1 wide znaků (text a ukončovací nula).
#[no_mangle]
pub unsafe extern "system" fn AccessStringVariable(index: u16, text: *mut u16, write: *mut u8) {
    if text.is_null() || write.is_null() {
        return;
    }
    let mut len = 0;
    while *text.add(len) != 0 && len < 65_535 {
        len += 1;
    }
    let buf = std::slice::from_raw_parts_mut(text, len);
    // vstupní string čtou všechny panely, výstupní řádek píše ten, komu patří
    let res = with_hub(|h| h.panels.string_var(index, buf));
    if let Some(Some((n, overrun))) = res {
        // nikdy víc, než kolik buffer drží; ukončovací nula zůstává na místě
        *text.add(n) = 0;
        *write = 1;
        // Test z oddílu 3.1 (jen na výslovné přání v cfg): o jeden znak víc, než má buffer.
        if n == len && overrun {
            *text.add(len) = b'0' as u16;
            *text.add(len + 1) = 0;
        }
    }
}

/// # Safety
/// `active` ukazuje na Boolean. Plugin žádné triggery nepoužívá.
#[no_mangle]
pub unsafe extern "system" fn AccessTrigger(_index: u16, _active: *mut u8) {}
