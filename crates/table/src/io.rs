//! Reading and writing delimited text.
//!
//! The parser is RFC 4180 with the usual leniency (a quote inside an unquoted field is literal,
//! text after a closing quote is kept, a file may end without a line break, lone CR line ends are
//! accepted). It works on bytes: the delimiter, quote and line-break characters are ASCII, so
//! field boundaries always fall on character boundaries of the decoded UTF-8 text.

use crate::dialect::{Dialect, Quoting};
use crate::table::{Cell, Row, Table};
use crate::text::{decode, encode, sniff_delimiter, sniff_line_ending};

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
    let quoting = if stats.fields > 0 && stats.quoted_fields == stats.fields { Quoting::All } else { Quoting::Minimal };
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
    while i < n {
        // One field.
        let quoted = b.get(i) == Some(&quote);
        let field: Cell = if quoted {
            i += 1;
            let mut s = String::new();
            let mut start = i;
            loop {
                match b.get(i) {
                    None => {
                        s.push_str(text.get(start..i).unwrap_or(""));
                        break;
                    }
                    Some(&c) if c == quote => {
                        s.push_str(text.get(start..i).unwrap_or(""));
                        if b.get(i + 1) == Some(&quote) {
                            s.push(char::from(quote));
                            i += 2;
                            start = i;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(_) => i += 1,
                }
            }
            // Lenient: anything between the closing quote and the next delimiter is kept.
            let tail = i;
            while let Some(&c) = b.get(i) {
                if is_break(c) {
                    break;
                }
                i += 1;
            }
            s.push_str(text.get(tail..i).unwrap_or(""));
            Cell::from(s)
        } else {
            let start = i;
            while let Some(&c) = b.get(i) {
                if is_break(c) {
                    break;
                }
                i += 1;
            }
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
                    on_record(std::mem::take(&mut record));
                    stats.ended_with_terminator = false;
                }
            }
            Some(&c) if c == b'\r' || c == b'\n' => {
                i += 1;
                if c == b'\r' && b.get(i) == Some(&b'\n') {
                    i += 1;
                }
                on_record(std::mem::take(&mut record));
                stats.ended_with_terminator = true;
            }
            None => {
                on_record(std::mem::take(&mut record));
                stats.ended_with_terminator = false;
            }
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

/// Writes the table in the dialect. The second value counts characters the encoding could not
/// represent (see [`encode`]).
pub fn write(table: &Table, d: &Dialect) -> (Vec<u8>, usize) {
    let delim = char::from(d.delimiter);
    let q = char::from(d.quote);
    let nl = d.line_ending.as_str();
    let rows = table.row_count();
    let mut out = String::with_capacity(rows.saturating_mul(16).min(1 << 28));
    for (ri, row) in table.rows().iter().enumerate() {
        let count = if d.pad_rows { table.width().max(row.len()) } else { row.len() };
        for c in 0..count {
            if c > 0 {
                out.push(delim);
            }
            write_field(&mut out, row.get(c).map_or("", |s| s.as_str()), delim, q, d.quoting);
        }
        if ri + 1 < rows || d.trailing_newline {
            out.push_str(nl);
        }
    }
    encode(&out, d.encoding)
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
    let needs = quoting == Quoting::All || text.chars().any(|ch| ch == delim || ch == q || ch == '\r' || ch == '\n');
    if needs {
        out.push(q);
        for ch in text.chars() {
            if ch == q {
                out.push(q);
            }
            out.push(ch);
        }
        out.push(q);
    } else {
        out.push_str(text);
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
        assert_eq!(d.quoting, Quoting::Minimal);
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
        let samples: [&[u8]; 10] = [
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
