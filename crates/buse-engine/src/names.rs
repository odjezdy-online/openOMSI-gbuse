//! Normalizace a párování názvů zastávek (OMSI / HOF <-> ZST) a převod textu na kódy glyfů.

use crate::db::Db;

/// Odstraní diakritiku z jednoho znaku (čeština, slovenština, němčina, polština).
pub fn fold_char(c: char) -> char {
    match c {
        'á' | 'ä' | 'â' | 'ă' | 'ą' => 'a',
        'Á' | 'Ä' | 'Â' | 'Ă' | 'Ą' => 'A',
        'č' | 'ć' | 'ç' => 'c',
        'Č' | 'Ć' | 'Ç' => 'C',
        'ď' | 'đ' => 'd',
        'Ď' | 'Đ' => 'D',
        'é' | 'ě' | 'ë' | 'ę' => 'e',
        'É' | 'Ě' | 'Ë' | 'Ę' => 'E',
        'í' | 'î' => 'i',
        'Í' | 'Î' => 'I',
        'ľ' | 'ĺ' | 'ł' => 'l',
        'Ľ' | 'Ĺ' | 'Ł' => 'L',
        'ň' | 'ń' => 'n',
        'Ň' | 'Ń' => 'N',
        'ó' | 'ö' | 'ô' | 'ő' => 'o',
        'Ó' | 'Ö' | 'Ô' | 'Ő' => 'O',
        'ř' | 'ŕ' => 'r',
        'Ř' | 'Ŕ' => 'R',
        'š' | 'ś' | 'ş' => 's',
        'Š' | 'Ś' | 'Ş' => 'S',
        'ť' | 'ţ' => 't',
        'Ť' | 'Ţ' => 'T',
        'ú' | 'ů' | 'ü' | 'ű' => 'u',
        'Ú' | 'Ů' | 'Ü' | 'Ű' => 'U',
        'ý' => 'y',
        'Ý' => 'Y',
        'ž' | 'ź' | 'ż' => 'z',
        'Ž' | 'Ź' | 'Ż' => 'Z',
        'ß' => 's',
        _ => c,
    }
}

/// Sjednocené zkratky: (zkratka bez tečky, plný tvar). Porovnává se po slovech.
const ABBREV: &[(&str, &str)] = &[
    ("nam", "namesti"),
    ("n", "namesti"),
    ("nadr", "nadrazi"),
    ("zel", "zeleznicni"),
    ("st", "stanice"),
    ("zast", "zastavka"),
    ("aut", "autobusove"),
    ("autobus", "autobusove"),
    ("hl", "hlavni"),
    ("ul", "ulice"),
    ("tr", "trida"),
    ("sidl", "sidliste"),
    ("nem", "nemocnice"),
    ("rozc", "rozcesti"),
    ("kr", "kralovo"),
    ("sv", "svateho"),
    ("gen", "generala"),
    ("dr", "doktora"),
    ("zs", "skola"),
    ("ks", "konecna"),
];

/// Normalizace: bez diakritiky, malá písmena, interpunkce -> mezery, rozvinuté zkratky.
pub fn normalize(name: &str) -> String {
    let mut flat = String::with_capacity(name.len());
    for c in name.chars() {
        let c = fold_char(c);
        if c.is_ascii_alphanumeric() {
            flat.push(c.to_ascii_lowercase());
        } else {
            flat.push(' ');
        }
    }
    let mut out = String::with_capacity(flat.len() + 8);
    for w in flat.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(ABBREV.iter().find(|a| a.0 == w).map_or(w, |a| a.1));
    }
    out
}

/// Skóre shody dvou normalizovaných názvů: 100 = shodné, 0 = nic společného.
/// Slova se párují v pořadí; zkrácené slovo (prefix) se počítá jako částečná shoda.
pub fn match_score(a: &str, b: &str) -> u32 {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    if a == b {
        return 100;
    }
    let (wa, wb): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    let n = wa.len().max(wb.len());
    let mut score = 0u32;
    for i in 0..wa.len().min(wb.len()) {
        let (x, y) = (wa[i], wb[i]);
        if x == y {
            score += 100;
        } else if x.len() >= 3 && y.len() >= 3 && (x.starts_with(y) || y.starts_with(x)) {
            score += 70;
        } else {
            return 0;
        }
    }
    // chybějící slova na konci (např. "Hlavní nádraží" vs "Hlavní nádraží, nást. 3")
    (score / n as u32).min(99).saturating_sub(10 * (wa.len().abs_diff(wb.len()) as u32).min(3))
}

