//! The find and replace bar.

use csv_editor_table::{CellRef, FindOptions, Matcher, Range, Selection, search};
use egui::{Key, Ui};

use crate::grid::{self, GridState};
use crate::theme::Tokens;
use crate::view::DocView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    Selection,
    Column,
}

/// Matches past this many are not counted (the count is a hint, not a job).
const COUNT_CAP: usize = 100_000;

#[derive(Default)]
pub struct FindBar {
    pub open: bool,
    /// Also show the replace row.
    pub replace: bool,
    pub opts: FindOptions,
    pub replacement: String,
    pub scope: Option<Scope>,
    /// One-line result of the last action ("3 replaced", "Not found").
    pub status: String,
    matcher: Option<Result<Matcher, String>>,
    compiled_for: Option<FindOptions>,
    count: Option<(FindOptions, Option<Scope>, u64, usize)>,
    focus_query: bool,
}

impl FindBar {
    pub fn show(&mut self, replace: bool) {
        self.open = true;
        self.replace |= replace;
        self.focus_query = true;
        self.status.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.status.clear();
    }

    fn scope(&self, view: &DocView) -> Scope {
        self.scope.unwrap_or(if view.sel.is_single_cell() { Scope::All } else { Scope::Selection })
    }

    fn scope_range(&self, view: &DocView) -> Range {
        let table = &view.doc.table;
        let (start, last_row, last_col) = (view.body_start(), table.row_count().saturating_sub(1), table.width().saturating_sub(1));
        match self.scope(view) {
            Scope::All => Range::new(CellRef::new(start, 0), CellRef::new(last_row.max(start), last_col)),
            Scope::Selection => view.sel.bounds(),
            Scope::Column => Range::new(CellRef::new(start, view.sel.active.col), CellRef::new(last_row.max(start), view.sel.active.col)),
        }
    }

    fn matcher(&mut self) -> Option<Matcher> {
        if self.compiled_for.as_ref() != Some(&self.opts) {
            self.matcher = Some(Matcher::new(&self.opts));
            self.compiled_for = Some(self.opts.clone());
        }
        match &self.matcher {
            Some(Ok(m)) => Some(m.clone()),
            Some(Err(e)) => {
                if !self.opts.query.is_empty() {
                    self.status = e.clone();
                }
                None
            }
            None => None,
        }
    }

    fn count(&mut self, view: &DocView) -> Option<usize> {
        let key = (self.opts.clone(), self.scope, view.doc.history.version());
        if let Some((o, s, v, n)) = &self.count
            && (o, *s, *v) == (&key.0, key.1, key.2)
        {
            return Some(*n);
        }
        let m = self.matcher()?;
        let scope = self.scope_range(view);
        let n = search::count_matches(&view.doc.table, &m, scope, COUNT_CAP, &|r| view.is_row_visible(r));
        self.count = Some((key.0, key.1, key.2, n));
        Some(n)
    }

    /// Selects the next (or previous) match from the active cell.
    pub fn next(&mut self, grid: &mut GridState, view: &mut DocView, forward: bool) -> bool {
        grid::commit_edit(grid, view);
        let Some(m) = self.matcher() else { return false };
        let scope = self.scope_range(view);
        let from = view.sel.active;
        match search::find_next(&view.doc.table, &m, scope, Some(from), forward, &|r| view.is_row_visible(r)) {
            Some(c) => {
                if self.scope(view) == Scope::Selection {
                    view.sel.active = c;
                    view.sel.anchor = c;
                } else {
                    view.sel = Selection::at(c);
                }
                grid.scroll_to_active = true;
                self.status.clear();
                true
            }
            None => {
                self.status = "Not found".to_string();
                false
            }
        }
    }

    /// Replaces in the active cell if it matches, then moves to the next match.
    pub fn replace_one(&mut self, grid: &mut GridState, view: &mut DocView) {
        grid::commit_edit(grid, view);
        let Some(m) = self.matcher() else { return };
        let replaced = view.doc.replace_in_cell(&m, &self.replacement, view.sel.active);
        self.next(grid, view, true);
        if replaced {
            self.status = "Replaced".to_string();
        }
    }

