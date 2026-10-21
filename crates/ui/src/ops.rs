//! Editing operations on a document, driven by its selection. Shared by the menu bar, the grid's
//! context menu and the keyboard shortcuts, so each exists once.
//!
//! Row and column operations act on exactly the rows (columns) that contain a selected cell,
//! never on the gap between disconnected selections; the menu items say which ones.

use csv_editor_table::{Case, Cell, CellRef, Range, Row, Selection, SortKind, SortSpec, io};
use egui::{Button, Ui};

use crate::grid::{self, Clip, GridState};
use crate::view::DocView;

/// The current range clipped to the table's extent, as cells (empty past short rows).
fn current_cells(view: &DocView) -> (Range, Vec<Row>) {
    let table = &view.doc.table;
    let r = view.sel.current();
    let end = CellRef::new(
        r.end.row.min(table.row_count().saturating_sub(1)).max(r.start.row),
        r.end.col.min(table.width().saturating_sub(1)).max(r.start.col),
    );
    let r = Range::new(r.start, end);
    let rows = r.rows().map(|row| r.cols().map(|c| Cell::from(table.get(row, c))).collect()).collect();
    (r, rows)
}

/// Copies the current range to the system clipboard (tab-separated) and the editor's own.
pub fn copy(grid: &mut GridState, view: &mut DocView, ctx: &egui::Context) -> bool {
    grid::commit_edit(grid, view);
    let (_, cells) = current_cells(view);
    let text = io::block_text(&cells);
    ctx.copy_text(text.clone());
    grid.clip = Some(Clip { text, cells, cut: None });
    true
}

/// Like [`copy`], and the source is emptied when the block is pasted.
pub fn cut(grid: &mut GridState, view: &mut DocView, ctx: &egui::Context) -> bool {
    copy(grid, view, ctx);
    let (range, _) = current_cells(view);
    if let Some(clip) = grid.clip.as_mut() {
        clip.cut = Some(range);
    }
    true
}

/// Pastes `text` (what the system clipboard holds; read through the host when `None`). Text that
/// matches the editor's own clipboard pastes those cells exactly, carrying a cut along.
pub fn paste(grid: &mut GridState, view: &mut DocView, text: Option<String>) -> bool {
    grid::commit_edit(grid, view);
    let text = text.or_else(|| grid.paste_text.as_ref().and_then(|read| read()));
    let same = |a: &str, b: &str| a.replace("\r\n", "\n") == b.replace("\r\n", "\n");
    let (cells, cut) = match (&grid.clip, &text) {
        (Some(clip), Some(t)) if same(&clip.text, t) => (clip.cells.clone(), clip.cut),
        (Some(clip), None) => (clip.cells.clone(), clip.cut),
        (_, Some(t)) => (io::parse_block(t), None),
        (None, None) => return false,
    };
    let target = view.sel.current();
    match view.doc.paste(target, &cells, cut) {
        Some(r) => {
            view.sel = Selection::range(r);
            if cut.is_some()
                && let Some(clip) = grid.clip.as_mut()
            {
                clip.cut = None;
            }
            true
        }
        None => false,
    }
}

// ----- Which rows and columns an operation means -------------------------------------------------

