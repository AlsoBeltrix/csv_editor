//! How a delimited-text file is laid out on disk: detected on open, kept, applied on save.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Crlf,
    Lf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Crlf => "\r\n",
            LineEnding::Lf => "\n",
            LineEnding::Cr => "\r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Crlf => "CRLF",
            LineEnding::Lf => "LF",
            LineEnding::Cr => "CR",
        }
    }

    pub const ALL: [LineEnding; 3] = [LineEnding::Crlf, LineEnding::Lf, LineEnding::Cr];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
        }
    }

    pub const ALL: [Encoding; 5] = [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Windows1252];
}

/// When fields are wrapped in quotes on save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoting {
    /// Untouched fields as they were in the file; new or edited text only when it needs quotes.
    AsSource,
    /// Only fields that contain the delimiter, the quote character or a line break.
    Minimal,
    /// Every field.
    All,
}

impl Quoting {
    pub const ALL: [Quoting; 3] = [Quoting::AsSource, Quoting::Minimal, Quoting::All];

    pub fn label(self) -> &'static str {
        match self {
            Quoting::AsSource => "As in the file (edited text only when needed)",
            Quoting::Minimal => "Only when needed",
            Quoting::All => "Every field",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialect {
    pub delimiter: u8,
    pub quote: u8,
    pub line_ending: LineEnding,
    pub encoding: Encoding,
    pub quoting: Quoting,
    /// The file ends with a line terminator after the last record.
    pub trailing_newline: bool,
    /// Rows are written with exactly the table's width in fields (empty ones added). Off for files
    /// that were ragged when opened, so untouched short rows are written back as they were.
    pub pad_rows: bool,
    /// The first row names the columns.
    pub header: bool,
}

impl Default for Dialect {
    fn default() -> Self {
        Dialect {
            delimiter: b',',
            quote: b'"',
            line_ending: LineEnding::Crlf,
            encoding: Encoding::Utf8,
            quoting: Quoting::AsSource,
            trailing_newline: true,
            pad_rows: true,
            header: true,
        }
    }
}

impl Dialect {
    pub const DELIMITERS: [(u8, &'static str); 4] = [(b',', "Comma"), (b';', "Semicolon"), (b'\t', "Tab"), (b'|', "Pipe")];

    pub fn delimiter_label(&self) -> String {
        Self::DELIMITERS
            .iter()
            .find(|(d, _)| *d == self.delimiter)
            .map(|(_, l)| (*l).to_string())
            .unwrap_or_else(|| format!("{:?}", char::from(self.delimiter)))
    }
}
