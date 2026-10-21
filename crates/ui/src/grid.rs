//! The grid: row gutter, column headers and cells. Slice 1 paints an empty sheet; data, scrolling
//! and selection arrive with later slices (see `.agents/plans/csv-editor.md`).

use egui::{Align2, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::EditorApp;
use crate::theme::Tokens;

#[derive(Clone, Debug)]
pub struct GridState {
    pub default_col_width: f32,
    pub row_height: f32,
    pub gutter_width: f32,
    pub header_height: f32,
    pub zoom: f32,
}

impl Default for GridState {
    fn default() -> Self {
        GridState { default_col_width: 96.0, row_height: 22.0, gutter_width: 48.0, header_height: 24.0, zoom: 1.0 }
    }
}

pub fn show(app: &mut EditorApp, ui: &mut egui::Ui, t: &Tokens) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let _resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let g = &app.grid;
    let z = g.zoom;
    let (gutter, head_h, cw, rh) = (g.gutter_width * z, g.header_height * z, g.default_col_width * z, g.row_height * z);
    let origin: Pos2 = pos2(rect.left() + gutter, rect.top() + head_h);

    p.rect_filled(rect, 0.0, t.grid_bg);
    // Header strips.
    p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.right(), origin.y)), 0.0, t.header_bg);
    p.rect_filled(Rect::from_min_max(rect.min, pos2(origin.x, rect.bottom())), 0.0, t.header_bg);

    let cols = ((rect.right() - origin.x) / cw).ceil().max(0.0) as usize;
    let rows = ((rect.bottom() - origin.y) / rh).ceil().max(0.0) as usize;
    let gridline = Stroke::new(1.0, t.gridline);
    let header_line = Stroke::new(1.0, t.header_line);
    let font = crate::theme::ui_font(12.0 * z);

    for c in 0..=cols {
        let x = origin.x + c as f32 * cw;
        p.line_segment([pos2(x, origin.y), pos2(x, rect.bottom())], gridline);
        p.line_segment([pos2(x, rect.top()), pos2(x, origin.y)], header_line);
        if c < cols {
            let label = csv_editor_table::column_label(c);
            p.text(pos2(x + cw / 2.0, rect.top() + head_h / 2.0), Align2::CENTER_CENTER, label, font.clone(), t.header_text);
        }
    }
    for r in 0..=rows {
        let y = origin.y + r as f32 * rh;
        p.line_segment([pos2(origin.x, y), pos2(rect.right(), y)], gridline);
        p.line_segment([pos2(rect.left(), y), pos2(origin.x, y)], header_line);
        if r < rows {
            p.text(pos2(rect.left() + gutter / 2.0, y + rh / 2.0), Align2::CENTER_CENTER, (r + 1).to_string(), font.clone(), t.header_text);
        }
    }
    // The header corner and its outer lines.
    p.rect_filled(Rect::from_min_size(rect.min, vec2(gutter, head_h)), 0.0, t.header_bg);
    p.line_segment([pos2(rect.left(), origin.y), pos2(rect.right(), origin.y)], header_line);
    p.line_segment([pos2(origin.x, rect.top()), pos2(origin.x, rect.bottom())], header_line);
}
