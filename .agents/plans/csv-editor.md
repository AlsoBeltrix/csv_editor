# Plan: CSV editor (lean standalone app)

Status: **approved 2026-10-21** (owner chose "lean standalone app" over wrapping GridCraft's UI).
Implementation: **all eight slices landed on `master` 2026-10-21**; each passed the verification
entry point before landing. Remaining: the owner's hands-on review of the running app, and the
follow-ups listed at the end.

Progress: slices 1–8 landed 2026-10-21. Verified on this machine: build, tests, clippy, fmt, and a
headless screenshot (`CSV_EDITOR_SCREENSHOT=<png>` saves the window after a few frames and quits)
showing a 400-row sample with header names, leading zeros kept and numbers right-aligned.
Slice 4 added headless interface tests (`crates/ui/tests/editing.rs`, egui_kittest) that type,
press keys and check the document. Slice 5 added copy/cut/paste (tab-separated text, exact cells
within the editor, tiling, growth, cut-moves). Slice 6 added the find/replace bar (Ctrl+F/H;
case, whole cell, regex; scope everywhere/selection/column), sorting by column (natural text or
numeric, header-aware, one undo step) and per-column row filters as a view the grid scrolls
through. Slice 7 added the cleanup tools (trim, change case, remove empty rows/columns, remove
duplicate rows; selection-scoped where that makes sense), the delimiter-and-encoding dialog (a
changed dialect dirties the document) and Go to row (Ctrl+G). Slice 8 added persisted
preferences (theme, zoom, recent files, window size in `%APPDATA%\csv-editor\prefs.json`), an
Open Recent menu, system font fallbacks (Segoe UI plus symbol and CJK faces, validated before
egui sees them), zone-aware right-click menus (cells, row gutter, column header), a Help menu
with the shortcut list and About, and an open-time log line.

Large-file soak (this machine, Intel integrated GPU): a 100.3 MB, 1,357,661-row × 8-column file
opened in 0.70 s (release) / 1.15 s (dev profile) with a peak working set of 702 MB / 831 MB. The
release executable is 16.3 MB. After the per-cell quote flag (cells 24 → 32 bytes): 0.58 s and
765 MB in release. After slice 9 (measured with a poller of the peak working set, which reads
higher than the earlier method): 0.45 s open, 0.22 s save, 524 MB peak; see slice 9.

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
- Quote fidelity: every cell carries a `quoted` flag set by the parser. With the dialect's
  quoting at `AsSource` (the default for opened files) an untouched cell is written back as it
  came; new or edited text is quoted only when it needs to be. `Minimal` strips optional quotes,
  `All` quotes everything. The flag rides inside the cell, so moves, sorts, undo and copies keep
  it; equality ignores it.
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

## Owner review round 1 (2026-10-21)

Findings from the owner's first hands-on session and how they were resolved:

1. Double-clicking a column or row edge now fits every selected column or row (the edge's
   own line when it is not selected). Rows gained adjustable heights: drag a gutter edge, or fit
   to contents, and cells in tall rows show their lines.
2. Row 1 is in the grid like any row (the header strip shows letters only, as in Excel), so a
   column is renamed by typing into row 1. "First row is header" only tells sort, filters and
   cleanup to leave row 1 alone and lets menus name columns by it; "Freeze top row" (View) pins
   it. The rename dialog is gone.
3. Row and column operations act on exactly the rows (columns) that contain a selected cell,
   never on the gap between disconnected selections. Delete and duplicate name their targets
   ("Delete rows 2, 5–6"); moves need a contiguous block; row operations are disabled when
   whole columns are selected and vice versa. Insert adds blank rows (columns) at *each* run of
   selected rows (columns), as many as the run has, as Excel does. A headless test drives a real
   Ctrl+click through the grid and checks delete, delete columns and insert on the result.
   Root cause of the owner's report: selection was decided at mouse *release* with the modifiers
   held then; a hand that lets go of Ctrl first made the click plain and replaced the selection.
   Selection now happens on mouse down with that event's modifiers, as in a spreadsheet; the
   test releases Ctrl before the button and fails against the old code.

## Owner review round 2 (2026-10-21)

Quotes around fields that did not need them were dropped on save (the writer knew only an
aggregate policy). Fixed by the per-cell `quoted` flag described under Table model: mixed-quoting
files now round-trip byte for byte, edits follow the minimal rule, and the dialect dialog offers
"As in the file", "Only when needed" and "Every field".

## Slice 9: performance (owner go 2026-10-21, "keep optimizing")

Baseline on the 100.3 MB soak file (`crates/table/examples/soak.rs` with an outside poller of the
peak working set; release): open 0.52 s, save 0.40 s, peak 588 MB for open-then-save in the
example and 837 MB in the app. Where the memory goes: cells 348 MB (10.9 M × 32 bytes) + row
vectors 33 MB are the table itself; the rest of the peak is the file bytes plus a decoded copy on
open, and the text plus an encoded copy on save (about 2 × the file size each).

Changes:
- `decode` returns `Cow<str>`: a UTF-8 file (the common case) is parsed straight from its bytes.
- The record splitter scans with slice searches and sizes each row's vector from the previous
  row (one exact allocation per row).
- Saving streams row by row through a `BufWriter` to the temporary file (`io::write_to`); the
  in-memory `write` is kept for tests and the clipboard. Field quoting checks bytes first.
- The soak example doubles as a round-trip check (saved bytes equal the file).
- The wgpu device is created with `MemoryHints::MemoryUsage` (`graphics::lean_memory`). Measured
  by backend: with a 20 KB file the app's private memory was 377 MB on DirectX 12 and 45 MB on
  OpenGL. The difference is wgpu's default memory hint (`Performance`): its allocator carves GPU
  memory in 128–256 MB blocks and host-visible memory in 64–128 MB blocks, and on an integrated
  GPU those heaps are the process's own memory. `MemoryUsage` starts at 8 MB and 4 MB.

Results (same file and method): open 0.42 s, save 0.20 s; soak example peak 488 MB (the table,
381 MB, plus the file bytes while parsing); app peak 607 MB, and 200 MB with the 20 KB file
(was 443 MB). What remains in the app's baseline: 48 MB of system font files (four CJK and
symbol faces egui keeps in memory) and about 150 MB of DirectX 12 driver and runtime.

Then `Cell` went from 32 to 24 bytes: the quote flag is the first byte of the cell's own
`CompactString` (`"` or a space) and the text is the rest, so the flag no longer pads the struct
and `Row = Vec<Cell>` is untouched (the flag was read at four places outside the type). On this
file: table 381 → 298 MB; soak example peak 488 → 405 MB; app peak 607 → 524 MB. Building each
cell as prefix plus text costs about 30 ms on the open (0.42 → 0.45 s, within run-to-run noise).
A size test pins the 24 bytes.

## Follow-ups (not started)

- Parse in parallel (split at record boundaries outside quotes) for files past a few hundred MB.
- Fonts: the CJK fallback faces cost 48 MB whether or not the data needs them; loading them on
  the first missing glyph would need a font-fallback hook egui does not have yet.
- Multi-column sort, column drag-reordering with the mouse, a dashed
  marquee on the cut/copy source, a Windows installer and file association.

## Out of scope (v1)

Formulas, number formats, cell styling, charts, XLSX, multi-sheet, per-cell quote preservation,
web/wasm build, installer. Any of these is a new plan.
