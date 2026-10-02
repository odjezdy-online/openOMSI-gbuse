//! Úpravy kopií souborů vozu (.bus, model.cfg, hlavní skript) pro vložení panelu.
//! Originály se jen čtou; výsledek jsou nové soubory s jiným jménem.
//!
//! Soubory vozů jsou ANSI: pracuje se s nimi jako s Latin-1 (bajt = znak), takže se
//! neznámé znaky při zápisu nezmění.

use crate::assets::Names;

pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

pub fn to_latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
}

fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect()
}

fn join(lines: &[String]) -> String {
    lines.join("\r\n")
}

fn find_tag(lines: &[String], tag: &str, from: usize) -> Option<usize> {
    (from..lines.len()).find(|&i| lines[i].trim().eq_ignore_ascii_case(tag))
}

/// Položky číslovaného seznamu `[tag]` + počet + položky; vrací (index řádku s počtem, položky).
pub fn list_entries(text: &str, tag: &str) -> Option<Vec<String>> {
    let l = lines(text);
    let i = find_tag(&l, tag, 0)?;
    let n: usize = l.get(i + 1)?.trim().parse().ok()?;
    Some(l.iter().skip(i + 2).take(n).map(|s| s.trim().to_string()).collect())
}

/// Přidá položku na konec číslovaného seznamu a zvýší počet.
fn list_push(l: &mut Vec<String>, tag: &str, entry: &str) -> Result<(), String> {
    let i = find_tag(l, tag, 0).ok_or(format!("{tag} v souboru není"))?;
    let n: usize = l.get(i + 1).and_then(|s| s.trim().parse().ok()).ok_or(format!("{tag}: chybí počet"))?;
    if i + 2 + n > l.len() {
        return Err(format!("{tag}: seznam je kratší než jeho počet"));
    }
    l[i + 1] = (n + 1).to_string();
    l.insert(i + 2 + n, entry.to_string());
    Ok(())
}

pub struct BusPatch<'a> {
    pub model_cfg: &'a str,
    /// (původní cesta skriptu, nová cesta) - hlavní skripty s `{frame}`.
    pub replaced_scripts: &'a [(String, String)],
    pub panel_script: &'a str,
    pub varlist: &'a str,
    pub stringvarlist: &'a str,
    pub name_suffix: &'a str,
}

/// Kopie `.bus`: jiný model.cfg, hlavní skript s voláním maker panelu, skript a seznamy panelu.
pub fn patch_bus(text: &str, p: &BusPatch) -> Result<String, String> {
    let mut l = lines(text);
    let m = find_tag(&l, "[model]", 0).ok_or("[model] v .bus není")?;
    *l.get_mut(m + 1).ok_or("[model]: chybí cesta")? = p.model_cfg.to_string();
    if let Some(f) = find_tag(&l, "[friendlyname]", 0) {
        if let Some(model) = l.get_mut(f + 2) {
            model.push_str(p.name_suffix);
        }
    }
    let s = find_tag(&l, "[script]", 0).ok_or("[script] v .bus není")?;
    let n: usize = l[s + 1].trim().parse().map_err(|_| "[script]: chybí počet")?;
    for line in l.iter_mut().skip(s + 2).take(n) {
        if let Some((_, new)) = p.replaced_scripts.iter().find(|(old, _)| old.eq_ignore_ascii_case(line.trim())) {
            *line = new.clone();
        }
    }
    list_push(&mut l, "[script]", p.panel_script)?;
    list_push(&mut l, "[varnamelist]", p.varlist)?;
    list_push(&mut l, "[stringvarnamelist]", p.stringvarlist)?;
    Ok(join(&l))
}

/// Počet `[scripttexture]` v model.cfg = index, který dostane nová textura.
pub fn scripttexture_count(text: &str) -> usize {
    lines(text).iter().filter(|l| l.trim().eq_ignore_ascii_case("[scripttexture]")).count()
}

/// Kopie model.cfg: nová `[scripttexture]` za poslední existující a meshe panelu
/// (na konec první úrovně `[LOD]`, bez LOD na konec souboru).
pub fn patch_model_cfg(text: &str, names: &Names, model_dir: &str) -> Result<String, String> {
    let mut l = lines(text);
    if scripttexture_count(text) != names.st_index {
        return Err(format!("model.cfg má {} skriptových textur, čekám {}", scripttexture_count(text), names.st_index));
    }
    let lod: Vec<usize> = (0..l.len()).filter(|&i| l[i].trim().eq_ignore_ascii_case("[LOD]")).collect();
    let mesh_at = lod.get(1).copied().unwrap_or(l.len());
    let meshes = lines(&names.cfg_meshes(model_dir));
    l.splice(mesh_at..mesh_at, meshes);
    let st: Vec<String> = lines(&names.cfg_scripttexture());
    let st_at = match (0..l.len()).rev().find(|&i| l[i].trim().eq_ignore_ascii_case("[scripttexture]")) {
        Some(i) => (i + 3).min(l.len()),
        // žádná skriptová textura: před první mesh
        None => find_tag(&l, "[mesh]", 0).unwrap_or(0),
    };
    l.splice(st_at..st_at, st);
    Ok(join(&l))
}

