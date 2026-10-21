# Plan: CSV editor (lean standalone app)

Status: **approved 2026-10-21** (owner chose "lean standalone app" over wrapping GridCraft's UI).
Implementation: in progress on `master`, slice by slice. Each slice is one or more commits that
pass the verification entry point before landing.

Progress: slices 1–5 landed 2026-10-21. Verified on this machine: build, tests, clippy, fmt, and a
headless screenshot (`CSV_EDITOR_SCREENSHOT=<png>` saves the window after a few frames and quits)
showing a 400-row sample with header names, leading zeros kept and numbers right-aligned.
Slice 4 added headless interface tests (`crates/ui/tests/editing.rs`, egui_kittest) that type,
press keys and check the document. Slice 5 added copy/cut/paste (tab-separated text, exact cells
within the editor, tiling, growth, cut-moves). Next: slice 6 (find, replace, sort, filter).

## Goal

A streamlined, spreadsheet-style desktop editor tuned only for delimited text files (`.csv`,
`.tsv`, `.txt`). No formulas, no analytics. Comprehensive enough that the owner never opens Excel
to make a change to a CSV. Byte-faithful: every cell is text; untouched cells round-trip
unchanged; delimiter, quote character, line endings and encoding are detected on open and
preserved on save.

## Constraints

- Rust 2024, egui/eframe desktop app, Cargo workspace at the repo root. Windows is the primary
  target; keep the code portable (no Windows-only APIs outside `cfg(windows)` blocks).
- No panics in non-test code (`unwrap`, `expect`, `panic!`, indexing on input-derived numbers).
  Crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, ...)]`.
  Input-derived numbers are hostile: `get()`, checked or saturating arithmetic, capped allocations.
- Code borrowed from GridCraft (`C:\Users\mcoelho\source\repos\gridcraft`, MIT OR Apache-2.0) is
  copied file-by-file, adapted, and listed in `ATTRIBUTION.md` with the MIT notice reproduced in
  `THIRD_PARTY_NOTICES.md`. No path or git dependency on the GridCraft workspace. No GridCraft
  assets, icons or branding.
- Undo is command-based (inverse edits), never whole-table snapshots: a 100 MB file must stay
  editable.
- Memory target: a 100 MB CSV opens in a few seconds and scrolls at 60 fps. Cells use a compact
  string type (short fields stored inline, no heap allocation).

## Architecture

```
csv_editor/            Cargo workspace
  crates/table/        L0  text table model, edits + undo, CSV sniff/decode/parse/write,
                           sort/filter/find helpers. No egui. Fully unit-tested.
  crates/ui/           L1  egui UI: grid (virtualized, own scrollbars), toolbar, menus,
                           dialogs, theme tokens, keymap, status bar. Depends on table.
  apps/csv-editor/     L2  eframe shell: window, Windows graphics probe, file pickers (rfd),
                           drag-drop, prefs/recent files, logging.
