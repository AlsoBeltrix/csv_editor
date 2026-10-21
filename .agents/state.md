# State

Sole current-state entry point (AGENTS.md). Integration branch: `master` (no remote configured).

## Now

- **CSV editor — implementing the approved plan** (`.agents/plans/csv-editor.md`, approved
  2026-10-21). Slices 1–4 landed 2026-10-21: Cargo workspace (`crates/table`, `crates/ui`,
  `apps/csv-editor`), eframe shell with the Windows graphics probe, table model with CSV
  sniff/parse/write, inverse-edit undo, document open/save, selection model, and the read-only
  grid (virtualized, own scrollbars, selection with mouse and keyboard, column resize/autofit,
  zoom, header-row toggle, tabs, open via dialog/drag-drop/command line), and editing (in-cell
  editor with Enter/Tab/Esc/arrow semantics, undo/redo, clear, fill down, insert/delete/
  duplicate/move rows and columns via Edit/Rows/Columns menus, context menu and Alt+arrows,
  header rename, Save/Save As with the dialect preserved, save-changes prompts on close and
  quit). Verification entry point (see `.agents/repo-guidance.md`) passes.
  Next action: slice 5 (clipboard: copy/cut/paste as tab-separated text, paste growing the
  table, tiling into a range).

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