/// Sorted, distinct rows that contain a selected cell (blank rows past the table included).
pub fn selected_rows(view: &DocView) -> Vec<usize> {
    let mut rows: Vec<usize> = view.sel.ranges.iter().flat_map(|r| r.rows()).collect();
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Sorted, distinct columns that contain a selected cell.
pub fn selected_cols(view: &DocView) -> Vec<usize> {
    let mut cols: Vec<usize> = view.sel.ranges.iter().flat_map(|r| r.cols()).collect();
    cols.sort_unstable();
    cols.dedup();
    cols
}

/// Whether every selected range spans every row: whole columns are selected, so an operation
/// on "the selected rows" has no meaning.
fn whole_columns_selected(view: &DocView) -> bool {
    let max = view.max_cell();
    view.sel.ranges.iter().all(|r| r.start.row == 0 && r.end.row >= max.row)
}

fn whole_rows_selected(view: &DocView) -> bool {
    let max = view.max_cell();
    view.sel.ranges.iter().all(|r| r.start.col == 0 && r.end.col >= max.col)
}

/// Selected rows that exist in the table (the ones a deletion or duplication can touch).
fn existing_rows(view: &DocView) -> Vec<usize> {
    let n = view.doc.table.row_count();
    selected_rows(view).into_iter().filter(|&r| r < n).collect()
}

fn existing_cols(view: &DocView) -> Vec<usize> {
    let w = view.doc.table.width();
    selected_cols(view).into_iter().filter(|&c| c < w).collect()
}

fn contiguous(idx: &[usize]) -> bool {
    idx.windows(2).all(|w| w[1] == w[0] + 1)
}

/// Runs of consecutive indices as `(first, count)`.
fn runs(idx: &[usize]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &i in idx {
        match runs.last_mut() {
            Some((start, count)) if *start + *count == i => *count += 1,
            _ => runs.push((i, 1)),
        }
    }
    runs
}

/// Human description of indices, 1-based: "row 5", "rows 2, 5–6", or "12 rows" when there are
/// many runs.
pub fn describe(idx: &[usize], what: &str, label: impl Fn(usize) -> String) -> String {
    if idx.is_empty() {
        return format!("no {what}s");
    }
    let runs = runs(idx);
    if runs.len() > 4 {
        return format!("{} {what}s", idx.len());
    }
    let parts: Vec<String> = runs.iter().map(|(s, n)| if *n == 1 { label(*s) } else { format!("{}–{}", label(*s), label(s + n - 1)) }).collect();
    format!("{what}{} {}", if idx.len() == 1 { "" } else { "s" }, parts.join(", "))
}

fn row_label(r: usize) -> String {
    (r + 1).to_string()
}

fn describe_rows(rows: &[usize]) -> String {
    describe(rows, "row", row_label)
}

fn describe_cols(cols: &[usize]) -> String {
    describe(cols, "column", csv_editor_table::column_label)
}

/// Moves every part of the selection by (`dr`, `dc`), clamped to the grid.
fn shift_selection(view: &mut DocView, dr: i64, dc: i64) {
    let max = view.max_cell();
    let mv = |c: CellRef| c.offset(dr, dc, max);
    view.sel.active = mv(view.sel.active);
    view.sel.anchor = mv(view.sel.anchor);
    for r in &mut view.sel.ranges {
        *r = Range::new(mv(r.start), mv(r.end));
    }
}

pub fn undo(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let did = view.doc.undo().is_some();
    view.clamp_selection();
    did
}

pub fn redo(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let did = view.doc.redo().is_some();
    view.clamp_selection();
    did
}

/// Empties the selected cells.
pub fn clear(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let ranges = view.sel.ranges.clone();
    view.doc.clear(&ranges)
}

/// Copies the top row of the current range into the rows below it.
pub fn fill_down(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    view.doc.fill_down(view.sel.current())
}

// ----- Rows ----------------------------------------------------------------------------------

/// Inserts blank rows above each run of selected rows, as many as the run has (Excel's rule).
pub fn insert_rows_above(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let rows = selected_rows(view);
    view.doc.insert_rows_at_runs(&rows, false)
}

/// Inserts blank rows below each run of selected rows, as many as the run has.
pub fn insert_rows_below(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let rows = selected_rows(view);
    view.doc.insert_rows_at_runs(&rows, true)
}

/// Deletes exactly the selected rows.
pub fn delete_rows(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let rows = existing_rows(view);
    let did = view.doc.delete_row_set(&rows);
    view.clamp_selection();
    did
}

/// Inserts a copy of each selected row right after it (after each run of them).
pub fn duplicate_rows(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let rows = existing_rows(view);
    view.doc.duplicate_row_set(&rows)
}

/// Moves the selected rows one step up (`delta` < 0) or down, taking the selection along. Only
/// a contiguous block of existing rows moves.
pub fn move_rows(grid: &mut GridState, view: &mut DocView, delta: i64) -> bool {
    grid::commit_edit(grid, view);
    let rows = selected_rows(view);
    let total = view.doc.table.row_count();
    let (Some(&from), Some(&last)) = (rows.first(), rows.last()) else { return false };
    if !contiguous(&rows) || last >= total {
        return false;
    }
    let count = rows.len();
    let to = if delta < 0 {
        if from <= view.data_start() {
            return false;
        }
        from - 1
    } else {
        if from + count >= total {
            return false;
        }
        from + count + 1
    };
    if view.doc.move_rows(from, count, to) {
        shift_selection(view, delta.signum(), 0);
        true
    } else {
        false
    }
}

// ----- Columns -------------------------------------------------------------------------------

/// Inserts blank columns left of each run of selected columns, as many as the run has.
pub fn insert_cols_left(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let cols = selected_cols(view);
    view.doc.insert_cols_at_runs(&cols, false)
}

pub fn insert_cols_right(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let cols = selected_cols(view);
    view.doc.insert_cols_at_runs(&cols, true)
}

/// Deletes exactly the selected columns.
pub fn delete_cols(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let cols = existing_cols(view);
    let did = view.doc.delete_col_set(&cols);
    view.clamp_selection();
    did
}

/// Moves the selected columns one step left or right. Only a contiguous block of existing
/// columns moves.
pub fn move_cols(grid: &mut GridState, view: &mut DocView, delta: i64) -> bool {
    grid::commit_edit(grid, view);
    let cols = selected_cols(view);
    let width = view.doc.table.width();
    let (Some(&from), Some(&last)) = (cols.first(), cols.last()) else { return false };
    if !contiguous(&cols) || last >= width {
        return false;
    }
    let count = cols.len();
    let to = if delta < 0 {
        if from == 0 {
            return false;
        }
        from - 1
    } else {
        if from + count >= width {
            return false;
        }
        from + count + 1
    };
    if view.doc.move_cols(from, count, to) {
        shift_selection(view, 0, delta.signum());
        true
    } else {
        false
    }
}

pub fn autofit_selected_cols(grid: &mut GridState, view: &mut DocView, ui: &Ui) {
    let cols = selected_cols(view);
    grid::autofit_cols(grid, view, ui, &cols);
}

pub fn autofit_selected_rows(grid: &mut GridState, view: &mut DocView) {
    let rows = selected_rows(view);
    grid::autofit_rows(grid, view, &rows);
}

fn item(ui: &mut Ui, enabled: bool, label: impl Into<egui::WidgetText>, shortcut: &str) -> bool {
    let button = Button::new(label).shortcut_text(shortcut);
    ui.add_enabled(enabled, button).clicked()
}

pub fn edit_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    let h = &view.doc.history;
    let undo_label = h.undo_label().map_or("Undo".to_string(), |l| format!("Undo {l}"));
    let redo_label = h.redo_label().map_or("Redo".to_string(), |l| format!("Redo {l}"));
    let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
    if item(ui, can_undo, undo_label, "Ctrl+Z") {
        undo(grid, view);
        ui.close();
    }
    if item(ui, can_redo, redo_label, "Ctrl+Y") {
        redo(grid, view);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Cut", "Ctrl+X") {
        cut(grid, view, ui.ctx());
        ui.close();
    }
    if item(ui, true, "Copy", "Ctrl+C") {
        copy(grid, view, ui.ctx());
        ui.close();
    }
    if item(ui, grid.clip.is_some() || grid.paste_text.is_some(), "Paste", "Ctrl+V") {
        paste(grid, view, None);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Delete contents", "Del") {
        clear(grid, view);
        ui.close();
    }
    if item(ui, view.sel.current().row_count() > 1, "Fill down", "Ctrl+D") {
        fill_down(grid, view);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Go to row…", "Ctrl+G") {
        grid.goto_request = true;
        ui.close();
    }
}

