//! The grid: row gutter, column headers, cells, selection, scrolling and column sizing.
//!
//! Only the visible rows and columns are laid out and painted. Scrolling is by whole rows and
//! columns (`DocView::scroll_row` / `scroll_col`), so a million rows never meet f32 precision. The
//! scrollbars are drawn by hand (after GridCraft's `grid.rs`, MIT, see `ATTRIBUTION.md`).

use csv_editor_table::{CellRef, Range, Selection, column_label, selection};
use egui::{Align2, Color32, CursorIcon, Event, EventFilter, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::EditorApp;
use crate::theme::{self, Tokens};
use crate::view::DocView;

pub const SCROLLBAR: f32 = 12.0;
const RESIZE_GRIP: f32 = 4.0;
pub const MIN_COL_WIDTH: f32 = 20.0;
pub const MAX_COL_WIDTH: f32 = 2000.0;
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct GridState {
    pub default_col_width: f32,
    pub row_height: f32,
    pub header_height: f32,
    pub zoom: f32,
    /// The in-cell editor, while a cell is being edited.
    pub editor: Option<CellEditor>,
    /// A request to rename this column's header, picked up by the app's dialog.
    pub rename_request: Option<usize>,
    drag: Option<Drag>,
    /// Wheel movement not yet turned into whole rows and columns.
    wheel: Vec2,
}

/// Text being typed into a cell.
#[derive(Clone, Debug)]
pub struct CellEditor {
    pub cell: CellRef,
    pub text: String,
    /// Started by typing over the cell: arrow keys commit and move, as in a spreadsheet's Enter
    /// mode. Started with F2 or a double-click, arrows move inside the text instead.
    pub replace: bool,
    fresh: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Cells,
    Rows,
    Cols,
    Resize { col: usize, start_x: f32, start_w: f32 },
}

impl Default for GridState {
    fn default() -> Self {
        GridState {
            default_col_width: 96.0,
            row_height: 22.0,
            header_height: 24.0,
            zoom: 1.0,
            editor: None,
            rename_request: None,
            drag: None,
            wheel: Vec2::ZERO,
        }
    }
}

/// Starts editing the active cell: with `text` typed over it, or its current text.
pub fn start_edit(grid: &mut GridState, view: &DocView, typed: Option<String>) {
    let cell = view.sel.active;
    let (text, replace) = match typed {
        Some(t) => (t, true),
        None => (view.doc.table.get(cell.row, cell.col).to_string(), false),
    };
    grid.editor = Some(CellEditor { cell, text, replace, fresh: true });
}

/// Writes the editor's text into its cell (one undo step) and closes the editor.
pub fn commit_edit(grid: &mut GridState, view: &mut DocView) -> bool {
    match grid.editor.take() {
        Some(ed) => view.doc.set_cell(ed.cell.row, ed.cell.col, ed.text.as_str()),
        None => false,
    }
}

/// [`commit_edit`] for an optional view (the editor only ever belongs to the active document).
pub fn commit_edit_in(grid: &mut GridState, view: Option<&mut DocView>) {
    match view {
        Some(view) => {
            commit_edit(grid, view);
        }
        None => grid.editor = None,
    }
}

pub fn cancel_edit(grid: &mut GridState) {
    grid.editor = None;
}

impl GridState {
    /// Sets the zoom, snapped to 5% steps within the supported range.
    pub fn set_zoom(&mut self, zoom: f32) {
        if zoom.is_finite() {
            self.zoom = ((zoom * 20.0).round() / 20.0).clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    pub fn zoom_step(&mut self, steps: i32) {
        self.set_zoom(self.zoom + 0.1 * steps as f32);
    }
}

/// Where everything is this frame.
pub struct Geo {
    /// The grid including its headers, without the scrollbars.
    pub rect: Rect,
    /// The body: cells only.
    pub cells: Rect,
    pub z: f32,
    pub rh: f32,
    /// Visible columns: index and horizontal extent (the last one may be partly hidden).
    pub cols: Vec<(usize, f32, f32)>,
    /// Visible rows: index and vertical extent.
    pub rows: Vec<(usize, f32, f32)>,
}

impl Geo {
    fn col_at(&self, x: f32) -> Option<usize> {
        self.cols.iter().find(|(_, x0, x1)| x >= *x0 && x < *x1).map(|c| c.0)
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        self.rows.iter().find(|(_, y0, y1)| y >= *y0 && y < *y1).map(|r| r.0)
    }

    fn cell_at(&self, p: Pos2) -> Option<CellRef> {
        Some(CellRef::new(self.row_at(p.y)?, self.col_at(p.x)?))
    }

    /// The cell under or nearest to the pointer, for drags that leave the body.
    fn cell_near(&self, p: Pos2) -> Option<CellRef> {
        let col = if p.x < self.cells.left() {
            self.cols.first().map(|c| c.0)
        } else if p.x >= self.cells.right() {
            self.cols.last().map(|c| c.0)
        } else {
            self.col_at(p.x)
        };
        let row = if p.y < self.cells.top() {
            self.rows.first().map(|r| r.0)
        } else if p.y >= self.cells.bottom() {
            self.rows.last().map(|r| r.0)
        } else {
            self.row_at(p.y)
        };
        Some(CellRef::new(row?, col?))
    }

    fn cell_rect(&self, c: CellRef) -> Option<Rect> {
        let (_, x0, x1) = self.cols.iter().find(|k| k.0 == c.col)?;
        let (_, y0, y1) = self.rows.iter().find(|k| k.0 == c.row)?;
        Some(Rect::from_min_max(pos2(*x0, *y0), pos2(*x1, *y1)))
    }

    /// The on-screen rectangle of a range, extended past the body edge where the range continues
    /// out of view; `None` when none of it is visible.
    fn range_rect(&self, r: Range) -> Option<Rect> {
        let (first_col, last_col) = (self.cols.first()?.0, self.cols.last()?.0);
        let (first_row, last_row) = (self.rows.first()?.0, self.rows.last()?.0);
        if r.end.col < first_col || r.start.col > last_col || r.end.row < first_row || r.start.row > last_row {
            return None;
        }
        let x0 = if r.start.col < first_col { self.cells.left() - 4.0 } else { self.cols.iter().find(|k| k.0 == r.start.col)?.1 };
        let x1 = if r.end.col > last_col { self.cells.right() + 4.0 } else { self.cols.iter().find(|k| k.0 == r.end.col)?.2 };
        let y0 = if r.start.row < first_row { self.cells.top() - 4.0 } else { self.rows.iter().find(|k| k.0 == r.start.row)?.1 };
        let y1 = if r.end.row > last_row { self.cells.bottom() + 4.0 } else { self.rows.iter().find(|k| k.0 == r.end.row)?.2 };
        Some(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)))
    }

    /// The column whose right edge is within the resize grip of `x`.
    fn col_edge_at(&self, x: f32) -> Option<usize> {
        self.cols.iter().find(|(_, _, x1)| (x - x1).abs() <= RESIZE_GRIP).map(|c| c.0)
    }

    /// How many rows are fully visible.
    fn full_rows(&self) -> usize {
        self.rows.iter().filter(|(_, _, y1)| *y1 <= self.cells.bottom() + 0.5).count().max(1)
    }

    fn full_cols(&self) -> usize {
        self.cols.iter().filter(|(_, _, x1)| *x1 <= self.cells.right() + 0.5).count().max(1)
    }
}

fn col_width(grid: &GridState, view: &DocView, c: usize) -> f32 {
    view.col_widths.get(&c).copied().unwrap_or(grid.default_col_width)
}

/// Cell text as drawn on one line: line breaks become a pilcrow (a glyph egui's default font has).
pub fn display_text(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\r', '\n']) {
        std::borrow::Cow::Owned(text.replace("\r\n", "\u{00B6}").replace(['\r', '\n'], "\u{00B6}"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Numbers are right-aligned, as in a spreadsheet; everything else is text.
pub fn looks_numeric(text: &str) -> bool {
    let s = text.trim();
    s.bytes().next().is_some_and(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.') && s.parse::<f64>().is_ok()
}

fn geometry(grid: &GridState, view: &mut DocView, rect: Rect) -> Geo {
    let z = grid.zoom;
    let rh = (grid.row_height * z).max(4.0);
    let head_h = if view.doc.dialect.header { 38.0 } else { grid.header_height } * z;
    let digits = view.extent_rows().to_string().len() as f32;
    let gutter_w = (digits * 8.0 + 16.0).max(40.0) * z;
    let cells = Rect::from_min_max(pos2(rect.left() + gutter_w, rect.top() + head_h), rect.max);
    let body_start = view.body_start();
    let visible_rows = ((cells.height() / rh).floor().max(1.0)) as usize;
    let max_scroll_row = view.extent_rows().saturating_sub(visible_rows).max(body_start);
    view.scroll_row = view.scroll_row.clamp(body_start, max_scroll_row);
    view.scroll_col = view.scroll_col.min(view.extent_cols().saturating_sub(1));
    let mut rows = Vec::new();
    let (mut y, mut r) = (cells.top(), view.scroll_row);
    while y < cells.bottom() && rows.len() < 10_000 {
        rows.push((r, y, y + rh));
        y += rh;
        r = r.saturating_add(1);
    }
    let mut cols = Vec::new();
    let (mut x, mut c) = (cells.left(), view.scroll_col);
    while x < cells.right() && cols.len() < 10_000 {
        let w = (col_width(grid, view, c) * z).max(1.0);
        cols.push((c, x, x + w));
        x += w;
        c = c.saturating_add(1);
    }
    Geo { rect, cells, z, rh, cols, rows }
}

pub fn show(app: &mut EditorApp, ui: &mut egui::Ui, t: &Tokens) {
    let full = ui.available_rect_before_wrap();
    if full.width() <= SCROLLBAR * 3.0 || full.height() <= SCROLLBAR * 3.0 {
        return;
    }
    let EditorApp { grid, docs, active, .. } = app;
    let Some(view) = docs.get_mut(*active) else { return };
    let rect = Rect::from_min_max(full.min, pos2(full.right() - SCROLLBAR, full.bottom() - SCROLLBAR));
    let vtrack = Rect::from_min_max(pos2(rect.right(), full.top()), pos2(full.right(), rect.bottom()));
    let htrack = Rect::from_min_max(pos2(full.left(), rect.bottom()), pos2(rect.right(), full.bottom()));
    ui.allocate_rect(full, Sense::hover());
    let resp = ui.interact(rect, ui.scope_id().with("grid"), Sense::click_and_drag());
    // Keys go to the grid unless something else (the cell editor, a dialog) has asked for them.
    if grid.editor.is_none() && ui.memory(|m| m.focused().is_none()) {
        resp.request_focus();
    }

    let geo = geometry(grid, view, rect);
    interact(grid, view, ui, &resp, &geo);
    // Interaction may have scrolled or resized: lay out again before painting.
    let geo = geometry(grid, view, rect);
    paint(ui, &geo, view, t);
    vertical_scrollbar(view, ui, vtrack, &geo, t);
    horizontal_scrollbar(grid, view, ui, htrack, &geo, t);
    ui.painter().rect_filled(Rect::from_min_max(pos2(rect.right(), rect.bottom()), full.max), 0.0, t.tab_bar);
    editor_ui(grid, view, ui, &resp, &geo, t);
    resp.context_menu(|ui| crate::ops::context_menu(grid, view, ui));
}

/// The in-cell editor: key handling first (Enter, Tab, Escape and, in replace mode, arrows act
/// on the grid, not the text), then the text field drawn over the cell, growing to fit.
fn editor_ui(grid: &mut GridState, view: &mut DocView, ui: &mut egui::Ui, grid_resp: &Response, geo: &Geo, t: &Tokens) {
    let Some(ed) = grid.editor.as_mut() else { return };
    let replace = ed.replace;
    let (enter, shift_enter, alt_enter, tab, shift_tab, esc) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::SHIFT, Key::Enter),
            i.consume_key(Modifiers::ALT, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Tab),
            i.consume_key(Modifiers::SHIFT, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if alt_enter {
        ed.text.push('\n');
    }
    let mut arrows = (0i64, 0i64);
    if replace {
        arrows = ui.input_mut(|i| {
            let mut d = (0i64, 0i64);
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                d.0 += 1;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                d.0 -= 1;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                d.1 += 1;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                d.1 -= 1;
            }
            d
        });
    }
    if esc {
        grid.editor = None;
        ui.memory_mut(|m| m.request_focus(grid_resp.id));
        return;
    }
    // (commit, move within the selection by row?, direction)
    let after: Option<(bool, i64, i64)> = if enter {
        Some((false, 1, 0))
    } else if shift_enter {
        Some((false, -1, 0))
    } else if tab {
        Some((true, 0, 1))
    } else if shift_tab {
        Some((true, 0, -1))
    } else if arrows != (0, 0) {
        Some((false, arrows.0, arrows.1))
    } else {
        None
    };
    if let Some((by_row, dr, dc)) = after {
        commit_edit(grid, view);
        let within_selection = (enter || shift_enter || tab || shift_tab) && view.sel.cycle(dr + dc > 0, by_row);
        if !within_selection {
            step(view, dr, dc, false, false);
        }
        ensure_visible(grid, view, geo);
        ui.memory_mut(|m| m.request_focus(grid_resp.id));
        return;
    }
    let Some(ed) = grid.editor.as_mut() else { return };
    let Some(cell) = geo.cell_rect(ed.cell) else {
        // Scrolled out of view: keep the text, draw nothing.
        return;
    };
    let z = geo.z;
    let font = theme::cell_font(13.0 * z);
    let needed = ui.ctx().fonts_mut(|f| f.layout_no_wrap(ed.text.clone(), font.clone(), Color32::WHITE).size());
    let w = (needed.x + 14.0 * z).max(cell.width()).min((geo.cells.right() - cell.left()).max(cell.width()));
    let h = (needed.y + 6.0 * z).max(cell.height());
    let rect = Rect::from_min_size(cell.min, vec2(w, h)).intersect(geo.rect);
    let p = ui.painter();
    p.rect_filled(rect, 0.0, t.grid_bg);
    p.rect_stroke(rect, 0.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
    let inner = rect.shrink2(vec2(4.0 * z, 1.0));
    let fresh = std::mem::take(&mut ed.fresh);
    let editor_id = ui.scope_id().with("cell-editor");
    if fresh {
        // Focus and the key filter from the first frame on, so an arrow key pressed right after
        // typing moves the caret (or commits) instead of moving focus to another widget.
        ui.memory_mut(|m| {
            m.request_focus(editor_id);
            m.set_focus_lock_filter(editor_id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true });
        });
    }
    let output = ui.put(
        inner,
        egui::TextEdit::multiline(&mut ed.text)
            .id(editor_id)
            .font(font)
            .text_color(t.cell_text)
            .frame(egui::Frame::NONE)
            .desired_width(inner.width())
            .lock_focus(true),
    );
    if !fresh && !output.has_focus() {
        // Clicked somewhere else (a menu, another panel): the edit is done.
        commit_edit(grid, view);
    }
}

fn paint(ui: &egui::Ui, geo: &Geo, view: &DocView, t: &Tokens) {
    let p = ui.painter_at(geo.rect);
    let z = geo.z;
    p.rect_filled(geo.rect, 0.0, t.grid_bg);
    let body = p.with_clip_rect(geo.cells);
    for r in &view.sel.ranges {
        if let Some(rect) = geo.range_rect(*r) {
            body.rect_filled(rect, 0.0, t.sel_fill);
        }
    }
    if let Some(active) = geo.cell_rect(view.sel.active) {
        body.rect_filled(active, 0.0, t.grid_bg);
    }
    let font = theme::cell_font(13.0 * z);
    let pad = 4.0 * z;
    let table = &view.doc.table;
    for &(r, y0, y1) in &geo.rows {
        for &(c, x0, x1) in &geo.cols {
            let text = table.get(r, c);
            if text.is_empty() {
                continue;
            }
            let cell = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            let clip = cell.intersect(geo.cells).shrink2(vec2(pad, 0.0));
            if clip.width() <= 0.0 {
                continue;
            }
            let shown = display_text(text);
            let (pos, anchor) = if looks_numeric(text) {
                (pos2(clip.right(), cell.center().y), Align2::RIGHT_CENTER)
            } else {
                (pos2(clip.left(), cell.center().y), Align2::LEFT_CENTER)
            };
            p.with_clip_rect(clip).text(pos, anchor, shown, font.clone(), t.cell_text);
        }
    }
    let gl = Stroke::new(1.0, t.gridline);
    for &(_, _, x1) in &geo.cols {
        body.vline(x1, geo.cells.y_range(), gl);
    }
    for &(_, _, y1) in &geo.rows {
        body.hline(geo.cells.x_range(), y1, gl);
    }
    if let Some(rect) = geo.range_rect(view.sel.current()) {
        body.rect_stroke(rect, 0.0, Stroke::new(2.0 * z.max(0.75), t.sel_border), StrokeKind::Inside);
    }
    paint_headers(&p, geo, view, t);
}

fn paint_headers(p: &egui::Painter, geo: &Geo, view: &DocView, t: &Tokens) {
    let z = geo.z;
    let rect = geo.rect;
    let head = Rect::from_min_max(rect.min, pos2(rect.right(), geo.cells.top()));
    let gutter = Rect::from_min_max(rect.min, pos2(geo.cells.left(), rect.bottom()));
    p.rect_filled(head, 0.0, t.header_bg);
    p.rect_filled(gutter, 0.0, t.header_bg);
    let hl = Stroke::new(1.0, t.header_line);
    let (rows_total, cols_total, body_start) = (view.extent_rows(), view.extent_cols(), view.body_start());
    let sel = &view.sel;
    let label_font = theme::ui_font(12.0 * z);
    let small_font = theme::ui_font(10.0 * z);
    let name_font = theme::ui_font(13.0 * z);
    let whole_col = |c: usize| sel.ranges.iter().any(|r| r.cols().contains(&c) && r.start.row <= body_start && r.end.row + 1 >= rows_total);
    let whole_row = |r: usize| sel.ranges.iter().any(|g| g.rows().contains(&r) && g.start.col == 0 && g.end.col + 1 >= cols_total);
    for &(c, x0, x1) in &geo.cols {
        let r = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, geo.cells.top()));
        let selected = sel.ranges.iter().any(|g| g.cols().contains(&c));
        if whole_col(c) {
            p.rect_filled(r.intersect(head), 0.0, t.header_all_bg);
        } else if selected {
            p.rect_filled(r.intersect(head), 0.0, t.header_sel_bg);
        }
        let color = if selected { t.header_sel_text } else { t.header_text };
        let clip = p.with_clip_rect(r.intersect(head).shrink2(vec2(3.0 * z, 0.0)));
        if view.doc.dialect.header {
            clip.text(pos2(r.center().x, r.top() + 9.0 * z), Align2::CENTER_CENTER, column_label(c), small_font.clone(), t.text_dim);
            let name = view.header_name(c);
            if !name.is_empty() {
                clip.text(pos2(r.center().x, r.top() + 26.0 * z), Align2::CENTER_CENTER, display_text(name), name_font.clone(), color);
            }
        } else {
            clip.text(r.center(), Align2::CENTER_CENTER, column_label(c), label_font.clone(), color);
        }
        p.vline(x1, head.y_range(), hl);
    }
    for &(r, y0, y1) in &geo.rows {
        let g = Rect::from_min_max(pos2(rect.left(), y0), pos2(geo.cells.left(), y1));
        let selected = sel.ranges.iter().any(|k| k.rows().contains(&r));
        if whole_row(r) {
            p.rect_filled(g.intersect(gutter), 0.0, t.header_all_bg);
        } else if selected {
            p.rect_filled(g.intersect(gutter), 0.0, t.header_sel_bg);
        }
        let color = if selected { t.header_sel_text } else { t.header_text };
        p.with_clip_rect(g.intersect(gutter)).text(
            pos2(g.right() - 6.0 * z, g.center().y),
            Align2::RIGHT_CENTER,
            (r + 1).to_string(),
            label_font.clone(),
            color,
        );
        p.hline(gutter.x_range(), y1, hl);
    }
    p.rect_filled(Rect::from_min_max(rect.min, geo.cells.min), 0.0, t.header_bg);
    p.hline(rect.x_range(), geo.cells.top(), hl);
    p.vline(geo.cells.left(), rect.y_range(), hl);
}

/// Replaces, extends (Shift) or adds to (Ctrl) the selection.
fn pick(view: &mut DocView, range: Range, active: CellRef, shift_to: CellRef, mods: Modifiers) {
    if mods.shift {
        view.sel.extend_to(shift_to);
    } else if mods.command {
        view.sel.add(range, active);
    } else {
        view.sel = Selection { active, anchor: active, ranges: vec![range] };
    }
}

fn interact(grid: &mut GridState, view: &mut DocView, ui: &mut egui::Ui, resp: &Response, geo: &Geo) {
    let mods = ui.input(|i| i.modifiers);
    let max = view.max_cell();
    let body_start = view.body_start();
    let clamp = |c: CellRef| CellRef::new(c.row.clamp(body_start, max.row), c.col.min(max.col));
    let in_head = |p: Pos2| p.y < geo.cells.top() && p.x >= geo.cells.left();
    let in_gutter = |p: Pos2| p.x < geo.cells.left() && p.y >= geo.cells.top();
    let in_corner = |p: Pos2| p.x < geo.cells.left() && p.y < geo.cells.top();

    if let Some(h) = resp.hover_pos()
        && (matches!(grid.drag, Some(Drag::Resize { .. })) || (grid.drag.is_none() && in_head(h) && geo.col_edge_at(h.x).is_some()))
    {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
    }

    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        grid.drag = None;
        if in_head(p) {
            if let Some(col) = geo.col_edge_at(p.x) {
                autofit(grid, view, ui, col);
            } else if view.doc.dialect.header
                && let Some(col) = geo.col_at(p.x)
            {
                grid.rename_request = Some(col);
            }
            return;
        }
        if p.x >= geo.cells.left() && p.y >= geo.cells.top() {
            start_edit(grid, view, None);
            return;
        }
    }

    let press = if resp.drag_started() {
        ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos())
    } else if resp.clicked() || resp.secondary_clicked() {
        resp.interact_pointer_pos()
    } else {
        None
    };
    if let Some(p) = press {
        commit_edit(grid, view);
        resp.request_focus();
        let dragging = resp.drag_started();
        // A right-click on a cell outside the selection selects it; inside, it keeps the selection.
        if resp.secondary_clicked() {
            if let Some(c) = geo.cell_at(p)
                && !view.sel.contains(clamp(c))
            {
                view.sel = Selection::at(clamp(c));
            }
            return;
        }
        if in_corner(p) {
            let all = Range::new(CellRef::new(body_start, 0), max);
            view.sel = Selection { active: CellRef::new(body_start, 0), anchor: CellRef::new(body_start, 0), ranges: vec![all] };
            grid.drag = None;
        } else if in_head(p) {
            if let Some(col) = geo.col_edge_at(p.x) {
                if dragging {
                    grid.drag = Some(Drag::Resize { col, start_x: p.x, start_w: col_width(grid, view, col) });
                }
            } else if let Some(c) = geo.col_at(p.x) {
                let c = c.min(max.col);
                let top = CellRef::new(body_start, c);
                pick(view, Range::new(top, CellRef::new(max.row, c)), top, CellRef::new(max.row, c), mods);
                grid.drag = dragging.then_some(Drag::Cols);
            }
        } else if in_gutter(p) {
            if let Some(r) = geo.row_at(p.y) {
                let r = r.clamp(body_start, max.row);
                let left = CellRef::new(r, 0);
                pick(view, Range::new(left, CellRef::new(r, max.col)), left, CellRef::new(r, max.col), mods);
                grid.drag = dragging.then_some(Drag::Rows);
            }
        } else if let Some(c) = geo.cell_at(p) {
            let c = clamp(c);
            pick(view, Range::cell(c), c, c, mods);
            grid.drag = dragging.then_some(Drag::Cells);
        }
    }

    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match grid.drag {
            Some(Drag::Cells) => {
                if let Some(c) = geo.cell_near(p) {
                    view.sel.extend_to(clamp(c));
                }
            }
            Some(Drag::Rows) => {
                if let Some(c) = geo.cell_near(p) {
                    view.sel.extend_to(CellRef::new(c.row.clamp(body_start, max.row), max.col));
                }
            }
            Some(Drag::Cols) => {
                if let Some(c) = geo.cell_near(p) {
                    view.sel.extend_to(CellRef::new(max.row, c.col.min(max.col)));
                }
            }
            Some(Drag::Resize { col, start_x, start_w }) => {
                let w = (start_w + (p.x - start_x) / geo.z).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
                view.col_widths.insert(col, w);
            }
            None => {}
        }
        // Dragging past an edge scrolls.
        if grid.drag.is_some() && !matches!(grid.drag, Some(Drag::Resize { .. })) {
            if p.y > geo.cells.bottom() {
                view.scroll_row = view.scroll_row.saturating_add(1);
            } else if p.y < geo.cells.top() && !matches!(grid.drag, Some(Drag::Cols)) {
                view.scroll_row = view.scroll_row.saturating_sub(1).max(body_start);
            }
            if p.x > geo.cells.right() {
                view.scroll_col = view.scroll_col.saturating_add(1);
            } else if p.x < geo.cells.left() && !matches!(grid.drag, Some(Drag::Rows)) {
                view.scroll_col = view.scroll_col.saturating_sub(1);
            }
            ui.ctx().request_repaint();
        }
    }
    if resp.drag_stopped() {
        grid.drag = None;
    }

    if resp.hovered() || resp.dragged() {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        if zoom != 1.0 {
            grid.set_zoom(grid.zoom * zoom);
        } else if scroll != Vec2::ZERO {
            grid.wheel += scroll;
            let dy = (grid.wheel.y / geo.rh).trunc();
            if dy != 0.0 {
                grid.wheel.y -= dy * geo.rh;
                view.scroll_row = (view.scroll_row as i64 - dy as i64).max(body_start as i64) as usize;
            }
            let unit = (grid.default_col_width * geo.z).max(1.0);
            let dx = (grid.wheel.x / unit).trunc();
            if dx != 0.0 {
                grid.wheel.x -= dx * unit;
                view.scroll_col = (view.scroll_col as i64 - dx as i64).max(0) as usize;
            }
        }
    }

    if resp.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
        keyboard(grid, view, ui, geo);
    }
}

