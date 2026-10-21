//! Cleanup tools: whitespace, letter case, empty rows and columns, duplicate rows. Each is one
//! undo step and reports how many cells or rows it changed.

use std::collections::HashSet;

use crate::document::Document;
use crate::edit::Edit;
use crate::selection::Range;
use crate::table::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Title,
}

impl Case {
    pub const ALL: [Case; 3] = [Case::Upper, Case::Lower, Case::Title];

    pub fn label(self) -> &'static str {
        match self {
            Case::Upper => "UPPER CASE",
            Case::Lower => "lower case",
            Case::Title => "Title Case",
        }
    }

    pub fn apply(self, s: &str) -> String {
        match self {
            Case::Upper => s.to_uppercase(),
            Case::Lower => s.to_lowercase(),
            Case::Title => {
                let mut out = String::with_capacity(s.len());
                let mut word_start = true;
                for c in s.chars() {
                    if c.is_alphanumeric() {
                        if word_start {
                            out.extend(c.to_uppercase());
                        } else {
                            out.extend(c.to_lowercase());
                        }
                        word_start = false;
                    } else {
                        out.push(c);
                        word_start = true;
                    }
                }
                out
            }
        }
    }
}

/// Row indices as descending runs of deletions, so each deletion leaves the earlier indices valid.
fn delete_rows_edits(mut rows: Vec<usize>) -> Vec<Edit> {
    rows.sort_unstable();
    rows.dedup();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for r in rows {
        match runs.last_mut() {
            Some((start, count)) if *start + *count == r => *count += 1,
            _ => runs.push((r, 1)),
        }
    }
    runs.into_iter().rev().map(|(at, count)| Edit::DeleteRows { at, count }).collect()
}

impl Document {
    /// Rewrites every existing cell of `scope` with `f`, as one undo step named `label`. Returns
    /// how many cells changed.
    pub fn map_cells(&mut self, scope: Range, label: &str, f: impl Fn(&str) -> String) -> usize {
        let table = &self.table;
        let mut edits = Vec::new();
        for r in scope.rows().take_while(|&r| r < table.row_count()) {
            let Some(row) = table.row(r) else { break };
            for c in scope.cols().take_while(|&c| c < row.len()) {
                let old = table.get(r, c);
                let new = f(old);
                if new != old {
                    edits.push(Edit::set_cell(r, c, Cell::from(new)));
                }
            }
        }
        let n = edits.len();
        if n > 0 && self.apply(label, Edit::Batch(edits)) { n } else { 0 }
    }

    pub fn trim_whitespace(&mut self, scope: Range) -> usize {
        self.map_cells(scope, "Trim whitespace", |s| s.trim().to_string())
    }

    pub fn change_case(&mut self, scope: Range, case: Case) -> usize {
        self.map_cells(scope, case.label(), |s| case.apply(s))
    }

    /// Deletes body rows whose cells are all blank. Returns how many.
    pub fn remove_empty_rows(&mut self, body_start: usize) -> usize {
        let empty: Vec<usize> =
            (body_start..self.table.row_count()).filter(|&r| self.table.row(r).is_none_or(|row| row.iter().all(|c| c.trim().is_empty()))).collect();
        let n = empty.len();
        if n > 0 && self.apply("Remove empty rows", Edit::Batch(delete_rows_edits(empty))) { n } else { 0 }
    }

    /// Deletes columns that are blank in every row (the header row included). Returns how many.
    pub fn remove_empty_cols(&mut self) -> usize {
        let table = &self.table;
        let empty: Vec<usize> =
            (0..table.width()).filter(|&c| table.rows().iter().all(|row| row.get(c).is_none_or(|x| x.trim().is_empty()))).collect();
        let mut edits = Vec::new();
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for c in &empty {
            match runs.last_mut() {
                Some((start, count)) if *start + *count == *c => *count += 1,
                _ => runs.push((*c, 1)),
            }
        }
        for (at, count) in runs.into_iter().rev() {
            edits.push(Edit::DeleteCols { at, count });
        }
        let n = empty.len();
        if n > 0 && self.apply("Remove empty columns", Edit::Batch(edits)) { n } else { 0 }
    }

