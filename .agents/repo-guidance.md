# Repo Guidance

Repo-specific rules; AGENTS.md includes this file.

- Integration branch: `master`. No remote yet.
- Stack: Rust 2024, egui/eframe desktop app, Cargo workspace at the repo root (layers:
  `crates/table` → `crates/ui` → `apps/csv-editor`; nothing below `crates/ui` depends on egui). Code borrowed from GridCraft (`C:\Users\mcoelho\source\repos\gridcraft`, MIT OR Apache-2.0)
  is copied file-by-file with attribution in `ATTRIBUTION.md`; no path or git dependency on the
  GridCraft workspace.
- Verification entry point, run before claiming completion:
  `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo test --workspace`. UI changes also get a smoke launch of `target\debug\csv-editor.exe`.
- Never-crash rule: no `unwrap`/`expect`/`panic!`/indexing on input-derived numbers outside tests
  (workspace clippy lints deny them). Implementation lands directly on `master` in verified slices.
- Plans live in `.agents/plans/<name>.md`; decisions in `.agents/decisions.md`.
