//! The grid: row gutter, column headers, cells, selection, scrolling, column and row sizing.
//!
//! Only the visible rows and columns are laid out and painted. Scrolling is by whole rows and
//! columns (`DocView::scroll` / `scroll_col`), so a million rows never meet f32 precision. Rows
//! hidden by a filter are skipped: the grid walks *display rows* (see `DocView`). The first row
//! can be frozen. The scrollbars are drawn by hand (after GridCraft's `grid.rs`, MIT, see
//! `ATTRIBUTION.md`).

use std::rc::Rc;

use csv_editor_table::{CellRef, Range, Row, Selection, column_label, selection};
use egui::{Align2, Color32, CursorIcon, Event, EventFilter, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::EditorApp;
use crate::theme::{self, Tokens};
use crate::view::DocView;

pub const SCROLLBAR: f32 = 12.0;
const RESIZE_GRIP: f32 = 4.0;
pub const MIN_COL_WIDTH: f32 = 20.0;
pub const MAX_COL_WIDTH: f32 = 2000.0;
pub const MIN_ROW_HEIGHT: f32 = 12.0;
pub const MAX_ROW_HEIGHT: f32 = 800.0;
pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 2.0;
/// Cell text size in unzoomed points; one text line in a cell is about 1.3× this.
const CELL_FONT: f32 = 13.0;
const LINE_HEIGHT: f32 = 17.0;

#[derive(Clone)]
pub struct GridState {
    pub default_col_width: f32,
    pub row_height: f32,
    pub header_height: f32,
    pub zoom: f32,
    /// The in-cell editor, while a cell is being edited.
    pub editor: Option<CellEditor>,
    /// A request to open the row-filter dialog for this column, picked up by the app.
    pub filter_request: Option<usize>,
    /// Scroll so the active cell is in view on the next frame (set by find, sort, filters).
    pub scroll_to_active: bool,
    /// Requests for the app's dialogs: the dialect (delimiter and encoding) and go-to-row.
    pub dialect_request: bool,
    pub goto_request: bool,
    /// A short result to show in the status bar ("3 rows removed"), picked up by the app.
    pub note: Option<String>,
    /// Where the last right-click landed, deciding which context menu opens.
    pub context_zone: Zone,
    /// The last copied or cut block, kept as cells so a paste within the editor is exact.
    pub clip: Option<Clip>,
    /// Reads the system clipboard, for the menu's Paste (Ctrl+V arrives as an egui event).
    pub paste_text: Option<Rc<dyn Fn() -> Option<String>>>,
    drag: Option<Drag>,
    /// Wheel movement not yet turned into whole rows and columns.
    wheel: Vec2,
}

/// A part of the grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zone {
    #[default]
    Cells,
    Gutter,
    Head,
}

/// A copied or cut block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    /// The block as tab-separated text, as handed to the system clipboard.
    pub text: String,
    pub cells: Vec<Row>,
    /// The source range of a cut, emptied when the block is pasted.
    pub cut: Option<Range>,
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
    ColResize { col: usize, start_x: f32, start_w: f32 },
    RowResize { row: usize, start_y: f32, start_h: f32 },
}