fn step(view: &mut DocView, dr: i64, dc: i64, shift: bool, jump: bool) {
    let max = view.max_cell();
    let body_start = view.body_start();
    let from = if shift { view.sel.moving_end() } else { view.sel.active };
    let to = if jump { selection::jump(&view.doc.table, from, dr, dc, max) } else { from.offset(dr, dc, max) };
    let to = CellRef::new(to.row.max(body_start), to.col);
    if shift { view.sel.extend_to(to) } else { view.sel = Selection::at(to) }
}

fn keyboard(grid: &mut GridState, view: &mut DocView, ui: &mut egui::Ui, geo: &Geo) {
    let (keys, typed): (Vec<(Key, Modifiers)>, String) = ui.input(|i| {
        let mut keys = Vec::new();
        let mut typed = String::new();
        for e in &i.events {
            match e {
                Event::Key { key, pressed: true, modifiers, .. } => keys.push((*key, *modifiers)),
                Event::Text(s) => typed.push_str(s),
                _ => {}
            }
        }
        (keys, typed)
    });
    if !typed.is_empty() && !typed.chars().all(char::is_control) {
        // The typed text opens the editor with that text; the text field must not see it again.
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(_))));
        start_edit(grid, view, Some(typed));
        ensure_visible(grid, view, geo);
        return;
    }
    if keys.is_empty() {
        return;
    }
    let body_start = view.body_start();
    let page = geo.full_rows().saturating_sub(1).max(1) as i64;
    for (key, m) in keys {
        let (ctrl, shift, alt) = (m.command, m.shift, m.alt);
        match key {
            Key::ArrowDown if alt => {
                crate::ops::move_rows(grid, view, 1);
            }
            Key::ArrowUp if alt => {
                crate::ops::move_rows(grid, view, -1);
            }
            Key::ArrowRight if alt => {
                crate::ops::move_cols(grid, view, 1);
            }
            Key::ArrowLeft if alt => {
                crate::ops::move_cols(grid, view, -1);
            }
            Key::ArrowDown => step(view, 1, 0, shift, ctrl),
            Key::ArrowUp => step(view, -1, 0, shift, ctrl),
            Key::ArrowLeft => step(view, 0, -1, shift, ctrl),
            Key::ArrowRight => step(view, 0, 1, shift, ctrl),
            Key::Delete | Key::Backspace => {
                crate::ops::clear(grid, view);
            }
            Key::F2 => start_edit(grid, view, None),
            Key::D if ctrl => {
                crate::ops::fill_down(grid, view);
            }
            Key::PageDown => step(view, page, 0, shift, false),
            Key::PageUp => step(view, -page, 0, shift, false),
            Key::Home => {
                let from = if shift { view.sel.moving_end() } else { view.sel.active };
                let dr = if ctrl { body_start as i64 - from.row as i64 } else { 0 };
                step(view, dr, -(from.col as i64), shift, false);
            }
            Key::End => {
                let from = if shift { view.sel.moving_end() } else { view.sel.active };
                let table = &view.doc.table;
                let (last_row, last_col) = (table.row_count().saturating_sub(1).max(body_start), table.width().saturating_sub(1));
                let (dr, dc) = if ctrl {
                    (last_row as i64 - from.row as i64, last_col as i64 - from.col as i64)
                } else {
                    let row_end = table.row(from.row).map_or(0, |r| r.iter().rposition(|c| !c.is_empty()).unwrap_or(0));
                    (0, row_end as i64 - from.col as i64)
                };
                step(view, dr, dc, shift, false);
            }
            Key::Tab => {
                if !view.sel.cycle(!shift, true) {
                    step(view, 0, if shift { -1 } else { 1 }, false, false);
                }
            }
            Key::Enter => {
                if !view.sel.cycle(!shift, false) {
                    step(view, if shift { -1 } else { 1 }, 0, false, false);
                }
            }
            Key::A if ctrl => {
                let table = &view.doc.table;
                let region = selection::current_region(table, view.sel.active);
                let region = Range::new(CellRef::new(region.start.row.max(body_start), region.start.col), region.end);
                let whole = Range::new(
                    CellRef::new(body_start, 0),
                    CellRef::new(table.row_count().saturating_sub(1).max(body_start), table.width().saturating_sub(1)),
                );
                let target = if region.is_single() || view.sel.current() == region { whole } else { region };
                let active = view.sel.active;
                view.sel = Selection { active, anchor: active, ranges: vec![target] };
            }
            Key::Escape => view.sel = Selection::at(view.sel.active),
            _ => continue,
        }
        ensure_visible(grid, view, geo);
    }
}