```

Ported from GridCraft (adapted): `csv.rs` decoder + delimiter sniffer → `table::csv`;
`graphics.rs` → `apps/csv-editor/src/graphics.rs`; `theme.rs` tokens + `apply` → `ui::theme`;
`selection.rs` → `table::selection`; the manual scrollbar technique from `grid.rs` → `ui::grid`.

### Table model

- `Table { rows: Vec<Row>, width: usize }`, `Row = Vec<CompactString>`. Ragged rows are
  preserved as read (a row may be shorter than `width`); the grid pads visually. Save writes rows
  as they are, so untouched ragged rows round-trip.
- `Dialect { delimiter: u8, quote: u8, line_ending: Crlf|Lf|Cr, encoding: Utf8|Utf8Bom|Utf16Le|
  Utf16Be|Cp1252, quoting: Minimal|All|AsSource, trailing_newline: bool, header: bool }`,
  detected on open, editable in a dialog, applied on save.
- Quote fidelity: the reader records whether each source field was quoted only as an aggregate
  policy (all / minimal / mixed → minimal). Per-cell quote flags are out of scope.
- `Edit` enum with `apply` and `invert`: `SetCell`, `SetCells` (rect), `InsertRows`, `DeleteRows`
  (stores removed rows), `InsertCols`, `DeleteCols`, `MoveRows`, `MoveCols`, `ReorderRows`
  (sort: stores the permutation), `Replace` (batch of SetCell). Undo/redo stacks of `Edit`.
  Consecutive typing into one cell coalesces.
- Row filter is a view: `visible: Option<Vec<u32>>` over row indices; edits address real rows.

### UI

- Toolbar (not a ribbon): File, Edit, Rows/Cols, Data, View menus plus a few direct buttons,
  the dialect badge and the search box. Status bar: rows × cols, selection, dirty, dialect.
- Grid: row-number gutter, column headers (letters, or header-row text when "first row is
  header"), frozen header row, column resize by drag and autofit by double-click, zoom.
  Virtualized: only visible rows and columns are laid out and painted; scroll state is
  `(first_row, pixel_offset)` so a million rows never exceed f32 precision.
- Editing: type-to-replace, F2/Enter/double-click to edit in place, Esc cancels, Enter/Tab move
  (Shift reverses), Delete clears, Ctrl+D fill down, Ctrl+Z/Y, Ctrl+X/C/V (tab-separated
  text, Excel-compatible; paste grows the table as needed), Ctrl+A, Ctrl+arrows jump,
  Shift+arrows extend, Ctrl+F find, Ctrl+H replace, Ctrl+G go to row.
- Multi-file tabs; drag-drop opens; recent files; "save changes?" on close.
- Light/dark theme following the system, overridable.

## Slices (each: implement → tests → verification → commit)

1. **Bootstrap.** Workspace, three crates, lints, `rustfmt.toml` (GridCraft's settings),
   `ATTRIBUTION.md`, `THIRD_PARTY_NOTICES.md`, `.gitignore`. eframe window that opens and shows
   an empty grid. Windows graphics probe ported. Proves the toolchain builds wgpu/eframe here.
2. **Table + CSV I/O.** `table` crate: model, dialect detection (BOM, UTF-16, CP1252 fallback,
   delimiter sniff, line ending, quoting policy, trailing newline), parser (streaming, RFC 4180
   with leniency), writer. Round-trip tests on synthetic files, including ragged rows, embedded
   newlines, doubled quotes, `;` and tab delimiters, all encodings.
3. **Grid read-only.** Virtualized grid with gutter, headers, own scrollbars, selection (cell,
   range, row, column, all; Shift/Ctrl modifiers; Ctrl+arrows), column resize/autofit, frozen
   header, zoom, header-row toggle. Open via menu, drag-drop, command line.
4. **Editing + undo.** In-cell editor, Edit enum with invert, undo/redo, Delete, fill down,
   insert/delete/duplicate/move rows and columns, rename header. Save/Save As with dialect
   preserved; dirty tracking; "save changes?" on close.
5. **Clipboard.** Copy/cut/paste as tab-separated text (quoted when needed), paste expanding the
   table, paste into a range by tiling, copy whole rows/columns.
6. **Find, replace, sort, filter.** Find bar with match navigation, replace one/all, options
   (match case, whole cell, regex, scope: selection/column/all). Sort by column (text, numeric,
   natural; ascending/descending; header-aware; stable; undoable). Per-column quick filter
   (contains text, blanks/non-blanks) as a non-destructive view; clear filters.
7. **Cleanup tools + dialect dialog.** Trim whitespace, remove empty rows/columns, dedupe rows,
   change case, go to row, dialect dialog (delimiter, quote, line ending, encoding, quoting,
   header). Status bar counts.
8. **Polish.** Multi-file tabs, recent files, prefs persisted (theme, zoom, window size),
   keyboard shortcut audit, large-file soak (synthetic 100 MB), context menus, about dialog.

## Verification entry point

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

UI behaviour that can be tested headlessly uses `egui_kittest` (as GridCraft does); everything in
`crates/table` is plain unit tests. New tests must be shown to bite (revert, fail, restore).

## Pending owner decisions

- None. Direct-to-`master` slices and the verification entry point above were stated as
  assumptions in the approved proposal.

## Out of scope (v1)

Formulas, number formats, cell styling, charts, XLSX, multi-sheet, per-cell quote preservation,
web/wasm build, installer. Any of these is a new plan.
