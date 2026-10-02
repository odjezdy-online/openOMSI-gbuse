//! Čtení názvů zastávek a cílů z HOF souboru a jejich párování na ZST (`zst_map.csv`).

use buse_engine::db::cp1250_to_char;
use buse_engine::names::{fix_mojibake, match_text, normalize, NameIndex};
use buse_engine::Db;
use std::fmt::Write as _;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Hof {
    pub name: String,
    /// (ident, řetězce)
    pub stops: Vec<(String, Vec<String>)>,
    /// (kód, ident, řetězce)
    pub termini: Vec<(String, String, Vec<String>)>,
    /// Čísla linek z `[infosystem_trip]` (čtvrtý řádek záznamu, jinak kód spoje děleno 100).
    pub lines: Vec<String>,
}

/// HOF je ANSI (na českém systému Windows-1250); UTF-8 s BOM se pozná podle BOM.
pub fn decode(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(utf8) => String::from_utf8_lossy(utf8).into_owned(),
        None => bytes.iter().map(|&b| cp1250_to_char(b)).collect(),
    }
}

fn clean(line: &str) -> &str {
    // `.hof` ořezává z konce řádku tabulátory, mezery, CR/LF a uvozovky (FORMATS.md)
    line.trim_end_matches(['\t', ' ', '\r', '\n', '"'])
}

