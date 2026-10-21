# Repo Guidance

Repo-specific rules; AGENTS.md includes this file. Stub pending plan approval (2026-10-21).

- Integration branch: `master`. No remote yet.
- Stack (proposed, not yet approved): Rust 2024, egui/eframe desktop app, Cargo workspace at repo
  root. Code borrowed from GridCraft (`C:\Users\mcoelho\source\repos\gridcraft`, MIT OR Apache-2.0)
  is copied file-by-file with attribution in `ATTRIBUTION.md`; no path or git dependency on the
  GridCraft workspace.
- Verification entry point: to be defined with the plan (expected: `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo test`).
- Plans live in `.agents/plans/<name>.md`; decisions in `.agents/decisions.md`.
