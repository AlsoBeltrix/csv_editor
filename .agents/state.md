# State

Sole current-state entry point (AGENTS.md). Integration branch: `master` (no remote configured as of be4efe0).

## Now

- **CSV editor — approach decision pending (2026-10-21).** Owner asked for a streamlined
  spreadsheet-style editor tuned only for `.csv` files (no formulas/analytics), reusing whatever is
  useful from GridCraft. Proposal presented 2026-10-21: lean standalone Rust/egui app with a
  text-only table model, lifting GridCraft's CSV decoder/sniffer, Windows graphics probe, logging,
  theme tokens and selection model (MIT/Apache, attributed). Alternative considered and not
  recommended: wrapping GridCraft's full `SheetApp` with trimmed chrome.
  Next action: on the owner's go, write the durable plan in `.agents/plans/csv-editor.md`, then
  implement slice 1 (workspace bootstrap, eframe hello-grid build on this machine).

## Facts

- GridCraft checkout: `C:\Users\mcoelho\source\repos\gridcraft` (not `..\gridcraft`), origin
  `storytold/gridcraft`, v0.9.0, MIT OR Apache-2.0. Never built on this machine (no `target/`).
- Toolchain: rustc/cargo 1.99.1, `stable-x86_64-pc-windows-msvc`; no `trunk` (web build not needed).
- `.agents/repo-guidance.md` is a stub until the plan is approved; verification entry point TBD there.

## Blockers

- None. Waiting on the owner's approach ruling.
