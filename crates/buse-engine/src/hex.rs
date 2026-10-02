//! Intel HEX -> souvislý obraz od adresy 0, nevyplněné bajty = 0xFF.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexError {
    BadDigit { line: usize },
    BadChecksum { line: usize },
    Truncated { line: usize },
    TooLarge { size: usize },
    Empty,
}

impl fmt::Display for HexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexError::BadDigit { line } => write!(f, "Intel HEX, řádek {line}: neplatná hex číslice"),
            HexError::BadChecksum { line } => write!(f, "Intel HEX, řádek {line}: špatný kontrolní součet"),
            HexError::Truncated { line } => write!(f, "Intel HEX, řádek {line}: zkrácený záznam"),
            HexError::TooLarge { size } => write!(f, "Intel HEX: obraz {size} B je větší než limit"),
            HexError::Empty => write!(f, "Intel HEX: žádná data"),
        }
    }
}

impl std::error::Error for HexError {}

/// Horní mez velikosti obrazu; databáze gBUSE mají desítky kB.
const MAX_IMAGE: usize = 16 << 20;

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn parse_intel_hex(text: &str) -> Result<Vec<u8>, HexError> {
    let mut img: Vec<u8> = Vec::new();
    let mut upper: usize = 0;
    let mut rec: Vec<u8> = Vec::with_capacity(64);
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        let line = line.trim();
        let Some(body) = line.strip_prefix(':') else { continue };
        let b = body.as_bytes();
        if b.len() % 2 != 0 {
            return Err(HexError::BadDigit { line: n });
        }
        rec.clear();
        for pair in b.chunks_exact(2) {
            let (Some(h), Some(l)) = (nibble(pair[0]), nibble(pair[1])) else {
                return Err(HexError::BadDigit { line: n });
            };
            rec.push(h << 4 | l);
        }
        if rec.iter().fold(0u8, |a, &x| a.wrapping_add(x)) != 0 {
            return Err(HexError::BadChecksum { line: n });
        }
        if rec.len() < 5 {
            return Err(HexError::Truncated { line: n });
        }
        let ln = rec[0] as usize;
        let addr = (rec[1] as usize) << 8 | rec[2] as usize;
        let data = &rec[4..rec.len() - 1];
        match rec[3] {
            0 => {
                let data = &data[..ln.min(data.len())];
                let start = upper + addr;
                let end = start + data.len();
                if end > MAX_IMAGE {
                    return Err(HexError::TooLarge { size: end });
                }
                if img.len() < end {
                    img.resize(end, 0xFF);
                }
                img[start..end].copy_from_slice(data);
            }
            1 => break,
            2 | 4 => {
                if data.len() < 2 {
                    return Err(HexError::Truncated { line: n });
                }
                let v = (data[0] as usize) << 8 | data[1] as usize;
                upper = if rec[3] == 2 { v << 4 } else { v << 16 };
            }
            _ => {}
        }
    }
    if img.is_empty() {
        return Err(HexError::Empty);
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_ff() {
        let img = parse_intel_hex(":020000000102FB\n:0100040005F6\n:00000001FF\n").unwrap();
        assert_eq!(img, [1, 2, 0xFF, 0xFF, 5]);
    }

    #[test]
    fn checksum_is_checked() {
        assert_eq!(parse_intel_hex(":020000000102FC\n"), Err(HexError::BadChecksum { line: 1 }));
    }
}