/// Opraví text, který vznikl čtením Windows-1250 jako Windows-1252 (typické pro OMSI 2
/// na českém systému: "Nádra¾í", "Èerná Pole"). Text bez takových znaků vrátí beze změny.
pub fn fix_mojibake(s: &str) -> std::borrow::Cow<'_, str> {
    // znaky, které se v českém textu nevyskytují, ale vznikají chybným dekódováním
    const SUSPECT: &[char] = &['è', 'ì', 'ø', 'ù', 'ï', 'ò', 'È', 'Ì', 'Ø', 'Ù', 'Ï', 'Ò', '¾', '»', '¹', 'ð', 'þ', 'æ', 'ê', 'å', 'ã', 'õ', 'û', 'à', 'À'];
    if !s.contains(SUSPECT) {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(
        s.chars()
            .map(|c| match c as u32 {
                0xA0..=0xFF => crate::db::cp1250_to_char(c as u8),
                0x9D => 'ť',
                0x8D => 'Ť',
                _ => c,
            })
            .collect(),
    )
}

/// Převede text na kódy glyfů databáze (bez řídicích kódů) a připojí je do `out`.
pub fn encode_text(db: &Db, s: &str, out: &mut Vec<u8>) {
    for c in s.chars() {
        let c = if c == '&' { '+' } else { c };
        let usable = |code: u8| (0x20..0xB0).contains(&code) && code != 0x26;
        let code = db
            .char_to_code(c)
            .filter(|&k| usable(k))
            .or_else(|| db.char_to_code(fold_char(c)).filter(|&k| usable(k)));
        match code {
            Some(k) => out.push(k),
            None if c.is_control() => {}
            None => out.push(b'?'),
        }
    }
}

/// Text záznamu pro párování názvů: jen písmena a číslice, bez `&`, piktogramů a glyfů
/// v neznámých (piktogramových) fontech.
pub fn match_text(db: &Db, raw: &[u8]) -> String {
    use crate::text::{tokens, Tok};
    let (mut s, mut known) = (String::new(), true);
    for tok in tokens(raw) {
        match tok {
            Tok::Font(f) => known = db.font(f).is_some(),
            Tok::Glyph(c) if known && c < 0xAF => {
                if let Some(ch) = db.code_to_char(c) {
                    s.push(ch);
                }
            }
            Tok::Spacing(v) if v >= 2 => s.push(' '),
            _ => {}
        }
    }
    s
}

/// Předpočítané normalizované názvy ZST pro hledání podle jména.
#[derive(Debug, Clone, Default)]
pub struct NameIndex {
    /// (normalizovaný název, index do `db.zst`)
    names: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameMatch {
    pub index: usize,
    pub score: u32,
}

impl NameIndex {
    pub fn new(db: &Db) -> Self {
        let names = db
            .zst
            .iter()
            .enumerate()
            .map(|(i, r)| (normalize(&match_text(db, &r.raw)), i))
            .filter(|(n, _)| !n.is_empty())
            .collect();
        NameIndex { names }
    }

    /// Nejlepší shoda; při rovnosti skóre vyhrává první záznam v databázi.
    pub fn find(&self, name: &str) -> Option<NameMatch> {
        let n = normalize(name);
        let mut best: Option<NameMatch> = None;
        for (zn, i) in &self.names {
            let score = match_score(&n, zn);
            if score > best.map_or(0, |b| b.score) {
                best = Some(NameMatch { index: *i, score });
                if score == 100 {
                    break;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize("Nám. Míru"), "namesti miru");
        assert_eq!(normalize("Žel. st. Řečkovice"), "zeleznicni stanice reckovice");
        assert_eq!(normalize("  Česká  "), "ceska");
    }

    #[test]
    fn scores() {
        assert_eq!(match_score("hlavni nadrazi", "hlavni nadrazi"), 100);
        assert!(match_score("kralovo pole nadrazi", "kralovo pole nadr") < 100);
        assert!(match_score("kohoutovice hajenka", "kohoutovice haj") >= 70);
        assert_eq!(match_score("ceska", "moravske namesti"), 0);
    }

    #[test]
    fn mojibake() {
        assert_eq!(fix_mojibake("Èerná Pole"), "Černá Pole");
        assert_eq!(fix_mojibake("Hlavní nádraží"), "Hlavní nádraží");
        assert_eq!(fix_mojibake("Øeèkovice"), "Řečkovice");
    }
}