/// Má skript blok `{init}` a `{frame}` (hlavní skript vozu)?
pub fn is_main_script(text: &str) -> bool {
    let l = lines(text);
    find_tag(&l, "{init}", 0).is_some() && find_tag(&l, "{frame}", 0).is_some()
}

/// Kopie hlavního skriptu: do `{init}` a `{frame}` přidá volání maker panelu.
pub fn patch_main_script(text: &str, prefix: &str) -> Result<String, String> {
    let mut l = lines(text);
    let init = find_tag(&l, "{init}", 0).ok_or("{init} ve skriptu není")?;
    let init_end = find_tag(&l, "{end}", init).ok_or("{init} nemá {end}")?;
    l.insert(init_end, format!("\t(M.L.{prefix}_init)"));
    let frame = find_tag(&l, "{frame}", 0).ok_or("{frame} ve skriptu není")?;
    let frame_end = find_tag(&l, "{end}", frame).ok_or("{frame} nemá {end}")?;
    // na konec snímku: IBIS skript už má nové hodnoty, plugin je uvidí v tomtéž snímku
    l.insert(frame_end, format!("\t(M.L.{prefix}_frame)"));
    Ok(join(&l))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUS: &str = "[friendlyname]\r\nSOR\r\nNB12\r\nDPP\r\n\r\n[model]\r\nmodel\\NB.cfg\r\n\r\n[varnamelist]\r\n2\r\na.txt\r\nb.txt\r\n\r\n[stringvarnamelist]\r\n1\r\ns.txt\r\n\r\n[script]\r\n2\r\nscript\\main.osc\r\nscript\\ibis.osc\r\n\r\n[constfile]\r\n1\r\nc.txt\r\n";

    #[test]
    fn bus_gets_new_model_scripts_and_lists() {
        let out = patch_bus(
            BUS,
            &BusPatch {
                model_cfg: "model\\NB_BSLED.cfg",
                replaced_scripts: &[("script\\main.osc".into(), "script\\BSLED\\main_BSLED.osc".into())],
                panel_script: "script\\BSLED\\buse_panel.osc",
                varlist: "script\\BSLED\\v.txt",
                stringvarlist: "script\\BSLED\\s.txt",
                name_suffix: " + BS120",
            },
        )
        .unwrap();
        assert_eq!(list_entries(&out, "[script]").unwrap(), ["script\\BSLED\\main_BSLED.osc", "script\\ibis.osc", "script\\BSLED\\buse_panel.osc"]);
        assert_eq!(list_entries(&out, "[varnamelist]").unwrap().len(), 3);
        assert_eq!(list_entries(&out, "[stringvarnamelist]").unwrap(), ["s.txt", "script\\BSLED\\s.txt"]);
        assert_eq!(list_entries(&out, "[constfile]").unwrap(), ["c.txt"]);
        assert!(out.contains("[model]\r\nmodel\\NB_BSLED.cfg\r\n"));
        assert!(out.contains("[friendlyname]\r\nSOR\r\nNB12 + BS120\r\nDPP\r\n"));
    }

    #[test]
    fn main_script_calls_panel_macros() {
        let src = "{init}\r\n\t(M.L.a_init)\r\n{end}\r\n\r\n{frame}\r\n\t(M.L.a_frame)\r\n(L.L.x)\r\n{if}\r\n{endif}\r\n{end}\r\n{trigger:t}\r\n{end}\r\n";
        assert!(is_main_script(src));
        let out = patch_main_script(src, "BSLED").unwrap();
        assert!(out.contains("\t(M.L.a_init)\r\n\t(M.L.BSLED_init)\r\n{end}"));
        assert!(out.contains("{endif}\r\n\t(M.L.BSLED_frame)\r\n{end}\r\n{trigger:t}"));
    }

    #[test]
    fn model_cfg_gets_texture_and_meshes() {
        let names = Names { prefix: "BSLED".into(), width: 112, strips: 1, height: 8, st_index: 2, power_var: "elec_busbar_main".into() };
        let src = "0\r\n[scripttexture]\r\n256\r\n64\r\n\r\n1\r\n[scripttexture]\r\n10\r\n20\r\n\r\n[mesh]\r\na.o3d\r\n";
        let out = patch_model_cfg(src, &names, "BSLED").unwrap();
        assert_eq!(scripttexture_count(&out), 3);
        let l = lines(&out);
        let last = (0..l.len()).rev().find(|&i| l[i] == "[scripttexture]").unwrap();
        assert_eq!((l[last + 1].as_str(), l[last + 2].as_str()), ("112", "8"));
        assert!(last < find_tag(&l, "[mesh]", 0).unwrap());
        assert!(out.contains("[matl_transmap]\r\n\\S:2\r\n"));
        assert!(out.trim_end().ends_with("[matl_nightmap]\r\nBSLED_led.png"));
        assert!(patch_model_cfg(src, &Names { st_index: 5, ..names }, "BSLED").is_err());
    }
}