/// The data rows of the table (the header row excluded when there is one).
pub fn whole_body(view: &DocView) -> Range {
    let table = &view.doc.table;
    let start = view.data_start();
    Range::new(CellRef::new(start, 0), CellRef::new(table.row_count().saturating_sub(1).max(start), table.width().saturating_sub(1)))
}

/// Where a cleanup tool works: the selection when more than one cell is selected, else everything.
fn scope(view: &DocView) -> Range {
    if view.sel.is_single_cell() { whole_body(view) } else { view.sel.bounds() }
}

fn note(grid: &mut GridState, n: usize, what: &str, verb: &str) {
    grid.note = Some(if n == 0 { format!("No {what} {verb}") } else { format!("{n} {what} {verb}") });
}

pub fn trim_whitespace(grid: &mut GridState, view: &mut DocView) -> usize {
    grid::commit_edit(grid, view);
    let n = view.doc.trim_whitespace(scope(view));
    note(grid, n, "cells", "trimmed");
    n
}

pub fn change_case(grid: &mut GridState, view: &mut DocView, case: Case) -> usize {
    grid::commit_edit(grid, view);
    let n = view.doc.change_case(scope(view), case);
    note(grid, n, "cells", "changed");
    n
}

pub fn remove_empty_rows(grid: &mut GridState, view: &mut DocView) -> usize {
    grid::cancel_edit(grid);
    let n = view.doc.remove_empty_rows(view.data_start());
    view.clamp_selection();
    note(grid, n, "empty rows", "removed");
    n
}