    pub fn replace_all(&mut self, grid: &mut GridState, view: &mut DocView) {
        grid::commit_edit(grid, view);
        let Some(m) = self.matcher() else { return };
        let scope = self.scope_range(view);
        let visible = view.visibility();
        let n = view.doc.replace_all(&m, &self.replacement, scope, &*visible);
        self.status = if n == 0 { "Nothing to replace".to_string() } else { format!("{n} replaced") };
    }

    pub fn ui(&mut self, ui: &mut Ui, grid: &mut GridState, view: &mut DocView, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.label("Find");
            let query = ui.add(
                egui::TextEdit::singleline(&mut self.opts.query).id(egui::Id::unique("find-query")).desired_width(220.0).hint_text("text to find"),
            );
            if std::mem::take(&mut self.focus_query) {
                query.request_focus();
            }
            let enter = query.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if enter {
                let backwards = ui.input(|i| i.modifiers.shift);
                self.next(grid, view, !backwards);
                query.request_focus();
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) && (query.has_focus() || query.lost_focus()) {
                self.close();
            }
            ui.toggle_value(&mut self.opts.match_case, "Aa").on_hover_text("Match case");
            ui.toggle_value(&mut self.opts.whole_cell, "[ ]").on_hover_text("Whole cell");
            ui.toggle_value(&mut self.opts.regex, ".*").on_hover_text("Regular expression");
            let scope = self.scope(view);
            let scope_label = match scope {
                Scope::All => "Everywhere".to_string(),
                Scope::Selection => "Selection".to_string(),
                Scope::Column => format!("Column {}", view.column_title(view.sel.active.col)),
            };
            egui::ComboBox::from_id_salt("find-scope").selected_text(scope_label).show_ui(ui, |ui| {
                if ui.selectable_label(scope == Scope::All, "Everywhere").clicked() {
                    self.scope = Some(Scope::All);
                }
                if ui.selectable_label(scope == Scope::Selection, "Selection").clicked() {
                    self.scope = Some(Scope::Selection);
                }
                if ui.selectable_label(scope == Scope::Column, "This column").clicked() {
                    self.scope = Some(Scope::Column);
                }
            });
            if ui.button("◀").on_hover_text("Previous (Shift+Enter)").clicked() {
                self.next(grid, view, false);
            }
            if ui.button("▶").on_hover_text("Next (Enter)").clicked() {
                self.next(grid, view, true);
            }
            let count = self.count(view);
            let summary = match count {
                Some(n) if n >= COUNT_CAP => format!("{COUNT_CAP}+ matches"),
                Some(1) => "1 match".to_string(),
                Some(n) => format!("{n} matches"),
                None => String::new(),
            };
            if !self.status.is_empty() {
                ui.colored_label(if self.status == "Not found" || self.status.contains("error") { t.danger } else { t.text_dim }, &self.status);
            } else if !summary.is_empty() {
                ui.colored_label(t.text_dim, summary);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Close (Esc)").clicked() {
                    self.close();
                }
                let label = if self.replace { "Hide replace" } else { "Replace…" };
                if ui.button(label).clicked() {
                    self.replace = !self.replace;
                }
            });
        });
        if self.replace {
            ui.horizontal(|ui| {
                ui.label("Replace");
                let edit = ui.add(egui::TextEdit::singleline(&mut self.replacement).desired_width(220.0).hint_text(if self.opts.regex {
                    "$1 for groups"
                } else {
                    ""
                }));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.replace_one(grid, view);
                    edit.request_focus();
                }
                if ui.button("Replace").clicked() {
                    self.replace_one(grid, view);
                }
                if ui.button("Replace all").clicked() {
                    self.replace_all(grid, view);
                }
            });
        }
    }
}
