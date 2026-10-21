//! Edits and their inverses: the undo system.
//!
//! Applying an [`Edit`] to a table returns the edit that undoes it, with whatever state that
//! needs (deleted rows, replaced texts) captured at that moment. Undo applies the inverse and
//! keeps *its* inverse for redo. Nothing snapshots the table, so a 100 MB file stays editable.

use crate::table::{Cell, Row, Table, invert_permutation};

/// What a block write must put back to leave no trace: the rows' lengths before the write (it may
/// have padded short rows) and the table's width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restore {
    pub row_lens: Vec<usize>,
    pub width: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Nothing (what an edit that did not fit the table applied as).
    Nop,
    /// Writes a block of texts with its top-left corner at (`row`, `col`). Rows that do not exist
    /// are skipped; short rows are padded.
    SetRect {
        row: usize,
        col: usize,
        cells: Vec<Row>,
        restore: Option<Restore>,
    },
    InsertRows {
        at: usize,
        rows: Vec<Row>,
    },
    DeleteRows {
        at: usize,
        count: usize,
    },
    /// `cells` restores what a deletion removed from each row; `None` inserts empty cells.
    InsertCols {
        at: usize,
        count: usize,
        cells: Option<Vec<Row>>,
    },
    DeleteCols {
        at: usize,
        count: usize,
    },
    MoveRows {
        from: usize,
        count: usize,
        to: usize,
    },
    /// `trim` restores the rows' lengths a previous column move padded (undo only).
    MoveCols {
        from: usize,
        count: usize,
        to: usize,
        trim: Option<Vec<usize>>,
    },
    /// New row `i` is old row `perm[i]` (sorting).
    Permute {
        perm: Vec<usize>,
    },
    /// Several edits as one undo step, applied in order.
    Batch(Vec<Edit>),
}

impl Edit {
    pub fn set_cell(row: usize, col: usize, text: impl Into<Cell>) -> Edit {
        Edit::SetRect { row, col, cells: vec![vec![text.into()]], restore: None }
    }

    pub fn set_rect(row: usize, col: usize, cells: Vec<Row>) -> Edit {
        Edit::SetRect { row, col, cells, restore: None }
    }

    pub fn is_nop(&self) -> bool {
        match self {
            Edit::Nop => true,
            Edit::Batch(v) => v.iter().all(Edit::is_nop),
            _ => false,
        }
    }

