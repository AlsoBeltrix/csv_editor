# State

Sole current-state entry point (AGENTS.md). Integration branch: `master` (no remote configured).

## Now

- **CSV editor — plan implemented, accepted by the owner 2026-10-21 ("good enough")**
  (`.agents/plans/csv-editor.md`,
  approved 2026-10-21). Slices 1–8 landed 2026-10-21: Cargo workspace (`crates/table`, `crates/ui`,
  `apps/csv-editor`), eframe shell with the Windows graphics probe, table model with CSV
  sniff/parse/write, inverse-edit undo, document open/save, selection model, and the read-only
  grid (virtualized, own scrollbars, selection with mouse and keyboard, column resize/autofit,
  zoom, header-row toggle, tabs, open via dialog/drag-drop/command line), and editing (in-cell
  editor with Enter/Tab/Esc/arrow semantics, undo/redo, clear, fill down, insert/delete/
  duplicate/move rows and columns via Edit/Rows/Columns menus, context menu and Alt+arrows,
  header rename, Save/Save As with the dialect preserved, save-changes prompts on close and
  quit), and the clipboard (Ctrl+C/X/V and Edit menu: tab-separated text to the system
  clipboard, exact cells within the editor, tiling into a multiple-sized range, growth, cut
  moves as one undo step), find/replace (Ctrl+F/H bar with case, whole-cell and regex options
  and a scope of everywhere/selection/column), sort (Data menu: natural text or numeric, either
  direction, header row kept, one undo step) and row filters (per column: contains, equals,
  blank…; a view the grid scrolls through, hidden rows skipped by arrows and find), cleanup
  tools (Data menu: trim whitespace, change case, remove empty rows/columns, remove duplicate
  rows; results shown in the status bar), the delimiter-and-encoding dialog (File menu and the
  toolbar badge; a changed dialect counts as unsaved), Go to row (Ctrl+G), persisted prefs and
  recent files, system font fallbacks, zone-aware context menus, Help (shortcuts, About).
  Verification entry point (see `.agents/repo-guidance.md`) passes; 100 MB soak numbers are in
  the plan.
  Owner review round 1 (2026-10-21) landed: Excel-like rows (row 1 in the grid, letters-only
  header strip, optional frozen top row), autofit over the selection for columns and rows with
  adjustable row heights, and row/column operations defined on exactly the selected rows and
  columns with descriptive menu labels (see the plan's review section). Round 1b: selection
  happens on mouse down with that press's modifiers (Ctrl released before the button no longer
  drops the added range); inserts land at every selected run. Round 2: per-cell quote
  preservation (`Cell { text, quoted }`), quoting mode "As in the file" as the default.
  The owner re-tested the release build on a real 7,294-row file after round 2 and accepted it.
  Next action: none pending; new findings or the follow-ups listed at the end of the plan
  (memory: parse from bytes; multi-column sort; column drag reorder; marquee; installer) start
  a new slice on the owner's go.

## Facts

- GridCraft checkout: `C:\Users\mcoelho\source\repos\gridcraft` (not `..\gridcraft`), origin
  `storytold/gridcraft`, v0.9.0, MIT OR Apache-2.0. Borrowed code is listed in `ATTRIBUTION.md`.
- Toolchain: rustc/cargo 1.99.1, `stable-x86_64-pc-windows-msvc`. egui/eframe 0.37.
- Build timings on this machine as of the slice-1 commit: cold workspace build ~6 min,
  incremental ~1 min. A second target dir (`target-table/`, gitignored) runs table tests while
  the main one is busy.
- First-run GPU: Intel integrated, DirectX 12 chosen by the probe.
- egui 0.37's default features do not include `default_fonts`; the workspace egui dependency
  enables it, otherwise text panics in tests of the UI crate alone.
- Widgets that must not take keyboard focus (scrollbars) use `Sense::CLICK | Sense::DRAG`, not
  `Sense::click_and_drag()` (which is focusable and catches egui's arrow-key focus moves).
- Looking at the UI without a person: `='<path>.png'` then run the exe
  with a file argument; it saves the window after four frames and exits. Sample data for this
  lives in the session scratchpad, not the repo.

## Blockers

- None.
