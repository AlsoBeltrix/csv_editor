//! Reading and writing delimited text.
//!
//! The parser is RFC 4180 with the usual leniency (a quote inside an unquoted field is literal,
//! text after a closing quote is kept, a file may end without a line break, lone CR line ends are
//! accepted). It works on bytes: the delimiter, quote and line-break characters are ASCII, so
//! field boundaries always fall on character boundaries of the decoded UTF-8 text.

use std::io::Write;

use crate::dialect::{Dialect, Encoding, Quoting};
use crate::table::{Cell, Row, Table};
use crate::text::{bom, decode, encode_into, sniff_delimiter, sniff_line_ending};

/// The largest file opened (bytes). Above this the open is refused before anything is allocated.
pub const MAX_INPUT_BYTES: usize = 2 << 30;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the file is {0} bytes; files up to 2 GiB can be opened")]
    TooLarge(usize),
    #[error("the delimiter and the quote character must differ")]
    DelimiterIsQuote,
    #[error("{path}: {source}")]
    Io { path: String, source: std::io::Error },
}

/// What the caller already knows about the file; everything `None` is sniffed.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParseOptions {
    pub delimiter: Option<u8>,
    /// The quote character; `None` means `"`.
    pub quote: Option<u8>,
    pub header: Option<bool>,
}

/// Parses file bytes into a table and the dialect it was written in.
pub fn parse(bytes: &[u8], opts: &ParseOptions) -> Result<(Table, Dialect), Error> {
    check_size(bytes.len())?;
    let (text, encoding) = decode(bytes);
    let quote = opts.quote.unwrap_or(b'"');
    let delimiter = opts.delimiter.unwrap_or_else(|| sniff_delimiter(&text, quote));
    if delimiter == quote {
        return Err(Error::DelimiterIsQuote);
    }
    let line_ending = sniff_line_ending(&text);
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let mut rows: Vec<Row> = Vec::with_capacity(newlines.saturating_add(1).min(1 << 24));
    let stats = parse_records(&text, delimiter, quote, |rec| rows.push(rec));
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let pad_rows = rows.iter().all(|r| r.len() == width);
    let quoting = if stats.fields > 0 && stats.quoted_fields == stats.fields { Quoting::All } else { Quoting::AsSource };
    let header = opts.header.unwrap_or_else(|| guess_header(&rows));
    let dialect = Dialect { delimiter, quote, line_ending, encoding, quoting, trailing_newline: stats.ended_with_terminator, pad_rows, header };
    Ok((Table::from_rows(rows), dialect))
}