    /// Applies the edit and returns its inverse.
    pub fn apply(self, t: &mut Table) -> Edit {
        match self {
            Edit::Nop => Edit::Nop,
            Edit::SetRect { row, col, cells, restore } => {
                let old_width = t.width();
                let mut old_cells = Vec::with_capacity(cells.len());
                let mut row_lens = Vec::with_capacity(cells.len());
                let mut touched = false;
                for (i, texts) in cells.into_iter().enumerate() {
                    let r = row.saturating_add(i);
                    let mut old_row = Vec::with_capacity(texts.len());
                    let mut old_len = None;
                    for (j, text) in texts.into_iter().enumerate() {
                        match t.set(r, col.saturating_add(j), text) {
                            Some((old, len)) => {
                                old_row.push(old);
                                old_len.get_or_insert(len);
                                touched = true;
                            }
                            None => old_row.push(Cell::default()),
                        }
                    }
                    row_lens.push(old_len.unwrap_or(0));
                    old_cells.push(old_row);
                }
                if !touched {
                    return Edit::Nop;
                }
                if let Some(rs) = restore {
                    for (i, len) in rs.row_lens.iter().enumerate() {
                        t.truncate_row(row.saturating_add(i), *len);
                    }
                    t.set_width(rs.width);
                }
                Edit::SetRect { row, col, cells: old_cells, restore: Some(Restore { row_lens, width: old_width }) }
            }
            Edit::InsertRows { at, rows } => {
                let count = rows.len();
                if count > 0 && t.insert_rows(at, rows) { Edit::DeleteRows { at, count } } else { Edit::Nop }
            }
            Edit::DeleteRows { at, count } => match t.remove_rows(at, count) {
                Some(rows) if !rows.is_empty() => Edit::InsertRows { at, rows },
                _ => Edit::Nop,
            },
            Edit::InsertCols { at, count, cells } => {
                if t.insert_cols(at, count, cells.as_deref()) {
                    Edit::DeleteCols { at, count }
                } else {
                    Edit::Nop
                }
            }
            Edit::DeleteCols { at, count } => match t.remove_cols(at, count) {
                Some(cells) => Edit::InsertCols { at, count, cells: Some(cells) },
                None => Edit::Nop,
            },
            Edit::MoveRows { from, count, to } => match t.move_rows(from, count, to) {
                Some(new_from) => Edit::MoveRows { from: new_from, count, to: if to > from { from } else { from + count } },
                None => Edit::Nop,
            },
            Edit::MoveCols { from, count, to, trim } => match t.move_cols(from, count, to) {
                Some((new_from, old_lens)) => {
                    if let Some(lens) = trim {
                        for (r, len) in lens.iter().enumerate() {
                            t.truncate_row(r, *len);
                        }
                    }
                    Edit::MoveCols { from: new_from, count, to: if to > from { from } else { from + count }, trim: Some(old_lens) }
                }
                None => Edit::Nop,
            },
            Edit::Permute { perm } => {
                if t.permute_rows(&perm) {
                    Edit::Permute { perm: invert_permutation(&perm) }
                } else {
                    Edit::Nop
                }
            }
            Edit::Batch(edits) => {
                let mut inverses: Vec<Edit> = edits.into_iter().map(|e| e.apply(t)).filter(|e| !e.is_nop()).collect();
                inverses.reverse();
                match inverses.len() {
                    0 => Edit::Nop,
                    1 => inverses.pop().unwrap_or(Edit::Nop),
                    _ => Edit::Batch(inverses),
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    label: String,
    inverse: Edit,
}

/// Undo and redo stacks, and the saved mark that decides whether the document is dirty.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_id: u64,
    saved: u64,
}

impl History {
    /// Applies the edit as one undo step named `label` ("Typing", "Delete rows"…). Returns false,
    /// recording nothing, when the edit changed nothing.
    pub fn apply(&mut self, t: &mut Table, label: impl Into<String>, edit: Edit) -> bool {
        let inverse = edit.apply(t);
        if inverse.is_nop() {
            return false;
        }
        self.next_id = self.next_id.wrapping_add(1);
        self.undo.push(Entry { id: self.next_id, label: label.into(), inverse });
        self.redo.clear();
        true
    }

    pub fn undo(&mut self, t: &mut Table) -> Option<String> {
        let e = self.undo.pop()?;
        let redo = e.inverse.apply(t);
        self.redo.push(Entry { id: e.id, label: e.label.clone(), inverse: redo });
        Some(e.label)
    }

    pub fn redo(&mut self, t: &mut Table) -> Option<String> {
        let e = self.redo.pop()?;
        let undo = e.inverse.apply(t);
        self.undo.push(Entry { id: e.id, label: e.label.clone(), inverse: undo });
        Some(e.label)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Identifies the table's state: the id of the last applied edit, 0 for the original.
    pub fn version(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.id)
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.version();
    }

    pub fn is_dirty(&self) -> bool {
        self.version() != self.saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::table_of;

    fn roundtrip(t: &Table, edit: Edit) -> Table {
        let mut work = t.clone();
        let inverse = edit.apply(&mut work);
        assert_ne!(&work, t, "the edit must change the table");
        let redo = inverse.apply(&mut work);
        assert_eq!(&work, t, "the inverse must restore the table exactly");
        assert!(!redo.is_nop());
        work
    }

    #[test]
    fn every_edit_inverts_exactly() {
        let t = table_of(&[&["a", "b", "c"], &["d"], &["e", "f", "g"]]);
        roundtrip(&t, Edit::set_cell(0, 1, "X"));
        roundtrip(&t, Edit::set_cell(1, 2, "pad"));
        roundtrip(&t, Edit::set_cell(1, 5, "wider"));
        roundtrip(&t, Edit::set_rect(0, 0, vec![vec![Cell::from("1"), Cell::from("2")], vec![Cell::from("3"), Cell::from("4")]]));
        roundtrip(&t, Edit::InsertRows { at: 1, rows: vec![vec![Cell::from("new")]] });
        roundtrip(&t, Edit::DeleteRows { at: 0, count: 2 });
        roundtrip(&t, Edit::InsertCols { at: 1, count: 2, cells: None });
        roundtrip(&t, Edit::InsertCols { at: 3, count: 1, cells: None });
        roundtrip(&t, Edit::DeleteCols { at: 0, count: 2 });
        roundtrip(&t, Edit::DeleteCols { at: 2, count: 1 });
        roundtrip(&t, Edit::MoveRows { from: 0, count: 1, to: 3 });
        roundtrip(&t, Edit::MoveRows { from: 2, count: 1, to: 0 });
        roundtrip(&t, Edit::Permute { perm: vec![2, 0, 1] });
        roundtrip(
            &t,
            Edit::Batch(vec![Edit::set_cell(0, 0, "q"), Edit::DeleteRows { at: 1, count: 1 }, Edit::InsertCols { at: 0, count: 1, cells: None }]),
        );
        // Column moves pad short rows; their inverse trims them again.
        roundtrip(&t, Edit::MoveCols { from: 0, count: 2, to: 3, trim: None });
        roundtrip(&t, Edit::MoveCols { from: 2, count: 1, to: 0, trim: None });
    }

    #[test]
    fn edits_that_do_not_fit_are_nops() {
        let t = table_of(&[&["a"]]);
        for edit in [
            Edit::set_cell(5, 0, "x"),
            Edit::DeleteRows { at: 0, count: 2 },
            Edit::DeleteCols { at: 3, count: 1 },
            Edit::MoveRows { from: 0, count: 1, to: 1 },
            Edit::Permute { perm: vec![1] },
            Edit::InsertRows { at: 0, rows: vec![] },
            Edit::Batch(vec![Edit::Nop]),
        ] {
            let mut work = t.clone();
            assert!(edit.clone().apply(&mut work).is_nop(), "{edit:?}");
            assert_eq!(work, t);
        }
    }

    #[test]
    fn history_tracks_dirty_state_across_undo_and_redo() {
        let mut t = table_of(&[&["a"]]);
        let mut h = History::default();
        assert!(!h.is_dirty());
        assert!(h.apply(&mut t, "Typing", Edit::set_cell(0, 0, "b")));
        assert!(!h.apply(&mut t, "Typing", Edit::set_cell(9, 0, "b")), "a no-op records nothing");
        assert!(h.is_dirty());
        assert_eq!(h.undo_label(), Some("Typing"));
        h.mark_saved();
        assert!(!h.is_dirty());
        assert_eq!(h.undo(&mut t), Some("Typing".to_string()));
        assert_eq!(t.get(0, 0), "a");
        assert!(h.is_dirty());
        assert_eq!(h.redo(&mut t), Some("Typing".to_string()));
        assert_eq!(t.get(0, 0), "b");
        assert!(!h.is_dirty());
        h.undo(&mut t);
        h.apply(&mut t, "Typing", Edit::set_cell(0, 0, "c"));
        assert!(!h.can_redo(), "a new edit drops the redo stack");
        assert!(h.is_dirty());
        assert_eq!(h.undo(&mut t), Some("Typing".to_string()));
        assert!(h.is_dirty(), "back at the original, which is not the saved state");
        assert_eq!(h.undo(&mut t), None);
    }

    #[test]
    fn random_edit_sequences_undo_back_to_the_start() {
        let original = table_of(&[&["a", "b", "c"], &["d", "e"], &["f", "g", "h"], &["i"]]);
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut rnd = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % (n.max(1) as u64)) as usize
        };
        for _ in 0..200 {
            let mut t = original.clone();
            let mut h = History::default();
            let steps = 1 + rnd(12);
            for _ in 0..steps {
                let rows = t.row_count();
                let width = t.width();
                let edit = match rnd(8) {
                    0 => Edit::set_cell(rnd(rows + 1), rnd(width + 2), "x"),
                    1 => Edit::InsertRows { at: rnd(rows + 1), rows: vec![vec![Cell::from("n")]; 1 + rnd(2)] },
                    2 => Edit::DeleteRows { at: rnd(rows + 1), count: 1 + rnd(2) },
                    3 => Edit::InsertCols { at: rnd(width + 1), count: 1 + rnd(2), cells: None },
                    4 => Edit::DeleteCols { at: rnd(width + 1), count: 1 + rnd(2) },
                    5 => Edit::MoveRows { from: rnd(rows + 1), count: 1 + rnd(2), to: rnd(rows + 1) },
                    6 => Edit::MoveCols { from: rnd(width + 1), count: 1 + rnd(2), to: rnd(width + 1), trim: None },
                    _ => {
                        let mut perm: Vec<usize> = (0..rows).collect();
                        perm.reverse();
                        Edit::Permute { perm }
                    }
                };
                h.apply(&mut t, "step", edit);
            }
            let applied = h.undo.len();
            while h.undo(&mut t).is_some() {}
            assert_eq!(t, original, "after undoing {applied} edits");
            while h.redo(&mut t).is_some() {}
            while h.undo(&mut t).is_some() {}
            assert_eq!(t, original, "after redoing and undoing {applied} edits again");
        }
    }
}