impl Default for GridState {
    fn default() -> Self {
        GridState {
            default_col_width: 96.0,
            row_height: 22.0,
            header_height: 24.0,
            zoom: 1.0,
            editor: None,
            drag: None,
            wheel: Vec2::ZERO,
            clip: None,
            paste_text: None,
            filter_request: None,
            scroll_to_active: false,
            dialect_request: false,
            goto_request: false,
            note: None,
            context_zone: Zone::Cells,
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
    /// The default row height, zoomed.
    pub rh: f32,
    /// Height of the frozen first row, 0 when none.
    pub frozen_h: f32,
    /// Visible columns: index and horizontal extent (the last one may be partly hidden).
    pub cols: Vec<(usize, f32, f32)>,
    /// Visible rows: index and vertical extent (the frozen row first, when there is one).
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
        let first_row = self.rows.iter().map(|k| k.0).min()?;
        let last_row = self.rows.iter().map(|k| k.0).max()?;
        if r.end.col < first_col || r.start.col > last_col || r.end.row < first_row || r.start.row > last_row {
            return None;
        }
        let x0 = if r.start.col < first_col { self.cells.left() - 4.0 } else { self.cols.iter().find(|k| k.0 == r.start.col)?.1 };
        let x1 = if r.end.col > last_col { self.cells.right() + 4.0 } else { self.cols.iter().find(|k| k.0 == r.end.col)?.2 };
        let y0 = match self.rows.iter().find(|k| k.0 == r.start.row) {
            Some(k) => k.1,
            None => self.cells.top() + self.frozen_h - 4.0,
        };
        let y1 = match self.rows.iter().find(|k| k.0 == r.end.row) {
            Some(k) => k.2,
            None => self.cells.bottom() + 4.0,
        };
        Some(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)))
    }

    /// The column whose right edge is within the resize grip of `x`.
    fn col_edge_at(&self, x: f32) -> Option<usize> {
        self.cols.iter().find(|(_, _, x1)| (x - x1).abs() <= RESIZE_GRIP).map(|c| c.0)
    }

    /// The row whose bottom edge is within the resize grip of `y`.
    fn row_edge_at(&self, y: f32) -> Option<usize> {
        self.rows.iter().find(|(_, _, y1)| (y - y1).abs() <= RESIZE_GRIP).map(|r| r.0)
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

fn row_height(grid: &GridState, view: &DocView, r: usize) -> f32 {
    view.row_heights.get(&r).copied().unwrap_or(grid.row_height)
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

/// How many text lines a cell has (capped, so a pasted novel does not make a mile-high row).
fn line_count(text: &str) -> usize {
    text.split('\n').count().clamp(1, 40)
}

/// The height a row needs to show every line of its tallest cell.
fn fit_height(lines: usize, base: f32) -> f32 {
    if lines <= 1 { base } else { base + (lines - 1) as f32 * LINE_HEIGHT }
}

fn geometry(grid: &GridState, view: &mut DocView, rect: Rect) -> Geo {
    let z = grid.zoom;
    let rh = (grid.row_height * z).max(4.0);
    let head_h = grid.header_height * z;
    let digits = view.extent_rows().to_string().len() as f32;
    let gutter_w = (digits * 8.0 + 16.0).max(40.0) * z;
    let cells = Rect::from_min_max(pos2(rect.left() + gutter_w, rect.top() + head_h), rect.max);
    view.refresh_visible();
    let frozen = view.freeze_top && view.display_count() > 1;
    let mut rows = Vec::new();
    let mut y = cells.top();
    let mut frozen_h = 0.0;
    if frozen {
        let r = view.row_at_display(0);
        let h = (row_height(grid, view, r) * z).max(4.0);
        rows.push((r, y, y + h));
        y += h;
        frozen_h = h;
    }
    let min_scroll = usize::from(frozen);
    let visible_rows = (((cells.height() - frozen_h) / rh).floor().max(1.0)) as usize;
    let max_scroll = view.display_count().saturating_sub(visible_rows).max(min_scroll);
    view.scroll = view.scroll.clamp(min_scroll, max_scroll);
    view.scroll_col = view.scroll_col.min(view.extent_cols().saturating_sub(1));
    let mut i = view.scroll;
    while y < cells.bottom() && rows.len() < 10_000 {
        let r = view.row_at_display(i);
        let h = (row_height(grid, view, r) * z).max(4.0);
        rows.push((r, y, y + h));
        y += h;
        i = i.saturating_add(1);
    }
    let mut cols = Vec::new();
    let (mut x, mut c) = (cells.left(), view.scroll_col);
    while x < cells.right() && cols.len() < 10_000 {
        let w = (col_width(grid, view, c) * z).max(1.0);
        cols.push((c, x, x + w));
        x += w;
        c = c.saturating_add(1);
    }
    Geo { rect, cells, z, rh, frozen_h, cols, rows }
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
    if std::mem::take(&mut grid.scroll_to_active) {
        ensure_visible(grid, view, &geo);
    }
    interact(grid, view, ui, &resp, &geo);
    // Interaction may have scrolled or resized: lay out again before painting.
    let geo = geometry(grid, view, rect);
    paint(ui, &geo, view, t);
    vertical_scrollbar(view, ui, vtrack, &geo, t);
    horizontal_scrollbar(view, ui, htrack, &geo, t);
    ui.painter().rect_filled(Rect::from_min_max(pos2(rect.right(), rect.bottom()), full.max), 0.0, t.tab_bar);
    editor_ui(grid, view, ui, &resp, &geo, t);
    resp.context_menu(|ui| match grid.context_zone {
        Zone::Cells => crate::ops::context_menu(grid, view, ui),
        Zone::Gutter => crate::ops::rows_menu(grid, view, ui),
        Zone::Head => crate::ops::header_context_menu(grid, view, ui),
    });
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
    let font = theme::cell_font(CELL_FONT * z);
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
    let font = theme::cell_font(CELL_FONT * z);
    let pad = 4.0 * z;
    let table = &view.doc.table;
    let header = view.doc.dialect.header;
    for &(r, y0, y1) in &geo.rows {
        let tall = (y1 - y0) > geo.rh * 1.4;
        for &(c, x0, x1) in &geo.cols {
            let text = table.get(r, c);
            if text.is_empty() {
                continue;
            }
            let cell = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            let clip = cell.intersect(geo.cells).shrink2(vec2(pad, 0.0));
            if clip.width() <= 0.0 || clip.height() <= 0.0 {
                continue;
            }
            let color = if header && r == 0 { t.header_text } else { t.cell_text };
            if tall {
                // A row sized to its content shows every line.
                let shown = text.replace("\r\n", "\n").replace('\r', "\n");
                p.with_clip_rect(clip).text(pos2(clip.left(), cell.top() + 3.0 * z), Align2::LEFT_TOP, shown, font.clone(), color);
                continue;
            }
            let shown = display_text(text);
            let (pos, anchor) = if looks_numeric(text) {
                (pos2(clip.right(), cell.center().y), Align2::RIGHT_CENTER)
            } else {
                (pos2(clip.left(), cell.center().y), Align2::LEFT_CENTER)
            };
            p.with_clip_rect(clip).text(pos, anchor, shown, font.clone(), color);
        }
    }
    if header && let Some(&(0, y0, y1)) = geo.rows.iter().find(|k| k.0 == 0) {
        // The header row reads as one: a soft band behind it.
        let band = Rect::from_min_max(pos2(geo.cells.left(), y0), pos2(geo.cells.right(), y1));
        body.rect_filled(band, 0.0, t.sel_fill.linear_multiply(0.5));
    }
    let gl = Stroke::new(1.0, t.gridline);
    for &(_, _, x1) in &geo.cols {
        body.vline(x1, geo.cells.y_range(), gl);
    }
    for &(_, _, y1) in &geo.rows {
        body.hline(geo.cells.x_range(), y1, gl);
    }
    if geo.frozen_h > 0.0 {
        body.hline(geo.cells.x_range(), geo.cells.top() + geo.frozen_h, Stroke::new(1.5, t.header_line));
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
    let (rows_total, cols_total) = (view.extent_rows(), view.extent_cols());
    let sel = &view.sel;
    let label_font = theme::ui_font(12.0 * z);
    let whole_col = |c: usize| sel.ranges.iter().any(|r| r.cols().contains(&c) && r.start.row == 0 && r.end.row + 1 >= rows_total);
    let whole_row = |r: usize| sel.ranges.iter().any(|g| g.rows().contains(&r) && g.start.col == 0 && g.end.col + 1 >= cols_total);
    for &(c, x0, x1) in &geo.cols {
        let r = Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, geo.cells.top()));
        let selected = sel.ranges.iter().any(|g| g.cols().contains(&c));
        if whole_col(c) {
            p.rect_filled(r.intersect(head), 0.0, t.header_all_bg);
        } else if selected {
            p.rect_filled(r.intersect(head), 0.0, t.header_sel_bg);
        } else if view.filters.iter().any(|f| f.col == c) {
            p.rect_filled(r.intersect(head), 0.0, t.accent_soft);
        }
        let color = if selected { t.header_sel_text } else { t.header_text };
        p.with_clip_rect(r.intersect(head).shrink2(vec2(3.0 * z, 0.0))).text(
            r.center(),
            Align2::CENTER_CENTER,
            column_label(c),
            label_font.clone(),
            color,
        );
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

/// The columns a double-click on `col`'s edge fits: every selected column when the edge belongs
/// to one, else just that column.
fn autofit_targets(view: &DocView, index: usize, by_rows: bool) -> Vec<usize> {
    let selected = if by_rows { crate::ops::selected_rows(view) } else { crate::ops::selected_cols(view) };
    if selected.binary_search(&index).is_ok() { selected } else { vec![index] }
}

fn interact(grid: &mut GridState, view: &mut DocView, ui: &mut egui::Ui, resp: &Response, geo: &Geo) {
    let mods = ui.input(|i| i.modifiers);
    let max = view.max_cell();
    let clamp = |c: CellRef| CellRef::new(c.row.min(max.row), c.col.min(max.col));
    let in_head = |p: Pos2| p.y < geo.cells.top() && p.x >= geo.cells.left();
    let in_gutter = |p: Pos2| p.x < geo.cells.left() && p.y >= geo.cells.top();
    let in_corner = |p: Pos2| p.x < geo.cells.left() && p.y < geo.cells.top();

    if let Some(h) = resp.hover_pos() {
        if matches!(grid.drag, Some(Drag::ColResize { .. })) || (grid.drag.is_none() && in_head(h) && geo.col_edge_at(h.x).is_some()) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeColumn);
        } else if matches!(grid.drag, Some(Drag::RowResize { .. })) || (grid.drag.is_none() && in_gutter(h) && geo.row_edge_at(h.y).is_some()) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeRow);
        }
    }

    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        grid.drag = None;
        if in_head(p) {
            if let Some(col) = geo.col_edge_at(p.x) {
                autofit_cols(grid, view, ui, &autofit_targets(view, col, false));
            }
            return;
        }
        if in_gutter(p) {
            if let Some(row) = geo.row_edge_at(p.y) {
                autofit_rows(grid, view, &autofit_targets(view, row, true));
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
        // A right-click outside the selection selects what is under it (cell, row or column);
        // inside, it keeps the selection. Where it landed decides the context menu.
        if resp.secondary_clicked() {
            if in_head(p) {
                grid.context_zone = Zone::Head;
                if let Some(c) = geo.col_at(p.x) {
                    let c = c.min(max.col);
                    if !view.sel.ranges.iter().any(|r| r.cols().contains(&c) && r.start.row == 0 && r.end.row >= max.row) {
                        let top = CellRef::new(0, c);
                        view.sel = Selection { active: top, anchor: top, ranges: vec![Range::new(top, CellRef::new(max.row, c))] };
                    }
                }
            } else if in_gutter(p) {
                grid.context_zone = Zone::Gutter;
                if let Some(r) = geo.row_at(p.y) {
                    let r = r.min(max.row);
                    if !view.sel.ranges.iter().any(|g| g.rows().contains(&r) && g.start.col == 0 && g.end.col >= max.col) {
                        let left = CellRef::new(r, 0);
                        view.sel = Selection { active: left, anchor: left, ranges: vec![Range::new(left, CellRef::new(r, max.col))] };
                    }
                }
            } else {
                grid.context_zone = Zone::Cells;
                if let Some(c) = geo.cell_at(p)
                    && !view.sel.contains(clamp(c))
                {
                    view.sel = Selection::at(clamp(c));
                }
            }
            return;
        }
        if in_corner(p) {
            let all = Range::new(CellRef::default(), max);
            view.sel = Selection { active: CellRef::default(), anchor: CellRef::default(), ranges: vec![all] };
            grid.drag = None;
        } else if in_head(p) {
            if let Some(col) = geo.col_edge_at(p.x) {
                if dragging {
                    grid.drag = Some(Drag::ColResize { col, start_x: p.x, start_w: col_width(grid, view, col) });
                }
            } else if let Some(c) = geo.col_at(p.x) {
                let c = c.min(max.col);
                let top = CellRef::new(0, c);
                pick(view, Range::new(top, CellRef::new(max.row, c)), top, CellRef::new(max.row, c), mods);
                grid.drag = dragging.then_some(Drag::Cols);
            }
        } else if in_gutter(p) {
            if let Some(row) = geo.row_edge_at(p.y) {
                if dragging {
                    grid.drag = Some(Drag::RowResize { row, start_y: p.y, start_h: row_height(grid, view, row) });
                }
            } else if let Some(r) = geo.row_at(p.y) {
                let r = r.min(max.row);
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
                    view.sel.extend_to(CellRef::new(c.row.min(max.row), max.col));
                }
            }
            Some(Drag::Cols) => {
                if let Some(c) = geo.cell_near(p) {
                    view.sel.extend_to(CellRef::new(max.row, c.col.min(max.col)));
                }
            }
            Some(Drag::ColResize { col, start_x, start_w }) => {
                let w = (start_w + (p.x - start_x) / geo.z).clamp(MIN_COL_WIDTH, MAX_COL_WIDTH);
                view.col_widths.insert(col, w);
            }
            Some(Drag::RowResize { row, start_y, start_h }) => {
                let h = (start_h + (p.y - start_y) / geo.z).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
                view.row_heights.insert(row, h);
            }
            None => {}
        }
        // Dragging past an edge scrolls.
        if matches!(grid.drag, Some(Drag::Cells | Drag::Rows | Drag::Cols)) {
            if p.y > geo.cells.bottom() {
                view.scroll = view.scroll.saturating_add(1);
            } else if p.y < geo.cells.top() && !matches!(grid.drag, Some(Drag::Cols)) {
                view.scroll = view.scroll.saturating_sub(1);
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
                view.scroll = (view.scroll as i64 - dy as i64).max(0) as usize;
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
    let from = if shift { view.sel.moving_end() } else { view.sel.active };
    let to = if jump {
        let t = selection::jump(&view.doc.table, from, dr, dc, max);
        CellRef::new(view.snap_visible(t.row), t.col)
    } else if dr != 0 && view.filtered() {
        // Row steps walk the displayed rows, so hidden rows are skipped.
        let d = view.display_of_row(from.row) as i64 + dr;
        let d = d.clamp(0, view.display_count().saturating_sub(1) as i64) as usize;
        CellRef::new(view.row_at_display(d), from.offset(0, dc, max).col)
    } else {
        from.offset(dr, dc, max)
    };
    if shift { view.sel.extend_to(to) } else { view.sel = Selection::at(to) }
}

fn keyboard(grid: &mut GridState, view: &mut DocView, ui: &mut egui::Ui, geo: &Geo) {
    let (keys, typed, copy, cut, paste): (Vec<(Key, Modifiers)>, String, bool, bool, Option<String>) = ui.input(|i| {
        let mut keys = Vec::new();
        let mut typed = String::new();
        let (mut copy, mut cut, mut paste) = (false, false, None);
        for e in &i.events {
            match e {
                Event::Key { key, pressed: true, modifiers, .. } => keys.push((*key, *modifiers)),
                Event::Text(s) => typed.push_str(s),
                Event::Copy => copy = true,
                Event::Cut => cut = true,
                Event::Paste(s) => paste = Some(s.clone()),
                _ => {}
            }
        }
        (keys, typed, copy, cut, paste)
    });
    if copy {
        crate::ops::copy(grid, view, ui.ctx());
    }
    if cut {
        crate::ops::cut(grid, view, ui.ctx());
    }
    if let Some(text) = paste {
        crate::ops::paste(grid, view, Some(text));
        ensure_visible(grid, view, geo);
    }
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
                let dr = if ctrl { -(from.row as i64) } else { 0 };
                step(view, dr, -(from.col as i64), shift, false);
            }
            Key::End => {
                let from = if shift { view.sel.moving_end() } else { view.sel.active };
                let table = &view.doc.table;
                let (last_row, last_col) = (table.row_count().saturating_sub(1), table.width().saturating_sub(1));
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
                let whole = Range::new(CellRef::default(), CellRef::new(table.row_count().saturating_sub(1), table.width().saturating_sub(1)));
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
    let d = view.display_of_row(a.row);
    let frozen = view.freeze_top && view.display_count() > 1;
    if !(frozen && d == 0) {
        let min_scroll = usize::from(frozen);
        let full_rows = geo.full_rows().saturating_sub(usize::from(frozen)).max(1);
        if d < view.scroll {
            view.scroll = d.max(min_scroll);
        } else if d >= view.scroll + full_rows {
            view.scroll = (d + 1 - full_rows).max(min_scroll);
        }
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

/// Sizes the columns to their widest text (sampling the first few thousand rows).
pub fn autofit_cols(grid: &GridState, view: &mut DocView, ui: &egui::Ui, cols: &[usize]) {
    let font = theme::cell_font(CELL_FONT);
    let table = &view.doc.table;
    let rows = table.row_count().min(5000);
    let mut widths = Vec::with_capacity(cols.len());
    ui.ctx().fonts_mut(|f| {
        for &col in cols.iter().take(10_000) {
            let mut widest: f32 = 0.0;
            for r in 0..rows {
                let text = table.get(r, col);
                if text.is_empty() {
                    continue;
                }
                let widest_line = text
                    .split('\n')
                    .map(|line| f.layout_no_wrap(display_text(line).into_owned(), font.clone(), Color32::WHITE).size().x)
                    .fold(0.0, f32::max);
                widest = widest.max(widest_line);
            }
            widths.push((col, if widest > 0.0 { widest + 12.0 } else { grid.default_col_width }));
        }
    });
    for (col, w) in widths {
        view.col_widths.insert(col, w.clamp(MIN_COL_WIDTH, MAX_COL_WIDTH));
    }
}

/// Sizes the rows to show every line of their cells; a one-line row goes back to the default.
pub fn autofit_rows(grid: &GridState, view: &mut DocView, rows: &[usize]) {
    let table = &view.doc.table;
    let mut heights = Vec::with_capacity(rows.len());
    for &r in rows.iter().take(100_000) {
        let lines = table.row(r).map_or(1, |row| row.iter().map(|c| line_count(c)).max().unwrap_or(1));
        heights.push((r, lines));
    }
    for (r, lines) in heights {
        if lines <= 1 {
            view.row_heights.remove(&r);
        } else {
            view.row_heights.insert(r, fit_height(lines, grid.row_height).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT));
        }
    }
}

/// Puts the rows back at the default height.
pub fn reset_row_heights(view: &mut DocView, rows: &[usize]) {
    for r in rows {
        view.row_heights.remove(r);
    }
}

fn thumb_colour(resp: &Response, t: &Tokens) -> Color32 {
    if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled }
}

fn vertical_scrollbar(view: &mut DocView, ui: &mut egui::Ui, track: Rect, geo: &Geo, t: &Tokens) {
    let min_scroll = usize::from(geo.frozen_h > 0.0);
    let visible = geo.full_rows().saturating_sub(min_scroll).max(1) as f32;
    let extent = view.display_count().saturating_sub(min_scroll) as f32;
    let max_scroll = (extent - visible).max(0.0);
    let scroll = view.scroll.saturating_sub(min_scroll) as f32;
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
        view.scroll = min_scroll + next as usize;
        ui.ctx().request_repaint();
    }
}

fn horizontal_scrollbar(view: &mut DocView, ui: &mut egui::Ui, track: Rect, geo: &Geo, t: &Tokens) {
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
    let next = next.round().clamp(0.0, max_scroll);
    if next != scroll {
        view.scroll_col = next as usize;
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv_editor_table::{Document, ParseOptions};

    fn view(text: &str) -> DocView {
        DocView::new(Document::from_bytes(text.as_bytes(), &ParseOptions::default()).unwrap())
    }

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
        let mut view = view("h1,h2\n1,2\n3,4\n");
        let grid = GridState::default();
        view.scroll = 999; // far past the end: clamped so the last rows fill the view
        view.scroll_col = 999;
        let geo = geometry(&grid, &mut view, Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)));
        assert_eq!(view.scroll, view.display_count() - geo.rows.len() + 1);
        assert_eq!(view.scroll_col, view.extent_cols() - 1);
        assert!(geo.rows.len() >= 10 && !geo.cols.is_empty());
        view.scroll = 0;
        let geo = geometry(&grid, &mut view, Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)));
        assert_eq!(geo.rows.first().map(|r| r.0), Some(0), "row 1 is in the grid");
        assert!(geo.cell_at(geo.cells.min).is_some());
        assert!(geo.cell_at(pos2(-1.0, -1.0)).is_none());
        let far = Range::new(CellRef::new(500, 500), CellRef::new(600, 600));
        assert!(geo.range_rect(far).is_none());
    }

    #[test]
    fn a_frozen_top_row_stays_while_the_rest_scrolls() {
        let mut view = view("h\n1\n2\n3\n4\n5\n");
        let grid = GridState::default();
        view.freeze_top = true;
        view.scroll = 3;
        let geo = geometry(&grid, &mut view, Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0)));
        assert_eq!(geo.rows.first().map(|r| r.0), Some(0), "the frozen row comes first");
        assert_eq!(geo.rows.get(1).map(|r| r.0), Some(3), "then the scrolled-to row");
        assert!(geo.frozen_h > 0.0);
        view.scroll = 0;
        let geo = geometry(&grid, &mut view, Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0)));
        assert_eq!(view.scroll, 1, "scrolling never puts a second copy of the frozen row under it");
        assert_eq!(geo.rows.get(1).map(|r| r.0), Some(1));
    }

    #[test]
    fn row_autofit_follows_line_counts() {
        let mut view = view("a,b\none,two\nthree\nlines,x\n");
        view.doc.set_cell(1, 1, "two\nlines\nhere");
        let grid = GridState::default();
        autofit_rows(&grid, &mut view, &[0, 1, 2]);
        assert!(!view.row_heights.contains_key(&0), "a one-line row keeps the default height");
        assert_eq!(view.row_heights.get(&1).copied(), Some(fit_height(3, grid.row_height)));
        reset_row_heights(&mut view, &[1]);
        assert!(view.row_heights.is_empty());
        assert_eq!(line_count("x\n".repeat(100).as_str()), 40, "capped");
    }
}