/// Refuses input over [`MAX_INPUT_BYTES`] before anything is allocated.
pub(crate) fn check_size(len: usize) -> Result<(), Error> {
    if len > MAX_INPUT_BYTES { Err(Error::TooLarge(len)) } else { Ok(()) }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ParseStats {
    pub fields: usize,
    pub quoted_fields: usize,
    /// The text ended right after a line break (or was empty).
    pub ended_with_terminator: bool,
}

/// Splits text into records, calling `on_record` for each.
pub(crate) fn parse_records(text: &str, delim: u8, quote: u8, mut on_record: impl FnMut(Row)) -> ParseStats {
    let b = text.as_bytes();
    let n = b.len();
    let mut stats = ParseStats { ended_with_terminator: true, ..ParseStats::default() };
    let mut i = 0usize;
    let mut record: Row = Vec::new();
    let is_break = |c: u8| c == delim || c == b'\r' || c == b'\n';
    // The index of the first byte at or after `from` matching `stop` (or the end).
    let scan = |from: usize, stop: &dyn Fn(u8) -> bool| -> usize {
        let rest = b.get(from..).unwrap_or(&[]);
        from + rest.iter().position(|&c| stop(c)).unwrap_or(rest.len())
    };
    // Hands the record over; the next one gets the same capacity (rows are usually alike).
    let mut emit = |record: &mut Row, stats: &mut ParseStats, ended: bool| {
        let len = record.len();
        on_record(std::mem::replace(record, Vec::with_capacity(len)));
        stats.ended_with_terminator = ended;
    };
    while i < n {
        // One field.
        let quoted = b.get(i) == Some(&quote);
        let field: Cell = if quoted {
            i += 1;
            let mut s = String::new();
            let mut start = i;
            loop {
                i = scan(i, &|c| c == quote);
                s.push_str(text.get(start..i).unwrap_or(""));
                if i >= n {
                    break;
                }
                if b.get(i + 1) == Some(&quote) {
                    s.push(char::from(quote));
                    i += 2;
                    start = i;
                } else {
                    i += 1;
                    break;
                }
            }
            // Lenient: anything between the closing quote and the next delimiter is kept.
            let tail = i;
            i = scan(i, &is_break);
            s.push_str(text.get(tail..i).unwrap_or(""));
            Cell::new_quoted(s)
        } else {
            let start = i;
            i = scan(i, &is_break);
            Cell::from(text.get(start..i).unwrap_or(""))
        };
        stats.fields += 1;
        if quoted {
            stats.quoted_fields += 1;
        }
        record.push(field);
        // What ended the field.
        match b.get(i) {
            Some(&c) if c == delim => {
                i += 1;
                if i >= n {
                    // "a," ends with an empty field.
                    record.push(Cell::default());
                    stats.fields += 1;
                    emit(&mut record, &mut stats, false);
                }
            }
            Some(&c) if c == b'\r' || c == b'\n' => {
                i += 1;
                if c == b'\r' && b.get(i) == Some(&b'\n') {
                    i += 1;
                }
                emit(&mut record, &mut stats, true);
            }
            None => emit(&mut record, &mut stats, false),
            Some(_) => i += 1,
        }
    }
    if !record.is_empty() {
        on_record(record);
        stats.ended_with_terminator = false;
    }
    stats
}

/// A first row is a header when it has no empty field and nothing that reads as a number.
pub(crate) fn guess_header(rows: &[Row]) -> bool {
    let Some(first) = rows.first() else { return false };
    !first.is_empty() && first.iter().all(|c| !c.trim().is_empty() && c.trim().parse::<f64>().is_err())
}

/// Writes the table in the dialect to `out`, row by row, so saving never holds a second copy
/// of the file. Returns how many characters the encoding could not represent (see
/// [`encode_into`]).
pub fn write_to(table: &Table, d: &Dialect, out: &mut impl Write) -> std::io::Result<usize> {
    let delim = char::from(d.delimiter);
    let q = char::from(d.quote);
    let nl = d.line_ending.as_str();
    let rows = table.row_count();
    let mut lossy = 0usize;
    let mut line = String::new();
    let mut bytes = Vec::new();
    out.write_all(bom(d.encoding))?;
    for (ri, row) in table.rows().iter().enumerate() {
        line.clear();
        let count = if d.pad_rows { table.width().max(row.len()) } else { row.len() };
        for c in 0..count {
            if c > 0 {
                line.push(delim);
            }
            let (text, quoted) = row.get(c).map_or(("", false), |s| (s.as_str(), s.quoted()));
            let quoting = if quoted && d.quoting == Quoting::AsSource { Quoting::All } else { d.quoting };
            write_field(&mut line, text, delim, q, quoting);
        }
        if ri + 1 < rows || d.trailing_newline {
            line.push_str(nl);
        }
        match d.encoding {
            Encoding::Utf8 | Encoding::Utf8Bom => out.write_all(line.as_bytes())?,
            _ => {
                bytes.clear();
                lossy += encode_into(&line, d.encoding, &mut bytes);
                out.write_all(&bytes)?;
            }
        }
    }
    Ok(lossy)
}

/// The table in the dialect as bytes (see [`write_to`]); for tests and small documents.
pub fn write(table: &Table, d: &Dialect) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(table.row_count().saturating_mul(16).min(1 << 28));
    // Writing into a vector cannot fail.
    let lossy = write_to(table, d, &mut out).unwrap_or(0);
    (out, lossy)
}

/// Clipboard text to cells: tab-separated, double-quote quoted, any line ending. Plain text
/// without tabs or line breaks is a single cell.
pub fn parse_block(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    parse_records(text, b'\t', b'"', |r| {
        // Clipboard quoting says nothing about the file: pasted text is new text.
        rows.push(r.into_iter().map(Cell::unquoted).collect());
    });
    rows
}

/// Cells to clipboard text: tab-separated, CRLF line ends, fields quoted when they need it.
pub fn block_text(rows: &[Row]) -> String {
    rows.iter().map(|r| write_row(r, '\t', '"')).collect::<Vec<_>>().join("\r\n")
}

/// A single row as text, for the clipboard. Fields are quoted only when they need it.
pub fn write_row(row: &[Cell], delim: char, quote: char) -> String {
    let mut out = String::new();
    for (c, cell) in row.iter().enumerate() {
        if c > 0 {
            out.push(delim);
        }
        write_field(&mut out, cell.as_str(), delim, quote, Quoting::Minimal);
    }
    out
}

