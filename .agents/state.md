# State

Sole current-state entry point (AGENTS.md). Integration branch: `master` (no remote configured).

## Now

- **CSV editor — implementing the approved plan** (`.agents/plans/csv-editor.md`, approved
  2026-10-21). Slices 1–3 landed 2026-10-21: Cargo workspace (`crates/table`, `crates/ui`,
  `apps/csv-editor`), eframe shell with the Windows graphics probe, table model with CSV
  sniff/parse/write, inverse-edit undo, document open/save, selection model, and the read-only
  grid (virtualized, own scrollbars, selection with mouse and keyboard, column resize/autofit,
  zoom, header-row toggle, tabs, open via dialog/drag-drop/command line). Verification entry
  point (see `.agents/repo-guidance.md`) passes.
  Next action: slice 4 (in-cell editing, undo/redo wiring, row/column structure edits, Save and
  Save As with the dialect preserved, dirty prompt on close).

## Facts

- GridCraft checkout: `C:\Users\mcoelho\source\repos\gridcraft` (not `..\gridcraft`), origin
  `storytold/gridcraft`, v0.9.0, MIT OR Apache-2.0. Borrowed code is listed in `ATTRIBUTION.md`.
- Toolchain: rustc/cargo 1.99.1, `stable-x86_64-pc-windows-msvc`. egui/eframe 0.37.
- Build timings on this machine as of the slice-1 commit: cold workspace build ~6 min,
  incremental ~1 min. A second target dir (`target-table/`, gitignored) runs table tests while
  the main one is busy.
- First-run GPU: Intel integrated, DirectX 12 chosen by the probe.
- Looking at the UI without a person: `='<path>.png'` then run the exe
  with a file argument; it saves the window after four frames and exits. Sample data for this
  lives in the session scratchpad, not the repo.

## Blockers

- None.