/// Scrolls so the active cell is fully visible.
fn ensure_visible(grid: &GridState, view: &mut DocView, geo: &Geo) {
    let a = view.sel.active;
    let full_rows = geo.full_rows();
    if a.row < view.scroll_row {
        view.scroll_row = a.row;
    } else if a.row >= view.scroll_row + full_rows {
        view.scroll_row = a.row + 1 - full_rows;
    }
    if a.col < view.scroll_col {
        view.scroll_col = a.col;
    } else {
        let avail = geo.cells.width() / geo.z;
        while view.scroll_col < a.col {
            let total: f32 = (view.scroll_col..=a.col).map(|c| col_width(grid, view, c)).sum();
            if total <= avail {
                break;
            }
            view.scroll_col += 1;
        }
    }
}

/// Sizes the column to its widest text (sampling the first few thousand rows).
pub fn autofit(grid: &GridState, view: &mut DocView, ui: &egui::Ui, col: usize) {
    let font = theme::cell_font(13.0);
    let table = &view.doc.table;
    let rows = table.row_count().min(5000);
    let mut widest: f32 = 0.0;
    ui.ctx().fonts_mut(|f| {
        for r in 0..rows {
            let text = table.get(r, col);
            if text.is_empty() {
                continue;
            }
            widest = widest.max(f.layout_no_wrap(display_text(text).into_owned(), font.clone(), Color32::WHITE).size().x);
        }
    });
    let w = if widest > 0.0 { widest + 12.0 } else { grid.default_col_width };
    view.col_widths.insert(col, w.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH));
}