impl Hof {
    pub fn parse(text: &str) -> Hof {
        let mut hof = Hof::default();
        let (mut n_term, mut n_stop) = (0usize, 0usize);
        let mut lines = text.lines().map(clean);
        while let Some(line) = lines.next() {
            match line.trim() {
                "[name]" => hof.name = lines.next().unwrap_or("").trim().to_string(),
                "[stringcount_terminus]" => n_term = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0),
                "[stringcount_busstop]" => n_stop = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0),
                "[addbusstop]" => {
                    let ident = lines.next().unwrap_or("").trim().to_string();
                    let strings = (0..n_stop).filter_map(|_| lines.next()).map(|s| s.trim().to_string()).collect();
                    hof.stops.push((ident, strings));
                }
                "[addterminus]" | "[addterminus_allexit]" => {
                    let code = lines.next().unwrap_or("").trim().to_string();
                    let ident = lines.next().unwrap_or("").trim().to_string();
                    let strings = (0..n_term).filter_map(|_| lines.next()).map(|s| s.trim().to_string()).collect();
                    hof.termini.push((code, ident, strings));
                }
                "[infosystem_trip]" => {
                    let code = lines.next().unwrap_or("").trim().to_string();
                    let _name = lines.next();
                    let _terminus = lines.next();
                    let line = lines.next().unwrap_or("").trim().to_string();
                    let digits = |s: &str| !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit());
                    let line = if digits(&line) && line.parse::<u32>() != Ok(0) {
                        Some(line)
                    } else {
                        code.parse::<u32>().ok().map(|c| c / 100).filter(|l| (1..1000).contains(l)).map(|l| l.to_string())
                    };
                    if let Some(l) = line {
                        let l = l.trim_start_matches('0').to_string();
                        if !hof.lines.contains(&l) {
                            hof.lines.push(l);
                        }
                    }
                }
                tag @ ("[addbusstop_list]" | "[addterminus_list]") => {
                    for row in lines.by_ref() {
                        if row.trim() == "[end]" {
                            break;
                        }
                        let mut f = row.split('\t').map(|s| s.trim().trim_matches('"').to_string());
                        if tag == "[addbusstop_list]" {
                            if let Some(ident) = f.next().filter(|i| !i.is_empty()) {
                                hof.stops.push((ident, f.collect()));
                            }
                        } else {
                            // první sloupec řádku je příznak `{ALLEX}` (nebo prázdný), pak kód a ident
                            let mut first = f.next().unwrap_or_default();
                            if first.is_empty() || first.starts_with('{') {
                                first = f.next().unwrap_or_default();
                            }
                            let (code, ident) = (first, f.next().unwrap_or_default());
                            if !code.is_empty() {
                                hof.termini.push((code, ident, f.filter(|s| s != "{ALLEX}").collect()));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        hof
    }

    /// Všechny různé neprázdné názvy (zastávky i cíle) v pořadí výskytu, s původem.
    pub fn names(&self) -> Vec<(String, &'static str)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let stops = self.stops.iter().flat_map(|s| s.1.iter()).map(|n| (n, "zastávka"));
        let termini = self.termini.iter().flat_map(|t| t.2.iter()).map(|n| (n, "cíl"));
        for (name, kind) in stops.chain(termini) {
            let name = fix_mojibake(name.trim()).into_owned();
            let key = normalize(&name);
            if !key.is_empty() && seen.insert(key) {
                out.push((name, kind));
            }
        }
        out
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MapStats {
    pub names: usize,
    pub exact: usize,
    pub candidates: usize,
}

/// `zst_map.csv`: `omsi_name;zst_id;skóre;text ZST;původ`. Id se vyplní jen při přesné shodě
/// po normalizaci; částečné shody jsou nabídnuté v dalším sloupci k ručnímu doplnění.
pub fn zst_map_csv(db: &Db, hofs: &[(String, Hof)]) -> (String, MapStats) {
    let index = NameIndex::new(db);
    let mut stats = MapStats::default();
    let mut s = String::from(
        "# zst_map.csv - mapování názvů zastávek a cílů z OMSI (HOF) na ZST id databáze gBUSE1\n\
         # formát: omsi_name;zst_id;skóre;text ZST;původ   (plugin čte jen první dva sloupce)\n\
         # zst_id je vyplněné jen při přesné shodě po normalizaci (bez diakritiky, malá písmena,\n\
         # sjednocené zkratky). Řádky s prázdným id doplň ručně, nebo je nech být: název se pak\n\
         # vykreslí přímo fontem E1 (fallback).\n",
    );
    let mut seen = std::collections::HashSet::new();
    for (file, hof) in hofs {
        let _ = writeln!(s, "# --- {file} ({}): {} zastávek, {} cílů", hof.name, hof.stops.len(), hof.termini.len());
        for (name, kind) in hof.names() {
            if !seen.insert(normalize(&name)) {
                continue;
            }
            stats.names += 1;
            let found = index.find(&name);
            let text = |i: usize| match_text(db, &db.zst[i].raw).trim().to_string();
            match found {
                Some(m) if m.score == 100 => {
                    stats.exact += 1;
                    let _ = writeln!(s, "{name};{};100;{};{kind}", db.zst[m.index].id, text(m.index));
                }
                Some(m) if m.score >= 50 => {
                    stats.candidates += 1;
                    let _ = writeln!(s, "{name};;{};?{} {};{kind}", m.score, db.zst[m.index].id, text(m.index));
                }
                _ => {
                    let _ = writeln!(s, "{name};;0;;{kind}");
                }
            }
        }
    }
    (s, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lists_and_blocks() {
        let text = "[name]\nDepo\n\n[stringcount_terminus]\n2\n[stringcount_busstop]\n2\n\
                    [addterminus]\n12\nident_a\nHlavní nádraží\nHl. nádr.\n\
                    [addterminus_list]\n13\tident_b\t\tČeská\t\"\"\n[end]\n\
                    [addbusstop_list]\nstop1\t\tNám. Míru\t\tNám. Míru\nstop2\t\tAchtelky\n[end]\n\
                    [addbusstop]\nstop3\nLesná\nLesna\n";
        let hof = Hof::parse(text);
        assert_eq!(hof.name, "Depo");
        assert_eq!(hof.termini.len(), 2);
        assert_eq!(hof.termini[0], ("12".into(), "ident_a".into(), vec!["Hlavní nádraží".into(), "Hl. nádr.".into()]));
        assert_eq!(hof.stops.len(), 3);
        let names: Vec<String> = hof.names().into_iter().map(|n| n.0).collect();
        // "Hl. nádr." se po normalizaci rovná "Hlavní nádraží", "Lesna" = "Lesná"
        assert_eq!(names, ["Nám. Míru", "Achtelky", "Lesná", "Hlavní nádraží", "Česká"]);
    }
}
