# Workflow audit — 2026-10-30

Status: investigation complete. The owner subsequently approved repairs on
2026-10-30 ("plan then fix"). This report preserves the original failing evidence;
current repair progress is in [pre-retest-fixes.md](../plans/pre-retest-fixes.md).

Audited head: `307ad00eb7e0a3aa059389066c1cf4af9ad02523`, identical to remote
`master` at startup. No application source or dependency configuration changed.
The previously diagnosed native clipboard feature omission is recorded separately
in [system-clipboard.md](../plans/system-clipboard.md).

## Confirmed functional findings

### F1 — P1: an unfinished edit can overwrite a different document

Locations: `crates/ui/src/lib.rs:112`, `:503`; `crates/ui/src/grid.rs:479`.

Reproduction: open two files with A1 values `original` and `second`. In the first
tab, type `unsaved` without Enter, then click the second tab. The first file still
contains `original`, while the second file's A1 becomes `unsaved`. This was
reproduced with mouse press and release on separate UI frames. Opening a dropped
file during an edit has the same result: its existing A1 is overwritten when the
edit commits.

Cause: `CellEditor` belongs to the shared grid, but tab/open paths change the
active document before committing that editor to its original document.

Failing probes: `switching_tabs_keeps_pending_edit_in_original_document`,
`opening_a_dropped_file_preserves_the_original_pending_edit`. Actual document
pair in both: `("original", "unsaved")`, instead of editing only the first file.

### F2 — P1: closing the window can bypass unsaved-change protection

Location: `crates/ui/src/lib.rs:285`.

Two independent reproductions:

- Start from a clean document, type into a cell without Enter, then request window
  close. No `CancelClose` command is emitted: dirty-state checking overlooks the
  pending cell editor.
- Edit and commit a cell, request window close, then request close again while
  the save prompt is open. The first request emits `CancelClose`; the second
  does not, because the guard requires `self.prompt.is_none()`.

Failing probes: `window_close_protects_an_uncommitted_cell_edit`,
`second_window_close_cannot_bypass_save_prompt`.

### F3 — P1: a cross-document cut clears unrelated destination cells

Locations: `crates/ui/src/ops.rs:41`, `:53`, `:59`;
`crates/table/src/document.rs:309`.

Reproduction: cut A1=`source` in file A. File B has A1=`precious` and B1=`target`.
Paste into B1 in file B. B1 becomes `source`, but A1 in file B is emptied; file A
still holds the source. A cut remembers only coordinates, so paste clears those
coordinates in the destination document instead of identifying the source.

Failing probe: `cross_document_cut_does_not_erase_destination_source_coordinates`.
The copied target is correct; the unrelated destination A1 is `""`.

### F4 — P1: selection-scoped replacement and cleanup modify unselected gaps

Locations: `crates/ui/src/find.rs:58`, `:130`;
`crates/ui/src/ops.rs:375`, `:383`.

Reproduction: put `x` in A1, B1 and C1, then select A1 and Ctrl-select C1.
Replace All with scope Selection changes B1 too. Selecting the same disconnected
cells containing ` x ` and running Trim also trims B1. Both operations use the
bounding rectangle rather than the selected ranges. Change Case uses the same
cleanup scope helper; it was inspected, not separately executed.

Failing probes: `replace_all_respects_disconnected_selection`,
`trim_respects_disconnected_selection`.

### F5 — P2: Ctrl+S does nothing during a cell edit

Location: `crates/ui/src/lib.rs:282`.

Reproduction: open a saved file, type `unsaved` into A1 without Enter, then press
Ctrl+S. The file on disk still contains `original`. All global shortcuts are
skipped while `grid.editor` exists, including Save, even though `save()` itself
already knows how to commit a pending edit.

Failing probe: `ctrl_s_saves_the_active_cell_edit`, which checks the temporary
file's actual bytes after the keypress.

### F6 — P2: Don't save cannot finish quitting after a dialect change

Location: `crates/ui/src/lib.rs:632`.

Reproduction: change the delimiter on a document, request window close, and click
Don't save. No final close command is sent and the same save prompt remains.
Discard only marks edit history saved, while `Document::is_dirty()` still sees
the changed delimiter/encoding/line-ending settings.

Failing probe: `dont_save_can_discard_a_changed_dialect_on_quit`.

### F7 — P2: copy/paste drops the original CSV quote flags

Location: `crates/ui/src/ops.rs:22`.

Reproduction: open `"001",plain`, copy A1 inside the editor, and paste into B1.
The destination cell's quote flag is false. With the default As in the file
quoting mode, the pasted value saves unquoted instead of preserving the source
cell's quoting. `current_cells()` constructs fresh cells from strings rather
than cloning cells, losing metadata before the internal clipboard is populated.
Cut uses the same copy helper.

Failing probe: `copy_paste_preserves_source_quote_flags`.

## Additional accessibility finding

The in-cell multiline text editor has no accessible name. The default
egui_kittest accessibility check fails while it is open. Existing tests normally
finish editing before running this check, which is why the baseline passes.

The first scratch run exposed this failure before three behavioral assertions.
The scratch harness then disabled only the accessible-name check after source
inspection established that it runs after settling and does not affect input or
application state. All ten behavioral probes subsequently reached and failed
their intended assertions. No repository check or application guard was changed.
Add an accessible cell label when fixing the editor lifecycle and retain the
normal accessibility check in committed regression tests.

## Verification and limits

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: all 86 existing tests passed (4 app, 59 table,
  15 UI unit, 8 UI integration).
- `cargo build --offline -p csv-editor`: passed.
- Native Windows debug smoke: opened a disposable 3-row CSV using OpenGL,
  saved a screenshot, exited 0. Screenshot inspected; grid contents and
  leading-zero values rendered correctly. Preferences were disabled by the
  existing screenshot mode.
- Ten targeted diagnostic probes failed on the audited source, covering the
  seven functional groups above. The two document-transition probes were rerun
  with assertions covering both documents, confirming the destination overwrite.
- Scratch harness: `target/clipboard-audit/src/lib.rs`, runnable locally with
  `cargo test --offline --manifest-path target/clipboard-audit/Cargo.toml --target-dir target -- --nocapture`.
  These are ignored local diagnostic artifacts, not committed regression tests;
  the reproduction steps and results above are the durable evidence.
- Native OS clipboard interaction and Linux/macOS execution were not tested.
  Behavioral UI probes use egui's real frame/input processing headlessly; the
  dropped-file probe calls the method invoked by the native drop handler.

No fixes were applied. Promote these cases to permanent regressions during the
approved repair work, then run real clipboard integration and one final release
smoke before the owner's retest.