fn thumb_colour(resp: &Response, t: &Tokens) -> Color32 {
    if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled }
}

fn vertical_scrollbar(view: &mut DocView, ui: &mut egui::Ui, track: Rect, geo: &Geo, t: &Tokens) {
    let body_start = view.body_start();
    let visible = geo.full_rows() as f32;
    let extent = view.extent_rows().saturating_sub(body_start) as f32;
    let max_scroll = (extent - visible).max(0.0);
    let scroll = view.scroll_row.saturating_sub(body_start) as f32;
    let id = ui.scope_id().with("vscroll");
    let resp = ui.interact(track, id, Sense::CLICK | Sense::DRAG);
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    let thumb_h = (track.height() * visible / extent.max(visible)).clamp(24.0_f32.min(track.height()), track.height());
    let travel = (track.height() - thumb_h).max(0.0);
    let thumb_top = track.top() + if max_scroll > 0.0 { travel * (scroll / max_scroll).clamp(0.0, 1.0) } else { 0.0 };
    let thumb = Rect::from_min_size(pos2(track.left() + 3.0, thumb_top), vec2((track.width() - 6.0).max(0.0), thumb_h));
    ui.painter().rect_filled(thumb, 4.0, thumb_colour(&resp, t));
    let start_id = id.with("start");
    let mut next = scroll;
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, scroll));
    }
    if resp.dragged()
        && travel > 0.0
        && let (Some(origin), Some(p)) = (ui.input(|i| i.pointer.press_origin()), resp.interact_pointer_pos())
    {
        let start = ui.ctx().data(|d| d.get_temp::<f32>(start_id)).unwrap_or(scroll);
        next = start + (p.y - origin.y) / travel * max_scroll;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.y < thumb.top() {
            next -= visible;
        } else if p.y > thumb.bottom() {
            next += visible;
        }
    }
    let next = next.round().clamp(0.0, max_scroll);
    if next != scroll {
        view.scroll_row = body_start.saturating_add(next as usize);
        ui.ctx().request_repaint();
    }
}

