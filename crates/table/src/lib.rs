//! The CSV editor's table model.
//!
//! Every cell is text. Nothing here knows about egui: this crate is the part that is unit-tested
//! exhaustively and could sit under any interface. See `.agents/plans/csv-editor.md`.

pub mod cleanup;
pub mod dialect;
pub mod document;
pub mod edit;
pub mod io;
pub mod search;
pub mod selection;
pub mod sort;
pub mod table;
pub mod text;

pub use cleanup::Case;
pub use dialect::{Dialect, Encoding, LineEnding, Quoting};
pub use document::Document;
pub use edit::{Edit, History};
pub use io::{Error, ParseOptions};
pub use search::{FindOptions, Matcher};
pub use selection::{CellRef, Range, Selection};
pub use sort::{SortKind, SortSpec};
pub use table::{Cell, Row, Table};

/// Spreadsheet-style column label: 0 → `A`, 25 → `Z`, 26 → `AA`.
pub fn column_label(index: usize) -> String {
    let mut s = Vec::new();
    let mut i = index;
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8_lossy(&s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_labels_follow_the_spreadsheet_convention() {
        assert_eq!(column_label(0), "A");
        assert_eq!(column_label(25), "Z");
        assert_eq!(column_label(26), "AA");
        assert_eq!(column_label(27), "AB");
        assert_eq!(column_label(701), "ZZ");
        assert_eq!(column_label(702), "AAA");
    }
}
