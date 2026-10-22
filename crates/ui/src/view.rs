//! Per-document interface state: selection, scroll position, column widths, row heights, row
//! filters, the frozen top row.
//!
//! Every table row is in the grid, as in a spreadsheet: row 1 is row 1. "First row is header"
//! only tells sort, filters and the cleanup tools to leave row 1 alone, and lets menus name
//! columns by it. Freezing the top row is a separate view setting.
//!
//! Row filters are a view: hidden rows stay in the table (edits, saves and sorts see them), the
//! grid just does not show them. The grid scrolls through *display rows*: the visible rows in
//! order, followed by blank rows past the table.

use std::collections::HashMap;
use std::rc::Rc;

use csv_editor_table::{CellRef, Document, Range, Selection};

/// Blank rows kept scrollable past the table's last row, so there is always room to type into.
pub const EXTRA_ROWS: usize = 30;
/// Blank columns kept past the table's last column.
pub const EXTRA_COLS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Contains,
    NotContains,
    Equals,
    Blank,
    NotBlank,
}

impl FilterKind {
    pub const ALL: [FilterKind; 5] = [FilterKind::Contains, FilterKind::NotContains, FilterKind::Equals, FilterKind::Blank, FilterKind::NotBlank];

    pub fn label(self) -> &'static str {
        match self {
            FilterKind::Contains => "contains",
            FilterKind::NotContains => "does not contain",
            FilterKind::Equals => "equals",
            FilterKind::Blank => "is blank",
            FilterKind::NotBlank => "is not blank",
        }
    }

    pub fn takes_text(self) -> bool {
        matches!(self, FilterKind::Contains | FilterKind::NotContains | FilterKind::Equals)
    }
}

/// One column's row filter. Text comparisons ignore case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub col: usize,
    pub kind: FilterKind,
    pub text: String,
}

impl Filter {
    pub fn passes(&self, cell: &str) -> bool {
        self.prepared().passes(cell)
    }

    /// The filter with its needle lowercased once, for a pass over many rows.
    pub fn prepared(&self) -> Prepared<'_> {
        Prepared { filter: self, needle: self.text.trim().to_lowercase() }
    }

    pub fn describe(&self) -> String {
        if self.kind.takes_text() { format!("{} \"{}\"", self.kind.label(), self.text) } else { self.kind.label().to_string() }
    }
}

/// A filter ready to test rows: the needle is lowercased once, and ASCII cells (most of them)
/// are compared without allocating. A filter runs over every row after every edit, so this is
/// what keeps editing a filtered large file smooth.
pub struct Prepared<'a> {
    filter: &'a Filter,
    needle: String,
}

impl Prepared<'_> {
    pub fn passes(&self, cell: &str) -> bool {
        match self.filter.kind {
            FilterKind::Blank => cell.trim().is_empty(),
            FilterKind::NotBlank => !cell.trim().is_empty(),
            FilterKind::Contains => contains_ci(cell, &self.needle),
            FilterKind::NotContains => !contains_ci(cell, &self.needle),
            FilterKind::Equals => equals_ci(cell.trim(), &self.needle),
        }
    }
}

/// Whether `text` contains `needle_lower` (already lowercased), ignoring case.
fn contains_ci(text: &str, needle_lower: &str) -> bool {
    if text.is_ascii() && needle_lower.is_ascii() {
        let n = needle_lower.len();
        n == 0 || text.as_bytes().windows(n).any(|w| w.eq_ignore_ascii_case(needle_lower.as_bytes()))
    } else {
        text.to_lowercase().contains(needle_lower)
    }
}

/// Whether `text` is `needle_lower` (already lowercased), ignoring case.
fn equals_ci(text: &str, needle_lower: &str) -> bool {
    if text.is_ascii() && needle_lower.is_ascii() { text.eq_ignore_ascii_case(needle_lower) } else { text.to_lowercase() == needle_lower }
}

pub struct DocView {
    pub doc: Document,
    pub sel: Selection,
    /// The display row shown at the top of the scrolling body (0 = the first row).
    pub scroll: usize,
    /// The column shown at the left of the body.
    pub scroll_col: usize,
    /// Column widths in unzoomed points, where the user or autofit set one.
    pub col_widths: HashMap<usize, f32>,
    /// Row heights in unzoomed points, where the user or autofit set one.
    pub row_heights: HashMap<usize, f32>,
    /// Keep the first row on screen while scrolling.
    pub freeze_top: bool,
    pub filters: Vec<Filter>,
    /// Rows that pass the filters, in order (the header row always included); `None` without
    /// filters.
    visible: Option<Rc<Vec<usize>>>,
    /// What `visible` was computed for: the document version and the filter generation.
    visible_key: (u64, u64),
    filter_gen: u64,
}