pub fn remove_empty_cols(grid: &mut GridState, view: &mut DocView) -> usize {
    grid::cancel_edit(grid);
    let n = view.doc.remove_empty_cols();
    view.clamp_selection();
    note(grid, n, "empty columns", "removed");
    n
}

pub fn remove_duplicate_rows(grid: &mut GridState, view: &mut DocView) -> usize {
    grid::cancel_edit(grid);
    let n = view.doc.remove_duplicate_rows(view.data_start());
    view.clamp_selection();
    note(grid, n, "duplicate rows", "removed");
    n
}

pub fn rows_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    ui.set_min_width(240.0);
    if whole_columns_selected(view) {
        ui.label(egui::RichText::new("Whole columns are selected: select cells or rows first.").weak());
        return;
    }
    let rows = selected_rows(view);
    let existing = existing_rows(view);
    let n = rows.len();
    let s = if n == 1 { "row" } else { "rows" };
    let which = describe_rows(&rows);
    let which_existing = describe_rows(&existing);
    ui.label(egui::RichText::new(format!("Selected: {which}")).weak());
    let where_rows = if runs(&rows).len() > 1 { " (above each)" } else { "" };
    if item(ui, !rows.is_empty(), format!("Insert {n} blank {s} above{where_rows}"), "") {
        insert_rows_above(grid, view);
        ui.close();
    }
    let where_rows = if runs(&rows).len() > 1 { " (below each)" } else { "" };
    if item(ui, !rows.is_empty(), format!("Insert {n} blank {s} below{where_rows}"), "") {
        insert_rows_below(grid, view);
        ui.close();
    }
    if item(ui, !existing.is_empty(), format!("Delete {which_existing}"), "") {
        delete_rows(grid, view);
        ui.close();
    }
    if item(ui, !existing.is_empty(), format!("Duplicate {which_existing}"), "") {
        duplicate_rows(grid, view);
        ui.close();
    }
    ui.separator();
    let movable = contiguous(&rows) && rows.last().is_some_and(|&l| l < view.doc.table.row_count());
    if item(ui, movable, format!("Move {which} up"), "Alt+Up") {
        move_rows(grid, view, -1);
    }
    if item(ui, movable, format!("Move {which} down"), "Alt+Down") {
        move_rows(grid, view, 1);
    }
    if !movable && !rows.is_empty() {
        ui.label(egui::RichText::new("Only a contiguous block of rows can move.").weak());
    }
    ui.separator();
    if item(ui, !existing.is_empty(), "Fit height to contents", "") {
        autofit_selected_rows(grid, view);
        ui.close();
    }
    if item(ui, rows.iter().any(|r| view.row_heights.contains_key(r)), "Reset height", "") {
        grid::reset_row_heights(view, &rows);
        ui.close();
    }
}

pub fn cols_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    ui.set_min_width(240.0);
    if whole_rows_selected(view) {
        ui.label(egui::RichText::new("Whole rows are selected: select cells or columns first.").weak());
        return;
    }
    let cols = selected_cols(view);
    let existing = existing_cols(view);
    let n = cols.len();
    let s = if n == 1 { "column" } else { "columns" };
    let which = describe_cols(&cols);
    let which_existing = describe_cols(&existing);
    ui.label(egui::RichText::new(format!("Selected: {which}")).weak());
    let where_cols = if runs(&cols).len() > 1 { " (left of each)" } else { "" };
    if item(ui, !cols.is_empty(), format!("Insert {n} blank {s} left{where_cols}"), "") {
        insert_cols_left(grid, view);
        ui.close();
    }
    let where_cols = if runs(&cols).len() > 1 { " (right of each)" } else { "" };
    if item(ui, !cols.is_empty(), format!("Insert {n} blank {s} right{where_cols}"), "") {
        insert_cols_right(grid, view);
        ui.close();
    }
    if item(ui, !existing.is_empty(), format!("Delete {which_existing}"), "") {
        delete_cols(grid, view);
        ui.close();
    }
    ui.separator();
    let movable = contiguous(&cols) && cols.last().is_some_and(|&l| l < view.doc.table.width());
    if item(ui, movable, format!("Move {which} left"), "Alt+Left") {
        move_cols(grid, view, -1);
    }
    if item(ui, movable, format!("Move {which} right"), "Alt+Right") {
        move_cols(grid, view, 1);
    }
    if !movable && !cols.is_empty() {
        ui.label(egui::RichText::new("Only a contiguous block of columns can move.").weak());
    }
    ui.separator();
    if item(ui, !cols.is_empty(), "Fit width to contents", "") {
        autofit_selected_cols(grid, view, ui);
        ui.close();
    }
}