fn write_field(out: &mut String, text: &str, delim: char, q: char, quoting: Quoting) {
    let needs = quoting == Quoting::All || contains_any(text, delim, q);
    if !needs {
        out.push_str(text);
        return;
    }
    out.push(q);
    if text.contains(q) {
        for ch in text.chars() {
            if ch == q {
                out.push(q);
            }
            out.push(ch);
        }
    } else {
        out.push_str(text);
    }
    out.push(q);
}

/// Whether the text has the delimiter, the quote or a line break. Both are single bytes in the
/// dialect, so for ASCII ones (always, in practice) this is a byte scan.
fn contains_any(text: &str, delim: char, q: char) -> bool {
    match (u8::try_from(delim), u8::try_from(q)) {
        (Ok(d), Ok(qb)) if d.is_ascii() && qb.is_ascii() => text.bytes().any(|b| b == d || b == qb || b == b'\r' || b == b'\n'),
        _ => text.chars().any(|ch| ch == delim || ch == q || ch == '\r' || ch == '\n'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{Encoding, LineEnding};

    fn parsed(text: &str) -> (Table, Dialect) {
        parse(text.as_bytes(), &ParseOptions::default()).unwrap()
    }

    fn cells(t: &Table) -> Vec<Vec<&str>> {
        t.rows().iter().map(|r| r.iter().map(|c| c.as_str()).collect()).collect()
    }

    #[test]
    fn parses_quotes_escapes_and_embedded_breaks() {
        let (t, d) = parsed("name,note\r\n\"Smith, J\",\"says \"\"hi\"\"\"\r\n\"multi\r\nline\",plain\r\n");
        assert_eq!(cells(&t), vec![vec!["name", "note"], vec!["Smith, J", "says \"hi\""], vec!["multi\r\nline", "plain"]]);
        assert_eq!(d.quoting, Quoting::AsSource);
        assert!(d.header && d.trailing_newline && d.pad_rows);
        assert_eq!(d.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn parses_lenient_shapes() {
        // Quote mid-field is literal; text after a closing quote is kept; EOF inside quotes.
        assert_eq!(cells(&parsed("a\"b,c").0), vec![vec!["a\"b", "c"]]);
        assert_eq!(cells(&parsed("\"a\"b,c").0), vec![vec!["ab", "c"]]);
        assert_eq!(cells(&parsed("\"open,x").0), vec![vec!["open,x"]]);
        // Trailing delimiter means a trailing empty field; blank lines are empty rows.
        assert_eq!(cells(&parsed("a,").0), vec![vec!["a", ""]]);
        assert_eq!(cells(&parsed("a,b\n\nc,d\n").0), vec![vec!["a", "b"], vec![""], vec!["c", "d"]]);
        // Lone CR line ends.
        assert_eq!(cells(&parsed("a,b\rc,d\r").0), vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(cells(&parsed("").0), Vec::<Vec<&str>>::new());
    }

    #[test]
    fn detects_dialect_details() {
        let (t, d) = parsed("x;y\n1;2");
        assert_eq!(d.delimiter, b';');
        assert_eq!(d.line_ending, LineEnding::Lf);
        assert!(!d.trailing_newline);
        assert!(d.header, "x;y has no empty or numeric field, so it reads as a header");
        assert_eq!(cells(&t), vec![vec!["x", "y"], vec!["1", "2"]]);
        let (_, d) = parsed("\"a\",\"b\"\n\"1\",\"\"\n");
        assert_eq!(d.quoting, Quoting::All);
        let (_, d) = parsed("a,b\nc\n");
        assert!(!d.pad_rows, "a ragged file stays ragged");
        let (_, d) = parsed("Name,Age\nAnn,3\n");
        assert!(d.header);
        let (_, d) = parsed("1,2\n3,4\n");
        assert!(!d.header);
    }

    #[test]
    fn header_can_be_forced() {
        let (_, d) = parse(b"1,2\n3,4\n", &ParseOptions { header: Some(true), ..Default::default() }).unwrap();
        assert!(d.header);
    }

    #[test]
    fn refuses_a_delimiter_equal_to_the_quote() {
        let err = parse(b"a,b", &ParseOptions { delimiter: Some(b'"'), ..Default::default() }).unwrap_err();
        assert!(matches!(err, Error::DelimiterIsQuote));
    }

    #[test]
    fn untouched_files_round_trip_byte_for_byte() {
        let samples: [&[u8]; 13] = [
            b"a,\"b\",c\r\n\"1\",2,\"3\"\r\n",
            b"\"name\",age\n\"Ann\",3\n\"\",4\n",
            b"x,\"\"\r\n",
            b"a,b,c\r\n1,2,3\r\n",
            b"a,b,c\n1,2,3",
            b"a;b\n\"x;y\";\"q\"\"q\"\n",
            b"a\tb\r\n1\t2\r\n",
            b"\"a\",\"b\"\r\n\"1\",\"\"\r\n",
            b"a,b\nc\nd,e,f\n",
            b"a,b\n\nc,d\n",
            b"\xEF\xBB\xBFname,city\r\nAnn,Oslo\r\n",
            b"\xFF\xFEa\0,\0b\0\r\0\n\0",
            b"caf\xE9,\x80\r\n",
        ];
        for bytes in samples {
            let (t, d) = parse(bytes, &ParseOptions::default()).unwrap();
            let (out, lossy) = write(&t, &d);
            assert_eq!(lossy, 0);
            assert_eq!(out, bytes, "{:?}", String::from_utf8_lossy(bytes));
        }
    }

    #[test]
    fn source_quotes_survive_untouched_cells_and_follow_edits() {
        let (mut t, d) = parsed("a,\"b\",c\r\n");
        assert_eq!(d.quoting, Quoting::AsSource);
        assert!(t.get(0, 1) == "b");
        // Editing a quoted cell: the new text is quoted only if it needs it.
        t.set(0, 1, "bee");
        assert_eq!(String::from_utf8(write(&t, &d).0).unwrap(), "a,bee,c\r\n");
        t.set(0, 2, "c,d");
        assert_eq!(String::from_utf8(write(&t, &d).0).unwrap(), "a,bee,\"c,d\"\r\n");
        // Minimal strips the file's optional quotes; All quotes everything.
        let (t, d) = parsed("a,\"b\",c\r\n");
        assert_eq!(String::from_utf8(write(&t, &Dialect { quoting: Quoting::Minimal, ..d }).0).unwrap(), "a,b,c\r\n");
        assert_eq!(String::from_utf8(write(&t, &Dialect { quoting: Quoting::All, ..d }).0).unwrap(), "\"a\",\"b\",\"c\"\r\n");
        // Clipboard text does not count as source quoting.
        assert!(parse_block("\"x\"\ty").iter().flatten().all(|c| !c.quoted()));
    }

    #[test]
    fn writer_quotes_only_what_needs_it_and_pads_when_asked() {
        let t = Table::from_rows(vec![
            vec![Cell::from("plain"), Cell::from("a,b"), Cell::from("say \"hi\""), Cell::from("two\nlines")],
            vec![Cell::from("short")],
        ]);
        let d = Dialect { line_ending: LineEnding::Lf, ..Dialect::default() };
        let (out, _) = write(&t, &d);
        assert_eq!(String::from_utf8(out).unwrap(), "plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\"\nshort,,,\n");
        let d = Dialect { pad_rows: false, trailing_newline: false, quoting: Quoting::All, ..d };
        let (out, _) = write(&t, &d);
        assert_eq!(String::from_utf8(out).unwrap(), "\"plain\",\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\"\n\"short\"");
        assert_eq!(write_row(&[Cell::from("a\tb"), Cell::from("c")], '\t', '"'), "\"a\tb\"\tc");
    }

    #[test]
    fn clipboard_blocks_round_trip() {
        let rows = vec![vec![Cell::from("a"), Cell::from("tab\there")], vec![Cell::from("line\nbreak"), Cell::from("q\"q")]];
        let text = block_text(&rows);
        assert_eq!(text, "a\t\"tab\there\"\r\n\"line\nbreak\"\t\"q\"\"q\"");
        assert_eq!(parse_block(&text), rows);
        assert_eq!(parse_block("plain"), vec![vec![Cell::from("plain")]]);
        assert_eq!(parse_block("x\ty\n1\t2\n"), vec![vec![Cell::from("x"), Cell::from("y")], vec![Cell::from("1"), Cell::from("2")]]);
        assert!(parse_block("").is_empty());
    }

    #[test]
    fn lossy_encoding_is_counted() {
        let t = Table::from_rows(vec![vec![Cell::from("日本")]]);
        let d = Dialect { encoding: Encoding::Windows1252, ..Dialect::default() };
        assert_eq!(write(&t, &d).1, 2);
    }

    #[test]
    fn refuses_oversized_input_by_length_alone() {
        assert!(check_size(MAX_INPUT_BYTES).is_ok());
        assert!(matches!(check_size(MAX_INPUT_BYTES + 1), Err(Error::TooLarge(_))));
    }
}