impl DocView {
    pub fn new(doc: Document) -> DocView {
        DocView {
            doc,
            sel: Selection::default(),
            scroll: 0,
            scroll_col: 0,
            col_widths: HashMap::new(),
            row_heights: HashMap::new(),
            freeze_top: false,
            filters: Vec::new(),
            visible: None,
            visible_key: (0, 0),
            filter_gen: 0,
        }
    }

    /// The first table row the grid shows: always 0, every row is in the grid.
    pub fn body_start(&self) -> usize {
        0
    }

    /// The first data row: 1 when the first row is the header (sort, filters and cleanup leave
    /// it alone), else 0.
    pub fn data_start(&self) -> usize {
        usize::from(self.doc.dialect.header)
    }

    /// Rows the grid lets you scroll to and select, blank ones past the data included.
    pub fn extent_rows(&self) -> usize {
        self.doc.table.row_count().saturating_add(EXTRA_ROWS)
    }

    pub fn extent_cols(&self) -> usize {
        self.doc.table.width().max(1).saturating_add(EXTRA_COLS)
    }

    /// The bottom-right selectable cell.
    pub fn max_cell(&self) -> CellRef {
        CellRef::new(self.extent_rows() - 1, self.extent_cols() - 1)
    }

    /// The column's name from the header row, or empty when there is no header row.
    pub fn header_name(&self, col: usize) -> &str {
        if self.doc.dialect.header { self.doc.table.get(0, col) } else { "" }
    }

    /// The column's label for menus: its letter, and its header name when it has one.
    pub fn column_title(&self, col: usize) -> String {
        let letter = csv_editor_table::column_label(col);
        let name = self.header_name(col);
        if name.is_empty() { letter } else { format!("{letter} ({name})") }
    }