/// Sorts the data rows by the active cell's column.
pub fn sort(grid: &mut GridState, view: &mut DocView, ascending: bool, kind: SortKind) -> bool {
    grid::commit_edit(grid, view);
    let spec = SortSpec { col: view.sel.active.col, ascending, kind };
    let did = view.doc.sort(view.data_start(), spec);
    grid.scroll_to_active = true;
    did
}

pub fn data_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    let col = view.sel.active.col;
    let title = view.column_title(col);
    ui.label(egui::RichText::new(format!("Column {title}")).weak());
    if item(ui, true, "Sort A → Z", "") {
        sort(grid, view, true, SortKind::Text);
        ui.close();
    }
    if item(ui, true, "Sort Z → A", "") {
        sort(grid, view, false, SortKind::Text);
        ui.close();
    }
    if item(ui, true, "Sort numbers 0 → 9", "") {
        sort(grid, view, true, SortKind::Numeric);
        ui.close();
    }
    if item(ui, true, "Sort numbers 9 → 0", "") {
        sort(grid, view, false, SortKind::Numeric);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Filter rows by this column…", "") {
        grid.filter_request = Some(col);
        ui.close();
    }
    let has_filter = view.filters.iter().any(|f| f.col == col);
    if item(ui, has_filter, "Clear this column's filter", "") {
        view.clear_filter(col);
        grid.scroll_to_active = true;
        ui.close();
    }
    if item(ui, view.filtered(), "Clear all filters", "") {
        view.clear_filters();
        grid.scroll_to_active = true;
        ui.close();
    }
    ui.separator();
    let where_ = if view.sel.is_single_cell() { "everywhere" } else { "in the selection" };
    if item(ui, true, format!("Trim whitespace {where_}"), "") {
        trim_whitespace(grid, view);
        ui.close();
    }
    ui.menu_button(format!("Change case {where_}"), |ui| {
        for case in Case::ALL {
            if ui.button(case.label()).clicked() {
                change_case(grid, view, case);
                ui.close();
            }
        }
    });
    ui.separator();
    if item(ui, true, "Remove empty rows", "") {
        remove_empty_rows(grid, view);
        ui.close();
    }
    if item(ui, true, "Remove empty columns", "") {
        remove_empty_cols(grid, view);
        ui.close();
    }
    if item(ui, true, "Remove duplicate rows", "") {
        remove_duplicate_rows(grid, view);
        ui.close();
    }
}

/// The column header's right-click menu: column structure, then sort and filter.
pub fn header_context_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    cols_menu(grid, view, ui);
    ui.separator();
    data_menu(grid, view, ui);
}

