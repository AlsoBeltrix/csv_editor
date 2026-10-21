//! Bytes to text and back, and the sniffers that read a file's layout off its first kilobytes.
//!
//! `decode` and `sniff_delimiter` are adapted from GridCraft's `crates/xlsx/src/csv.rs` (MIT, see
//! `ATTRIBUTION.md`).

use std::borrow::Cow;

use crate::dialect::{Encoding, LineEnding};

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// How far the sniffers read.
const SNIFF_BYTES: usize = 64 * 1024;

/// Decodes file bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Windows-1252. Never fails. Valid
/// UTF-8 is borrowed, not copied: a large file is parsed straight from its bytes.
pub fn decode(bytes: &[u8]) -> (Cow<'_, str>, Encoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest), Encoding::Utf8Bom);
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let (pairs, _odd_byte) = b.as_chunks::<2>();
        let units = pairs.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (Cow::Owned(utf16(rest, true)), Encoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (Cow::Owned(utf16(rest, false)), Encoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (Cow::Borrowed(s), Encoding::Utf8),
        Err(_) => (Cow::Owned(bytes.iter().map(|&b| cp1252_char(b)).collect()), Encoding::Windows1252),
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

/// The byte-order mark the encoding starts a file with (empty for most).
pub fn bom(encoding: Encoding) -> &'static [u8] {
    match encoding {
        Encoding::Utf8 | Encoding::Windows1252 => &[],
        Encoding::Utf8Bom => &[0xEF, 0xBB, 0xBF],
        Encoding::Utf16Le => &[0xFF, 0xFE],
        Encoding::Utf16Be => &[0xFE, 0xFF],
    }
}

/// Encodes text for saving: the BOM, then the body. The second value counts characters the
/// encoding cannot represent (written as `?`); it is only ever non-zero for Windows-1252.
pub fn encode(text: &str, encoding: Encoding) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(text.len() + 3);
    out.extend_from_slice(bom(encoding));
    let lossy = encode_into(text, encoding, &mut out);
    (out, lossy)
}

/// Appends the encoded text (no BOM) to `out`; see [`encode`] for the count returned.
pub fn encode_into(text: &str, encoding: Encoding, out: &mut Vec<u8>) -> usize {
    match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            out.extend_from_slice(text.as_bytes());
            0
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let le = encoding == Encoding::Utf16Le;
            out.reserve(text.len() * 2);
            for u in text.encode_utf16() {
                out.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            0
        }
        Encoding::Windows1252 => {
            let mut lossy = 0usize;
            out.reserve(text.len());
            for c in text.chars() {
                out.push(cp1252_byte(c).unwrap_or_else(|| {
                    lossy += 1;
                    b'?'
                }));
            }
            lossy
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

    fn decoded(bytes: &[u8]) -> (String, Encoding) {
        let (text, enc) = decode(bytes);
        (text.into_owned(), enc)
    }

    #[test]
    fn decodes_boms_and_fallbacks() {
        assert_eq!(decoded(b"\xEF\xBB\xBFa,b"), ("a,b".to_string(), Encoding::Utf8Bom));
        assert_eq!(decoded(b"\xFF\xFEa\0,\0"), ("a,".to_string(), Encoding::Utf16Le));
        assert_eq!(decoded(b"\xFE\xFF\0a\0,"), ("a,".to_string(), Encoding::Utf16Be));
        assert_eq!(decoded("héllo".as_bytes()), ("héllo".to_string(), Encoding::Utf8));
        // Latin-1 é and the Windows-1252 euro sign.
        assert_eq!(decoded(b"h\xE9llo \x80"), ("héllo €".to_string(), Encoding::Windows1252));
    }

    #[test]
    fn valid_utf8_is_borrowed_not_copied() {
        assert!(matches!(decode(b"a,b\n").0, Cow::Borrowed(_)));
        assert!(matches!(decode(b"\xEF\xBB\xBFa,b\n").0, Cow::Borrowed(_)));
        assert!(matches!(decode(b"caf\xE9").0, Cow::Owned(_)));
    }

    #[test]
    fn encoding_round_trips_each_encoding() {
        for enc in Encoding::ALL {
            let text = if enc == Encoding::Windows1252 { "héllo € “quoted”" } else { "héllo € 日本" };
            let (bytes, lossy) = encode(text, enc);
            assert_eq!(lossy, 0, "{enc:?}");
            assert_eq!(decoded(&bytes), (text.to_string(), enc), "{enc:?}");
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