    /// Turns the "first row is header" setting on or off.
    pub fn set_header(&mut self, header: bool) {
        self.doc.dialect.header = header;
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    /// Keeps the selection inside the grid after the table shrank (undo, deletions).
    pub fn clamp_selection(&mut self) {
        let max = self.max_cell();
        let clamp = |c: CellRef| CellRef::new(c.row.min(max.row), c.col.min(max.col));
        self.sel.active = clamp(self.sel.active);
        self.sel.anchor = clamp(self.sel.anchor);
        for r in &mut self.sel.ranges {
            *r = Range::new(clamp(r.start), clamp(r.end));
        }
    }

    /// Whether this is an untouched, unnamed, empty document (the one a first Open replaces).
    pub fn is_blank(&self) -> bool {
        self.doc.path.is_none() && self.doc.table.is_empty() && !self.doc.is_dirty() && !self.doc.history.can_undo()
    }

    // ----- Filters and display rows -----------------------------------------------------------

    /// Replaces the filter on `filter.col` (or adds one).
    pub fn set_filter(&mut self, filter: Filter) {
        self.filters.retain(|f| f.col != filter.col);
        self.filters.push(filter);
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn clear_filter(&mut self, col: usize) {
        self.filters.retain(|f| f.col != col);
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear();
        self.filter_gen = self.filter_gen.wrapping_add(1);
    }

    pub fn filtered(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Brings the visible-row list up to date with the table and the filters. Cheap when nothing
    /// changed; the grid calls it every frame.
    pub fn refresh_visible(&mut self) {
        if self.filters.is_empty() {
            self.visible = None;
            return;
        }
        let key = (self.doc.history.version(), self.filter_gen);
        if self.visible.is_some() && self.visible_key == key {
            return;
        }
        let table = &self.doc.table;
        let start = self.data_start().min(table.row_count());
        let mut rows: Vec<usize> = (0..start).collect();
        let prepared: Vec<(usize, Prepared<'_>)> = self.filters.iter().map(|f| (f.col, f.prepared())).collect();
        rows.extend((start..table.row_count()).filter(|&r| prepared.iter().all(|(col, f)| f.passes(table.get(r, *col)))));
        self.visible = Some(Rc::new(rows));
        self.visible_key = key;
    }

    /// Data rows the grid shows (the header row not counted).
    pub fn visible_count(&self) -> usize {
        match &self.visible {
            Some(v) => v.len().saturating_sub(self.data_start()),
            None => self.doc.table.row_count().saturating_sub(self.data_start()),
        }
    }

    /// Display rows: the visible rows, then the blank rows past the table.
    pub fn display_count(&self) -> usize {
        match &self.visible {
            Some(v) => v.len().saturating_add(EXTRA_ROWS),
            None => self.extent_rows(),
        }
    }

    /// The table row shown at display row `i`; past the visible rows, the blank rows after the
    /// table in order.
    pub fn row_at_display(&self, i: usize) -> usize {
        match &self.visible {
            Some(v) => match v.get(i) {
                Some(&r) => r,
                None => self.doc.table.row_count().saturating_add(i - v.len()),
            },
            None => i,
        }
    }

    /// Where table row `r` is displayed; for a hidden row, where the next visible row is.
    pub fn display_of_row(&self, r: usize) -> usize {
        match &self.visible {
            Some(v) => {
                let rows = self.doc.table.row_count();
                if r >= rows {
                    return v.len().saturating_add(r - rows);
                }
                match v.binary_search(&r) {
                    Ok(i) => i,
                    Err(i) => i.min(v.len()),
                }
            }
            None => r,
        }
    }

    /// A row-visibility test that owns what it needs, for calls that also borrow the document
    /// mutably (replace all).
    pub fn visibility(&self) -> Box<dyn Fn(usize) -> bool> {
        let rows = self.doc.table.row_count();
        match &self.visible {
            Some(v) => {
                let v = Rc::clone(v);
                Box::new(move |r| r >= rows || v.binary_search(&r).is_ok())
            }
            None => Box::new(|_| true),
        }
    }

    pub fn is_row_visible(&self, r: usize) -> bool {
        match &self.visible {
            Some(v) => r >= self.doc.table.row_count() || v.binary_search(&r).is_ok(),
            None => true,
        }
    }

    /// `r` itself if it is shown, otherwise the nearest displayed row after it (or before, at
    /// the end).
    pub fn snap_visible(&self, r: usize) -> usize {
        if self.is_row_visible(r) {
            return r;
        }
        let d = self.display_of_row(r);
        self.row_at_display(d.min(self.display_count().saturating_sub(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_editor_table::ParseOptions;

    fn view(text: &str) -> DocView {
        DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap())
    }

    #[test]
    fn filters_match_text_and_blanks() {
        let f = |kind, text: &str| Filter { col: 0, kind, text: text.to_string() };
        assert!(f(FilterKind::Contains, "APP").passes("pineapple"));
        assert!(!f(FilterKind::NotContains, "APP").passes("pineapple"));
        assert!(f(FilterKind::Equals, " Oslo ").passes("oslo"));
        assert!(!f(FilterKind::Equals, "Oslo").passes("Oslo city"));
        assert!(f(FilterKind::Blank, "").passes("  ") && !f(FilterKind::Blank, "").passes("x"));
        assert!(f(FilterKind::NotBlank, "").passes("x"));
        // Non-ASCII goes through the lowercase path; ASCII through the byte path.
        assert!(f(FilterKind::Contains, "ZÜRICH").passes("in zürich"));
        assert!(f(FilterKind::Equals, "zürich").passes(" Zürich "));
        assert!(!f(FilterKind::Contains, "zürich").passes("zurich"));
        assert!(f(FilterKind::Contains, "").passes("anything"));
        assert!(!f(FilterKind::Contains, "longer than the cell").passes("short"));
        assert_eq!(f(FilterKind::Contains, "a").describe(), "contains \"a\"");
        assert_eq!(f(FilterKind::Blank, "").describe(), "is blank");
    }

    #[test]
    fn display_rows_keep_the_header_skip_hidden_rows_and_continue_past_the_table() {
        let mut v = view("city\nOslo\nBerlin\nOslo\nLisbon\n");
        assert_eq!(v.data_start(), 1);
        assert_eq!(v.display_count(), 5 + EXTRA_ROWS);
        assert_eq!(v.row_at_display(0), 0, "row 1 is in the grid");
        assert_eq!(v.display_of_row(3), 3);
        v.set_filter(Filter { col: 0, kind: FilterKind::Contains, text: "os".to_string() });
        v.refresh_visible();
        assert!(v.filtered());
        assert_eq!(v.visible_count(), 2, "Oslo twice; the header row is not counted");
        assert_eq!((v.row_at_display(0), v.row_at_display(1), v.row_at_display(2)), (0, 1, 3));
        assert_eq!(v.row_at_display(3), 5, "then the blank rows after the table");
        assert_eq!(v.display_of_row(3), 2);
        assert_eq!(v.display_of_row(2), 2, "a hidden row maps to the next visible one");
        assert!(v.is_row_visible(0), "the header row is never filtered out");
        assert!(!v.is_row_visible(2) && v.is_row_visible(3) && v.is_row_visible(9));
        assert_eq!(v.snap_visible(2), 3);
        assert_eq!(v.snap_visible(4), 5, "after the last visible row comes the first blank one");
        // Editing a filtered column re-evaluates the filter.
        v.doc.set_cell(2, 0, "Oslo");
        v.refresh_visible();
        assert_eq!(v.visible_count(), 3);
        // Without a header flag, row 1 is data and can be hidden too.
        v.set_header(false);
        v.refresh_visible();
        assert!(!v.is_row_visible(0));
        v.clear_filters();
        v.refresh_visible();
        assert!(!v.filtered());
        assert_eq!(v.row_at_display(1), 1);
    }

    #[test]
    fn column_titles_include_header_names() {
        let v = view("id,name\n1,Ann\n");
        assert_eq!(v.column_title(1), "B (name)");
        assert_eq!(v.column_title(5), "F");
    }
}