fn horizontal_scrollbar(grid: &GridState, view: &mut DocView, ui: &mut egui::Ui, track: Rect, geo: &Geo, t: &Tokens) {
    let visible = geo.full_cols() as f32;
    let extent = view.extent_cols() as f32;
    let max_scroll = (extent - visible).max(0.0);
    let scroll = view.scroll_col as f32;
    let id = ui.scope_id().with("hscroll");
    let resp = ui.interact(track, id, Sense::CLICK | Sense::DRAG);
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    let thumb_w = (track.width() * visible / extent.max(visible)).clamp(24.0_f32.min(track.width()), track.width());
    let travel = (track.width() - thumb_w).max(0.0);
    let thumb_left = track.left() + if max_scroll > 0.0 { travel * (scroll / max_scroll).clamp(0.0, 1.0) } else { 0.0 };
    let thumb = Rect::from_min_size(pos2(thumb_left, track.top() + 3.0), vec2(thumb_w, (track.height() - 6.0).max(0.0)));
    ui.painter().rect_filled(thumb, 4.0, thumb_colour(&resp, t));
    let start_id = id.with("start");
    let mut next = scroll;
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, scroll));
    }
    if resp.dragged()
        && travel > 0.0
        && let (Some(origin), Some(p)) = (ui.input(|i| i.pointer.press_origin()), resp.interact_pointer_pos())
    {
        let start = ui.ctx().data(|d| d.get_temp::<f32>(start_id)).unwrap_or(scroll);
        next = start + (p.x - origin.x) / travel * max_scroll;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.x < thumb.left() {
            next -= visible;
        } else if p.x > thumb.right() {
            next += visible;
        }
    }
    let _ = grid;
    let next = next.round().clamp(0.0, max_scroll);
    if next != scroll {
        view.scroll_col = next as usize;
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_recognised_conservatively() {
        assert!(looks_numeric("12") && looks_numeric(" -3.5 ") && looks_numeric("+1e3") && looks_numeric(".5"));
        assert!(!looks_numeric("abc") && !looks_numeric("inf") && !looks_numeric("1,000") && !looks_numeric("") && !looks_numeric("007x"));
    }

    #[test]
    fn display_text_flattens_line_breaks() {
        assert_eq!(display_text("a\r\nb\nc"), "a\u{00B6}b\u{00B6}c");
        assert!(matches!(display_text("plain"), std::borrow::Cow::Borrowed("plain")));
    }

    #[test]
    fn zoom_snaps_and_clamps() {
        let mut g = GridState::default();
        g.set_zoom(1.23);
        assert_eq!(g.zoom, 1.25);
        g.set_zoom(9.0);
        assert_eq!(g.zoom, MAX_ZOOM);
        g.set_zoom(f32::NAN);
        assert_eq!(g.zoom, MAX_ZOOM);
        g.zoom_step(-30);
        assert_eq!(g.zoom, MIN_ZOOM);
    }

    #[test]
    fn geometry_clamps_scroll_and_fills_the_viewport() {
        use csv_editor_table::{Document, ParseOptions};
        let doc = Document::from_bytes(b"h1,h2\n1,2\n3,4\n", &ParseOptions::default()).unwrap();
        let mut view = DocView::new(doc);
        let grid = GridState::default();
        view.scroll_row = 0; // the header row is not scrollable into the body
        view.scroll_col = 999;
        let geo = geometry(&grid, &mut view, Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)));
        assert_eq!(view.scroll_row, 1);
        assert_eq!(view.scroll_col, view.extent_cols() - 1);
        assert!(geo.rows.len() >= 10 && !geo.cols.is_empty());
        assert_eq!(geo.rows.first().map(|r| r.0), Some(1));
        assert!(geo.cell_at(geo.cells.min).is_some());
        assert!(geo.cell_at(pos2(-1.0, -1.0)).is_none());
        let far = Range::new(CellRef::new(500, 500), CellRef::new(600, 600));
        assert!(geo.range_rect(far).is_none());
    }
}
