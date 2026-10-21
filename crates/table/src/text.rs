//! Bytes to text and back, and the sniffers that read a file's layout off its first kilobytes.
//!
//! `decode` and `sniff_delimiter` are adapted from GridCraft's `crates/xlsx/src/csv.rs` (MIT, see
//! `ATTRIBUTION.md`).

use crate::dialect::{Encoding, LineEnding};

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// How far the sniffers read.
const SNIFF_BYTES: usize = 64 * 1024;

/// Decodes file bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Windows-1252. Never fails.
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), Encoding::Utf8Bom);
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let (pairs, _odd_byte) = b.as_chunks::<2>();
        let units = pairs.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (utf16(rest, true), Encoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (utf16(rest, false), Encoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), Encoding::Utf8),
        Err(_) => (bytes.iter().map(|&b| cp1252_char(b)).collect(), Encoding::Windows1252),
    }
}

fn cp1252_char(b: u8) -> char {
    match b {
        0x80..=0x9F => char::from_u32(CP1252.get(usize::from(b - 0x80)).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
        _ => char::from(b),
    }
}

fn cp1252_byte(c: char) -> Option<u8> {
    let u = u32::from(c);
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return u8::try_from(u).ok();
    }
    CP1252.iter().position(|&p| p == u).and_then(|i| u8::try_from(0x80 + i).ok())
}

/// Encodes text for saving. The second value counts characters the encoding cannot represent
/// (written as `?`); it is only ever non-zero for Windows-1252.
pub fn encode(text: &str, encoding: Encoding) -> (Vec<u8>, usize) {
    match encoding {
        Encoding::Utf8 => (text.as_bytes().to_vec(), 0),
        Encoding::Utf8Bom => {
            let mut v = Vec::with_capacity(text.len() + 3);
            v.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            v.extend_from_slice(text.as_bytes());
            (v, 0)
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let le = encoding == Encoding::Utf16Le;
            let mut v = Vec::with_capacity(text.len() * 2 + 2);
            v.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            for u in text.encode_utf16() {
                v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            (v, 0)
        }
        Encoding::Windows1252 => {
            let mut lossy = 0usize;
            let v = text
                .chars()
                .map(|c| {
                    cp1252_byte(c).unwrap_or_else(|| {
                        lossy += 1;
                        b'?'
                    })
                })
                .collect();
            (v, lossy)
        }
    }
}

/// Picks comma, semicolon, tab or pipe: the candidate with the most consistent count per line
/// (outside quotes) over the first non-blank lines. A candidate must appear on every such line,
/// otherwise comma wins.
pub fn sniff_delimiter(text: &str, quote: u8) -> u8 {
    let quote = char::from(quote);
    let mut best = (b',', 0usize, 0usize);
    for d in [',', ';', '\t', '|'] {
        let mut counts = vec![];
        let mut n = 0usize;
        let mut in_q = false;
        let mut blank = true;
        for ch in text.chars().take(SNIFF_BYTES) {
            if !in_q && ch == '\n' {
                if !blank {
                    counts.push(n);
                }
                n = 0;
                blank = true;
                if counts.len() >= 20 {
                    break;
                }
                continue;
            }
            if ch != '\r' {
                blank = false;
            }
            if ch == quote {
                in_q = !in_q;
            } else if !in_q && ch == d {
                n += 1;
            }
        }
        if !blank {
            counts.push(n);
        }
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        if min > 0 && (min, total) > (best.1, best.2) {
            best = (d as u8, min, total);
        }
    }
    best.0
}

/// The majority line ending in the first kilobytes; CRLF when there is none.
pub fn sniff_line_ending(text: &str) -> LineEnding {
    let b = text.as_bytes();
    let (mut crlf, mut lf, mut cr) = (0usize, 0usize, 0usize);
    let mut i = 0usize;
    let limit = b.len().min(SNIFF_BYTES);
    while i < limit {
        match b.get(i) {
            Some(b'\r') => {
                if b.get(i + 1) == Some(&b'\n') {
                    crlf += 1;
                    i += 1;
                } else {
                    cr += 1;
                }
            }
            Some(b'\n') => lf += 1,
            _ => {}
        }
        i += 1;
    }
    if crlf >= lf && crlf >= cr {
        LineEnding::Crlf
    } else if lf >= cr {
        LineEnding::Lf
    } else {
        LineEnding::Cr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_boms_and_fallbacks() {
        assert_eq!(decode(b"\xEF\xBB\xBFa,b"), ("a,b".to_string(), Encoding::Utf8Bom));
        assert_eq!(decode(b"\xFF\xFEa\0,\0"), ("a,".to_string(), Encoding::Utf16Le));
        assert_eq!(decode(b"\xFE\xFF\0a\0,"), ("a,".to_string(), Encoding::Utf16Be));
        assert_eq!(decode("héllo".as_bytes()), ("héllo".to_string(), Encoding::Utf8));
        // Latin-1 é and the Windows-1252 euro sign.
        assert_eq!(decode(b"h\xE9llo \x80"), ("héllo €".to_string(), Encoding::Windows1252));
    }

    #[test]
    fn encoding_round_trips_each_encoding() {
        for enc in Encoding::ALL {
            let text = if enc == Encoding::Windows1252 { "héllo € “quoted”" } else { "héllo € 日本" };
            let (bytes, lossy) = encode(text, enc);
            assert_eq!(lossy, 0, "{enc:?}");
            assert_eq!(decode(&bytes), (text.to_string(), enc), "{enc:?}");
        }
        let (bytes, lossy) = encode("日本", Encoding::Windows1252);
        assert_eq!((bytes, lossy), (b"??".to_vec(), 2));
    }

    #[test]
    fn sniffs_delimiters() {
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3\n", b'"'), b',');
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", b'"'), b';');
        assert_eq!(sniff_delimiter("a\tb\n1\t2\n", b'"'), b'\t');
        assert_eq!(sniff_delimiter("a|b\n1|2\n", b'"'), b'|');
        // Delimiters inside quotes don't count; a lone word is a comma file.
        assert_eq!(sniff_delimiter("\"a;b;c\",d\n\"1;2\",3\n", b'"'), b',');
        assert_eq!(sniff_delimiter("hello\n", b'"'), b',');
        // Blank lines carry no evidence.
        assert_eq!(sniff_delimiter("a;b\n\n1;2\n", b'"'), b';');
    }

    #[test]
    fn sniffs_line_endings() {
        assert_eq!(sniff_line_ending("a\r\nb\r\n"), LineEnding::Crlf);
        assert_eq!(sniff_line_ending("a\nb\n"), LineEnding::Lf);
        assert_eq!(sniff_line_ending("a\rb\r"), LineEnding::Cr);
        assert_eq!(sniff_line_ending("a\nb\r\nc\n"), LineEnding::Lf);
        assert_eq!(sniff_line_ending("no newline"), LineEnding::Crlf);
    }
}