    /// Deletes body rows identical to an earlier row (trailing blank cells ignored). Returns how
    /// many.
    pub fn remove_duplicate_rows(&mut self, body_start: usize) -> usize {
        let mut seen: HashSet<Vec<&str>> = HashSet::new();
        let mut dupes = Vec::new();
        for (i, row) in self.table.rows().iter().enumerate().skip(body_start) {
            let mut key: Vec<&str> = row.iter().map(Cell::as_str).collect();
            while key.last().is_some_and(|c| c.is_empty()) {
                key.pop();
            }
            if !seen.insert(key) {
                dupes.push(i);
            }
        }
        let n = dupes.len();
        if n > 0 && self.apply("Remove duplicate rows", Edit::Batch(delete_rows_edits(dupes))) { n } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::ParseOptions;
    use crate::selection::CellRef;

    fn doc(text: &str) -> Document {
        Document::from_bytes(text.as_bytes(), &ParseOptions { header: Some(true), ..Default::default() }).unwrap()
    }

    fn text(d: &Document) -> String {
        String::from_utf8(d.to_bytes().0).unwrap()
    }

    fn all(d: &Document) -> Range {
        Range::new(CellRef::new(0, 0), CellRef::new(d.table.row_count().max(1) - 1, d.table.width().max(1) - 1))
    }

    #[test]
    fn cases() {
        assert_eq!(Case::Title.apply("hello wORLD-wide o'neil 3rd"), "Hello World-Wide O'Neil 3rd");
        assert_eq!(Case::Upper.apply("straße"), "STRASSE");
        assert_eq!(Case::Lower.apply("ÀB"), "àb");
    }

    #[test]
    fn trim_and_case_touch_only_changed_cells() {
        let mut d = doc("h\n a \nb\n  \n");
        assert_eq!(d.trim_whitespace(all(&d)), 2);
        assert_eq!(text(&d), "h\na\nb\n\n");
        assert_eq!(d.trim_whitespace(all(&d)), 0);
        assert_eq!(d.change_case(Range::new(CellRef::new(1, 0), CellRef::new(1, 0)), Case::Upper), 1);
        assert_eq!(text(&d), "h\nA\nb\n\n");
        d.undo();
        d.undo();
        assert_eq!(text(&d), "h\n a \nb\n  \n");
    }

    #[test]
    fn empty_rows_and_columns_are_removed_in_one_step() {
        let mut d = doc("h,,x\n,,\na,,b\n,,\n,,\nc,,d\n");
        assert_eq!(d.remove_empty_rows(1), 3);
        assert_eq!(text(&d), "h,,x\na,,b\nc,,d\n");
        assert_eq!(d.remove_empty_cols(), 1);
        assert_eq!(text(&d), "h,x\na,b\nc,d\n");
        assert_eq!(d.remove_empty_rows(1), 0);
        assert_eq!(d.undo(), Some("Remove empty columns".to_string()));
        assert_eq!(d.undo(), Some("Remove empty rows".to_string()));
        assert_eq!(text(&d), "h,,x\n,,\na,,b\n,,\n,,\nc,,d\n");
    }

    #[test]
    fn duplicate_rows_keep_the_first_occurrence() {
        let mut d = doc("h,i\na,1\nb,2\na,1\nb,2\nc,3\na,1\n");
        assert_eq!(d.remove_duplicate_rows(1), 3);
        assert_eq!(text(&d), "h,i\na,1\nb,2\nc,3\n");
        d.undo();
        assert_eq!(d.table.row_count(), 7);
        // Trailing blanks do not make a row different.
        let mut d = Document::from_bytes(b"a,\na\n", &ParseOptions { header: Some(false), ..Default::default() }).unwrap();
        assert_eq!(d.remove_duplicate_rows(0), 1);
    }

    #[test]
    fn descending_runs() {
        let edits = delete_rows_edits(vec![5, 1, 2, 3, 9, 9]);
        assert_eq!(edits, vec![Edit::DeleteRows { at: 9, count: 1 }, Edit::DeleteRows { at: 5, count: 1 }, Edit::DeleteRows { at: 1, count: 3 }]);
    }
}
