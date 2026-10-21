//! Editing operations on a document, driven by its selection. Shared by the menu bar, the grid's
//! context menu and the keyboard shortcuts, so each exists once.

use csv_editor_table::{Cell, CellRef, Range, Row, Selection, SortKind, SortSpec, io};
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

fn bounds(view: &DocView) -> Range {
    view.sel.bounds()
}

/// Moves every part of the selection by (`dr`, `dc`), clamped to the grid.
fn shift_selection(view: &mut DocView, dr: i64, dc: i64) {
    let max = view.max_cell();
    let body = view.body_start() as i64;
    let mv = |c: CellRef| {
        let moved = c.offset(dr, dc, max);
        CellRef::new(moved.row.max(body as usize), moved.col)
    };
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

pub fn insert_rows_above(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    view.doc.insert_rows(b.start.row, b.row_count())
}

pub fn insert_rows_below(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    view.doc.insert_rows(b.end.row + 1, b.row_count())
}

pub fn delete_rows(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let b = bounds(view);
    let did = view.doc.delete_rows(b.start.row, b.row_count());
    view.clamp_selection();
    did
}

pub fn duplicate_rows(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    view.doc.duplicate_rows(b.start.row, b.row_count())
}

/// Moves the selected rows one step up (`delta` < 0) or down, taking the selection along.
pub fn move_rows(grid: &mut GridState, view: &mut DocView, delta: i64) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    let (from, count) = (b.start.row, b.row_count());
    let rows = view.doc.table.row_count();
    if from + count > rows {
        return false;
    }
    let to = if delta < 0 {
        if from <= view.body_start() {
            return false;
        }
        from - 1
    } else {
        if from + count >= rows {
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

pub fn insert_cols_left(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    view.doc.insert_cols(b.start.col, b.col_count())
}

pub fn insert_cols_right(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    view.doc.insert_cols(b.end.col + 1, b.col_count())
}

pub fn delete_cols(grid: &mut GridState, view: &mut DocView) -> bool {
    grid::cancel_edit(grid);
    let b = bounds(view);
    let did = view.doc.delete_cols(b.start.col, b.col_count());
    view.clamp_selection();
    did
}

pub fn move_cols(grid: &mut GridState, view: &mut DocView, delta: i64) -> bool {
    grid::commit_edit(grid, view);
    let b = bounds(view);
    let (from, count) = (b.start.col, b.col_count());
    let width = view.doc.table.width();
    if from + count > width {
        return false;
    }
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

pub fn autofit_selected(grid: &mut GridState, view: &mut DocView, ui: &Ui) {
    for c in bounds(view).cols() {
        grid::autofit(grid, view, ui, c);
    }
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
}

pub fn rows_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    let n = bounds(view).row_count();
    let s = if n == 1 { "row" } else { "rows" };
    if item(ui, true, format!("Insert {n} {s} above"), "") {
        insert_rows_above(grid, view);
        ui.close();
    }
    if item(ui, true, format!("Insert {n} {s} below"), "") {
        insert_rows_below(grid, view);
        ui.close();
    }
    if item(ui, true, format!("Delete {s}"), "") {
        delete_rows(grid, view);
        ui.close();
    }
    if item(ui, true, format!("Duplicate {s}"), "") {
        duplicate_rows(grid, view);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Move up", "Alt+Up") {
        move_rows(grid, view, -1);
    }
    if item(ui, true, "Move down", "Alt+Down") {
        move_rows(grid, view, 1);
    }
}

pub fn cols_menu(grid: &mut GridState, view: &mut DocView, ui: &mut Ui) {
    let n = bounds(view).col_count();
    let s = if n == 1 { "column" } else { "columns" };
    if item(ui, true, format!("Insert {n} {s} left"), "") {
        insert_cols_left(grid, view);
        ui.close();
    }
    if item(ui, true, format!("Insert {n} {s} right"), "") {
        insert_cols_right(grid, view);
        ui.close();
    }
    if item(ui, true, format!("Delete {s}"), "") {
        delete_cols(grid, view);
        ui.close();
    }
    ui.separator();
    if item(ui, true, "Move left", "Alt+Left") {
        move_cols(grid, view, -1);
    }
    if item(ui, true, "Move right", "Alt+Right") {
        move_cols(grid, view, 1);
    }
    ui.separator();
    if item(ui, view.doc.dialect.header, "Rename header…", "") {
        grid.rename_request = Some(bounds(view).start.col);
        ui.close();
    }
    if item(ui, true, "Fit width to contents", "") {
        autofit_selected(grid, view, ui);
        ui.close();
    }
}

/// Sorts the body rows by the active cell's column.
pub fn sort(grid: &mut GridState, view: &mut DocView, ascending: bool, kind: SortKind) -> bool {
    grid::commit_edit(grid, view);
    let spec = SortSpec { col: view.sel.active.col, ascending, kind };
    let did = view.doc.sort(view.body_start(), spec);
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
    fn column_moves_and_structure_edits_follow_the_selection() {
        let mut v = view("a,b,c\n1,2,3\n");
        let mut g = GridState::default();
        v.sel = Selection::at(CellRef::new(1, 0));
        assert!(move_cols(&mut g, &mut v, 1));
        assert_eq!(text(&v), "b,a,c\n2,1,3\n");
        assert_eq!(v.sel.active, CellRef::new(1, 1));
        assert!(insert_cols_right(&mut g, &mut v));
        assert_eq!(text(&v), "b,a,,c\n2,1,,3\n");
        assert!(delete_cols(&mut g, &mut v), "deletes the selected column (the moved one)");
        assert_eq!(text(&v), "b,,c\n2,,3\n");
        assert!(insert_rows_below(&mut g, &mut v) && duplicate_rows(&mut g, &mut v));
        assert_eq!(v.doc.table.row_count(), 4);
        assert!(delete_rows(&mut g, &mut v));
        v.sel = Selection::range(Range::new(CellRef::new(1, 0), CellRef::new(1, 2)));
        assert!(clear(&mut g, &mut v));
        assert_eq!(text(&v), "b,,c\n,,\n,,\n");
        assert!(undo(&mut g, &mut v) && redo(&mut g, &mut v));
    }
}