/// The grid's right-click menu.
pub fn context_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    ui.set_min_width(220.0);
    edit_menu(grid, view, ui);
    ui.separator();
    ui.menu_button("Rows", |ui| rows_menu(grid, view, ui));
    ui.menu_button("Columns", |ui| cols_menu(grid, view, ui));
    ui.menu_button("Sort and filter", |ui| data_menu(grid, view, ui));
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_editor_table::{Document, ParseOptions, Selection};

    fn view(text: &str) -> DocView {
        DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap())
    }

    fn text(v: &DocView) -> String {
        String::from_utf8(v.doc.to_bytes().0).unwrap()
    }

    fn select(v: &mut DocView, ranges: &[(usize, usize, usize, usize)]) {
        let rs: Vec<Range> = ranges.iter().map(|&(r0, c0, r1, c1)| Range::new(CellRef::new(r0, c0), CellRef::new(r1, c1))).collect();
        let active = rs.last().map(|r| r.start).unwrap_or_default();
        v.sel = Selection { active, anchor: active, ranges: rs };
    }

    #[test]
    fn descriptions_name_the_exact_rows() {
        assert_eq!(describe_rows(&[4]), "row 5");
        assert_eq!(describe_rows(&[1, 4, 5]), "rows 2, 5–6");
        assert_eq!(describe_rows(&(0..12).step_by(2).collect::<Vec<_>>()), "6 rows");
        assert_eq!(describe_rows(&[]), "no rows");
        assert_eq!(describe_cols(&[0, 1, 2, 27]), "columns A–C, AB");
    }

    #[test]
    fn disconnected_selections_touch_only_their_own_rows() {
        let mut v = view("h\na\nb\nc\nd\ne\n");
        let mut g = GridState::default();
        // Rows 2 and 5 (b, e) selected as two cells with a gap.
        select(&mut v, &[(2, 0, 2, 0), (5, 0, 5, 0)]);
        assert_eq!(selected_rows(&v), vec![2, 5]);
        assert!(delete_rows(&mut g, &mut v));
        assert_eq!(text(&v), "h\na\nc\nd\n", "the rows in between stay");
        assert!(undo(&mut g, &mut v));
        select(&mut v, &[(2, 0, 2, 0), (5, 0, 5, 0)]);
        assert!(duplicate_rows(&mut g, &mut v));
        assert_eq!(text(&v), "h\na\nb\nb\nc\nd\ne\ne\n", "each selected row is copied in place");
        assert!(undo(&mut g, &mut v));
        select(&mut v, &[(2, 0, 2, 0), (5, 0, 5, 0)]);
        assert!(!move_rows(&mut g, &mut v, 1), "a gap means no move");
        assert!(insert_rows_above(&mut g, &mut v));
        assert_eq!(v.doc.table.row_count(), 8, "one blank row above each selected row");
        assert_eq!(text(&v), "h\na\n\nb\nc\nd\n\ne\n");
        assert!(undo(&mut g, &mut v));
        select(&mut v, &[(2, 0, 2, 0), (5, 0, 5, 0)]);
        assert!(insert_rows_below(&mut g, &mut v));
        assert_eq!(text(&v), "h\na\nb\n\nc\nd\ne\n\n");
    }

    #[test]
    fn row_moves_carry_the_selection_and_stop_at_the_edges() {
        let mut v = view("h\na\nb\nc\n");
        let mut g = GridState::default();
        v.sel = Selection::at(CellRef::new(2, 0));
        assert!(move_rows(&mut g, &mut v, 1));
        assert_eq!(text(&v), "h\na\nc\nb\n");
        assert_eq!(v.sel.active, CellRef::new(3, 0));
        assert!(!move_rows(&mut g, &mut v, 1), "last row cannot move down");
        assert!(move_rows(&mut g, &mut v, -1) && move_rows(&mut g, &mut v, -1));
        assert_eq!(v.sel.active, CellRef::new(1, 0));
        assert!(!move_rows(&mut g, &mut v, -1), "the header row stays first");
        v.sel = Selection::at(CellRef::new(20, 0));
        assert!(!move_rows(&mut g, &mut v, -1), "blank rows past the table do not move");
    }

    #[test]
    fn column_operations_follow_the_selected_columns() {
        let mut v = view("a,b,c,d\n1,2,3,4\n");
        let mut g = GridState::default();
        select(&mut v, &[(0, 0, 0, 0), (1, 3, 1, 3)]);
        assert_eq!(selected_cols(&v), vec![0, 3]);
        assert!(delete_cols(&mut g, &mut v));
        assert_eq!(text(&v), "b,c\n2,3\n", "only the selected columns go");
        assert!(undo(&mut g, &mut v));
        v.sel = Selection::at(CellRef::new(1, 0));
        assert!(move_cols(&mut g, &mut v, 1));
        assert_eq!(text(&v), "b,a,c,d\n2,1,3,4\n");
        assert_eq!(v.sel.active, CellRef::new(1, 1));
        assert!(insert_cols_right(&mut g, &mut v));
        assert_eq!(text(&v), "b,a,,c,d\n2,1,,3,4\n");
        // Whole-column and whole-row selections disable the other axis' operations.
        let max = v.max_cell();
        select(&mut v, &[(0, 1, max.row, 1)]);
        assert!(whole_columns_selected(&v) && !whole_rows_selected(&v));
        select(&mut v, &[(1, 0, 1, max.col)]);
        assert!(whole_rows_selected(&v) && !whole_columns_selected(&v));
    }
}
