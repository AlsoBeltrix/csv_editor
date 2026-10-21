//! Cell selection: one or more rectangular ranges with an active cell.
//!
//! Adapted from GridCraft's `crates/engine/src/selection.rs` (MIT, see `ATTRIBUTION.md`), with
//! the grid's finite bounds passed in instead of a sheet's maximum.

use std::ops::RangeInclusive;

use crate::table::Table;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellRef {
    pub row: usize,
    pub col: usize,
}

impl CellRef {
    pub const fn new(row: usize, col: usize) -> CellRef {
        CellRef { row, col }
    }

    /// The cell `dr` rows and `dc` columns away, clamped to `0..=max`.
    pub fn offset(self, dr: i64, dc: i64, max: CellRef) -> CellRef {
        let clamp = |v: usize, d: i64, hi: usize| -> usize {
            let moved = i128::from(v as u64) + i128::from(d);
            moved.clamp(0, i128::from(hi as u64)) as usize
        };
        CellRef::new(clamp(self.row, dr, max.row), clamp(self.col, dc, max.col))
    }
}

/// An inclusive rectangle of cells, normalised so `start <= end` on both axes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub start: CellRef,
    pub end: CellRef,
}

impl Range {
    pub fn new(a: CellRef, b: CellRef) -> Range {
        Range { start: CellRef::new(a.row.min(b.row), a.col.min(b.col)), end: CellRef::new(a.row.max(b.row), a.col.max(b.col)) }
    }

    pub fn cell(c: CellRef) -> Range {
        Range { start: c, end: c }
    }

    pub fn rows(&self) -> RangeInclusive<usize> {
        self.start.row..=self.end.row
    }

    pub fn cols(&self) -> RangeInclusive<usize> {
        self.start.col..=self.end.col
    }

    pub fn row_count(&self) -> usize {
        self.end.row - self.start.row + 1
    }

    pub fn col_count(&self) -> usize {
        self.end.col - self.start.col + 1
    }

    pub fn is_single(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, c: CellRef) -> bool {
        self.rows().contains(&c.row) && self.cols().contains(&c.col)
    }

    pub fn contains_range(&self, o: &Range) -> bool {
        self.contains(o.start) && self.contains(o.end)
    }

    pub fn intersects(&self, o: &Range) -> bool {
        self.start.row <= o.end.row && o.start.row <= self.end.row && self.start.col <= o.end.col && o.start.col <= self.end.col
    }

    pub fn union(&self, o: &Range) -> Range {
        Range {
            start: CellRef::new(self.start.row.min(o.start.row), self.start.col.min(o.start.col)),
            end: CellRef::new(self.end.row.max(o.end.row), self.end.col.max(o.end.col)),
        }
    }

