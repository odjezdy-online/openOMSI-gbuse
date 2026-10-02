//! OMSI 2 (32bit Delphi) hledá exporty pod nedekorovanými jmény (`PluginStart`, ne
//! `_PluginStart@4`). Na i686 MSVC to zajistí .def soubor, na i686 GNU `--kill-at`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=buse_panel.def");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || arch != "x86" {
        return;
    }
    if env == "msvc" {
        let def = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("buse_panel.def");
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
    } else {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--kill-at");
    }
}