    /// The overlap with `o`, if any.
    pub fn intersection(&self, o: &Range) -> Option<Range> {
        self.intersects(o).then(|| Range {
            start: CellRef::new(self.start.row.max(o.start.row), self.start.col.max(o.start.col)),
            end: CellRef::new(self.end.row.min(o.end.row), self.end.col.min(o.end.col)),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The active cell (drawn without fill); edits with a single target go here.
    pub active: CellRef,
    /// Where a Shift-extension is anchored.
    pub anchor: CellRef,
    /// Selected areas; the last one contains the active cell. Never empty.
    pub ranges: Vec<Range>,
}

impl Default for Selection {
    fn default() -> Self {
        Selection::at(CellRef::default())
    }
}

impl Selection {
    pub fn at(c: CellRef) -> Selection {
        Selection { active: c, anchor: c, ranges: vec![Range::cell(c)] }
    }

    pub fn range(r: Range) -> Selection {
        Selection { active: r.start, anchor: r.start, ranges: vec![r] }
    }

    /// The current (last) area.
    pub fn current(&self) -> Range {
        self.ranges.last().copied().unwrap_or(Range::cell(self.active))
    }

    /// Bounding range of all areas.
    pub fn bounds(&self) -> Range {
        let mut it = self.ranges.iter();
        let first = it.next().copied().unwrap_or(Range::cell(self.active));
        it.fold(first, |a, b| a.union(b))
    }

    pub fn contains(&self, c: CellRef) -> bool {
        self.ranges.iter().any(|r| r.contains(c))
    }

    pub fn is_single_cell(&self) -> bool {
        self.ranges.len() == 1 && self.current().is_single()
    }

    /// The corner of the current area opposite the anchor: the end that Shift+arrows move.
    pub fn moving_end(&self) -> CellRef {
        let r = self.current();
        let row = if self.anchor.row == r.start.row { r.end.row } else { r.start.row };
        let col = if self.anchor.col == r.start.col { r.end.col } else { r.start.col };
        CellRef::new(row, col)
    }

    /// Extends the current area from the anchor to `to` (Shift+click / Shift+arrows).
    pub fn extend_to(&mut self, to: CellRef) {
        let r = Range::new(self.anchor, to);
        match self.ranges.last_mut() {
            Some(last) => *last = r,
            None => self.ranges.push(r),
        }
    }

    /// Adds an area (Ctrl+click) and makes `active` the active cell and anchor.
    pub fn add(&mut self, r: Range, active: CellRef) {
        self.ranges.push(r);
        self.active = active;
        self.anchor = active;
    }

    /// Whether a whole row is selected, given the grid's column count.
    pub fn covers_row(&self, row: usize, cols: usize) -> bool {
        self.ranges.iter().any(|r| r.rows().contains(&row) && r.start.col == 0 && r.end.col + 1 >= cols)
    }

    /// Whether a whole column is selected, given the grid's row count.
    pub fn covers_col(&self, col: usize, rows: usize) -> bool {
        self.ranges.iter().any(|r| r.cols().contains(&col) && r.start.row == 0 && r.end.row + 1 >= rows)
    }

    /// Moves the active cell within the selection (Enter/Tab inside a multi-cell selection).
    /// Returns false for a single cell, where the caller moves the selection instead.
    pub fn cycle(&mut self, forward: bool, by_row: bool) -> bool {
        let r = self.current();
        if r.is_single() && self.ranges.len() == 1 {
            return false;
        }
        let (mut row, mut col) = (self.active.row as i64, self.active.col as i64);
        let step: i64 = if forward { 1 } else { -1 };
        let (r0, r1, c0, c1) = (r.start.row as i64, r.end.row as i64, r.start.col as i64, r.end.col as i64);
        if by_row {
            col += step;
            if col > c1 {
                col = c0;
                row += 1;
            } else if col < c0 {
                col = c1;
                row -= 1;
            }
            if row > r1 {
                row = r0;
            } else if row < r0 {
                row = r1;
            }
        } else {
            row += step;
            if row > r1 {
                row = r0;
                col += 1;
            } else if row < r0 {
                row = r1;
                col -= 1;
            }
            if col > c1 {
                col = c0;
            } else if col < c0 {
                col = c1;
            }
        }
        self.active = CellRef::new(row.max(0) as usize, col.max(0) as usize);
        true
    }
}

/// Ctrl+arrow: the edge of the current data run, or the next filled cell, or the grid edge `max`.
pub fn jump(t: &Table, from: CellRef, dr: i64, dc: i64, max: CellRef) -> CellRef {
    if dr == 0 && dc == 0 {
        return from;
    }
    let filled = |c: CellRef| !t.get(c.row, c.col).is_empty();
    let step = |c: CellRef| {
        let n = c.offset(dr, dc, max);
        (n != c).then_some(n)
    };
    let Some(mut cur) = step(from) else { return from };
    if filled(from) && filled(cur) {
        while let Some(n) = step(cur) {
            if !filled(n) {
                break;
            }
            cur = n;
        }
        return cur;
    }
    // Past the table's extent everything is empty: go straight to the edge.
    loop {
        if filled(cur) {
            return cur;
        }
        let beyond = (dr > 0 && cur.row >= t.row_count()) || (dc > 0 && cur.col >= t.width());
        if beyond {
            return CellRef::new(if dr > 0 { max.row } else { cur.row }, if dc > 0 { max.col } else { cur.col });
        }
        match step(cur) {
            Some(n) => cur = n,
            None => return cur,
        }
    }
}

/// The contiguous block of filled cells around `c` (Ctrl+A, sort), within the table's extent.
pub fn current_region(t: &Table, c: CellRef) -> Range {
    let filled = |r: usize, col: usize| !t.get(r, col).is_empty();
    let (rows, cols) = (t.row_count(), t.width());
    if rows == 0 || cols == 0 || c.row >= rows || c.col >= cols {
        return Range::cell(c);
    }
    let row_has = |row: usize, a: usize, b: usize| (a..=b).any(|col| filled(row, col));
    let col_has = |col: usize, a: usize, b: usize| (a..=b).any(|row| filled(row, col));
    let (mut r0, mut r1, mut c0, mut c1) = (c.row, c.row, c.col, c.col);
    loop {
        let before = (r0, r1, c0, c1);
        let (lo_c, hi_c) = (c0.saturating_sub(1), (c1 + 1).min(cols - 1));
        while r0 > 0 && row_has(r0 - 1, lo_c, hi_c) {
            r0 -= 1;
        }
        while r1 + 1 < rows && row_has(r1 + 1, lo_c, hi_c) {
            r1 += 1;
        }
        let (lo_r, hi_r) = (r0.saturating_sub(1), (r1 + 1).min(rows - 1));
        while c0 > 0 && col_has(c0 - 1, lo_r, hi_r) {
            c0 -= 1;
        }
        while c1 + 1 < cols && col_has(c1 + 1, lo_r, hi_r) {
            c1 += 1;
        }
        if (r0, r1, c0, c1) == before {
            break;
        }
    }
    Range::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::table_of;

    const MAX: CellRef = CellRef::new(99, 25);

    #[test]
    fn ranges_normalise_and_combine() {
        let r = Range::new(CellRef::new(3, 4), CellRef::new(1, 2));
        assert_eq!((r.start, r.end), (CellRef::new(1, 2), CellRef::new(3, 4)));
        assert_eq!((r.row_count(), r.col_count()), (3, 3));
        assert!(r.contains(CellRef::new(2, 3)) && !r.contains(CellRef::new(0, 3)));
        let o = Range::new(CellRef::new(3, 4), CellRef::new(5, 6));
        assert!(r.intersects(&o));
        assert_eq!(r.intersection(&o), Some(Range::cell(CellRef::new(3, 4))));
        assert_eq!(r.union(&o).end, CellRef::new(5, 6));
        assert!(r.intersection(&Range::cell(CellRef::new(9, 9))).is_none());
    }

    #[test]
    fn offsets_clamp_to_the_grid() {
        assert_eq!(CellRef::new(0, 0).offset(-1, -1, MAX), CellRef::new(0, 0));
        assert_eq!(CellRef::new(0, 0).offset(1000, 1000, MAX), MAX);
        assert_eq!(CellRef::new(5, 5).offset(-2, 3, MAX), CellRef::new(3, 8));
    }

    #[test]
    fn selection_extends_adds_and_reports_whole_lines() {
        let mut s = Selection::at(CellRef::new(2, 2));
        s.extend_to(CellRef::new(0, 4));
        assert_eq!(s.current(), Range::new(CellRef::new(0, 2), CellRef::new(2, 4)));
        assert_eq!(s.moving_end(), CellRef::new(0, 4));
        s.add(Range::new(CellRef::new(5, 0), CellRef::new(5, 9)), CellRef::new(5, 0));
        assert_eq!(s.ranges.len(), 2);
        assert!(s.covers_row(5, 10) && !s.covers_row(5, 11) && !s.covers_row(1, 10));
        assert_eq!(s.bounds(), Range::new(CellRef::new(0, 0), CellRef::new(5, 9)));
        let s = Selection::range(Range::new(CellRef::new(0, 3), CellRef::new(9, 3)));
        assert!(s.covers_col(3, 10) && !s.covers_col(3, 11));
    }

    #[test]
    fn cycling_wraps_inside_the_selection() {
        let mut s = Selection::range(Range::new(CellRef::new(0, 0), CellRef::new(1, 1)));
        assert!(s.cycle(true, false));
        assert_eq!(s.active, CellRef::new(1, 0));
        s.cycle(true, false);
        assert_eq!(s.active, CellRef::new(0, 1));
        s.cycle(true, true);
        assert_eq!(s.active, CellRef::new(1, 0));
        s.cycle(false, true);
        assert_eq!(s.active, CellRef::new(0, 1));
        assert!(!Selection::at(CellRef::default()).cycle(true, true));
    }

    #[test]
    fn jumps_follow_data_runs_then_the_edge() {
        let t = table_of(&[&["1"], &["1"], &["1"], &[""], &[""], &["1"]]);
        assert_eq!(jump(&t, CellRef::new(0, 0), 1, 0, MAX), CellRef::new(2, 0));
        assert_eq!(jump(&t, CellRef::new(2, 0), 1, 0, MAX), CellRef::new(5, 0));
        assert_eq!(jump(&t, CellRef::new(5, 0), 1, 0, MAX), CellRef::new(99, 0));
        assert_eq!(jump(&t, CellRef::new(0, 0), 0, 1, MAX), CellRef::new(0, 25));
        assert_eq!(jump(&t, CellRef::new(0, 0), -1, 0, MAX), CellRef::new(0, 0));
        assert_eq!(jump(&t, CellRef::new(3, 0), 0, 0, MAX), CellRef::new(3, 0));
        assert_eq!(jump(&t, CellRef::new(99, 0), -1, 0, MAX), CellRef::new(5, 0));
    }

    #[test]
    fn regions_grow_to_the_filled_block() {
        let t = table_of(&[&["", "", "", ""], &["", "a", "b", ""], &["", "c", "d", ""], &["", "", "", ""], &["", "", "", "z"]]);
        assert_eq!(current_region(&t, CellRef::new(1, 1)), Range::new(CellRef::new(1, 1), CellRef::new(2, 2)));
        assert_eq!(current_region(&t, CellRef::new(3, 3)), Range::new(CellRef::new(1, 1), CellRef::new(4, 3)));
        // An empty cell touching a block (diagonally counts) selects the block, as Ctrl+A does.
        assert_eq!(current_region(&t, CellRef::new(0, 0)), Range::new(CellRef::new(0, 0), CellRef::new(2, 2)));
        assert_eq!(current_region(&t, CellRef::new(0, 3)), Range::new(CellRef::new(0, 1), CellRef::new(4, 3)));
        assert_eq!(current_region(&t, CellRef::new(40, 40)), Range::cell(CellRef::new(40, 40)));
    }
}

