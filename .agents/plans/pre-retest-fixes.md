# Plan: repair confirmed workflow defects before the next owner retest

Status: complete 2026-10-30. Approved by the owner ("plan then fix") and landed
directly on `master`; all functional commits are pushed to `origin`. Scope includes
the seven functional findings, accessible control labels, and native clipboard
support. No pending implementation, merge, deletion or owner decisions.

Canonical findings and reproduction evidence:
[workflow audit](../reviews/2026-10-30-workflows.md).
Native clipboard finding and narrow repair:
[system clipboard plan](system-clipboard.md).

## Implementation slices

0. Add the accessible cell label in its own verified slice so all subsequent UI
   regressions retain the harness's default accessibility check.
1. F1: bind pending cell edits to their document. Commit to the original document
   before changing tabs or opening files; cover background-tab close and document
   creation paths as part of the same ownership rule.
2. F2: include pending edits in close handling and cancel every unconfirmed
   native close request, including requests while a prompt is already open.
3. F3: associate cuts with their source document and revision, preventing paste
   from clearing unrelated cells in the destination or after source changes.
   Unchanged same-document cuts remain moves in one undo step. Cross-document
   or stale cuts paste a copy, preserve the source, and explain this in the status
   bar; this avoids destructive cross-document undo coupling. Test overlap,
   cross-document paste, changed source and undo behavior.
4. F4: make replace/cleanup operate on the selected ranges, preserving gaps,
   avoiding duplicate edits where selections overlap, and retaining one undo step.
5. F5: dispatch Save/Save As while a cell is being edited, committing the edit
   through the existing save path while preserving text-edit undo behavior.
6. F6: make quit decisions independent of document dirty-state bookkeeping so
   Don't save handles dialect changes and cancelling quit retains truthful state.
7. F7: clone source cells into the internal clipboard to preserve quote metadata.
8. Apply the native clipboard feature fix from the linked clipboard plan.

Land one finding per verified commit directly on `master`, update records, and
push each commit under the existing always-push policy. No merge or branch
deletion is needed.

## Validation and owner testing

Promote the failing diagnostic scenarios into permanent regression tests before
each corresponding repair. Demonstrate failure without the fix and success with
it, then run the required verification entry point for each landing. Native
clipboard coverage must exercise external text to grid, grid to external
clipboard, and tab-separated multiline data; synthetic paste events alone
cannot verify that integration.

After the repairs, build the release executable and run the native Windows
clipboard/active-cell Save/undo/save-reopen smoke. The permanent Windows UI harness
covers tab changes, selection operations and quit decisions. Give the owner one rebuilt executable and a short
final smoke checklist; automated regressions should cover the defect variants.
Do not represent Windows/headless checks as Linux/macOS runtime verification.

## Progress

- Planning: finalized; baseline evidence is in the linked audit.
- Accessible cell label: implemented. The permanent regression fails with the
  production change removed; all 87 tests, fmt, clippy and native debug smoke pass.
- F1: implemented; five document-transition regressions fail with the fix removed.
  All 92 tests, fmt, clippy, build and native debug smoke pass. Edits commit to
  their owner before switching/opening/creating/closing or saving another tab.
- F2: implemented; both unsafe close paths fail with the fix removed, and the
  clean-document close control passes. All 95 tests and the full entry point,
  including native debug smoke, pass.
- F3: implemented; cross-document and stale-cut regressions fail with the fix
  removed; overlapping moves and one-step undo also pass. Source identity survives
  tab reordering and closing. All 98 tests and the full entry point/debug smoke pass.
- F4: implemented; targeted regressions demonstrate the old selection failures.
  Find/count/replace honor the selected cells and current filters; cleanup merges
  overlapping spans and preserves gaps in one undo step. The Search scope control
  is named for accessibility. All 102 tests and the full entry point/debug smoke pass.
- F5: implemented; Save, Save As and cancelled-picker regressions all fail with
  the fix removed. Active edits commit before save without stealing text-edit
  undo. All 105 tests and the full entry point/debug smoke pass.
- Native clipboard baseline: actual Windows Ctrl+V/Ctrl+S left the disposable
  file unchanged (`old,value`), confirming the omitted bridge beyond injected UI events.
- F6: implemented; all three quit-decision regressions fail with the fix removed.
  Discard decisions live only in the current quit prompt, so dialect changes do
  not loop and cancelling a multi-document quit preserves dirty state. All 108
  tests and the full entry point/debug smoke pass.
- F7: implemented; quote flags and serialized CSV bytes are preserved by internal
  copy/cut, including normalized clipboard line endings. The regression fails
  with the fix removed; the foreign-text control passes. All 110 tests and the
  full entry point/debug smoke pass.
- Native clipboard: implemented; the feature graph includes the OS bridge, all
  110 tests and the full verification entry point pass. The committed native
  Windows keyboard test fails on the old binary and passes on the rebuilt debug
  binary, covering external copy/paste, in-cell Save, cut/move/undo and save/reopen.
- Release build and release native keyboard smoke: passed.
- Commits and pushes: each functional slice landed and was pushed separately.

## Completion receipt

Implementation head: `45a3e71` (native clipboard support). Earlier functional
commits: `34c68f5` (cell label), `c659ba3` (F1), `b2bc123` (F2), `35039a0` (F3),
`c2545ca` (F4), `e907037` (F5), `d76c592` (F6), `b375ed7` (F7).

All 110 tests pass, including 24 new permanent workflow regressions in
`crates/ui/tests/workflows.rs`; the targeted cases were checked with their fixes
removed. Formatting, workspace clippy with warnings denied, debug build and native
OpenGL smoke passed. `cargo build --locked --release -p csv-editor` passed.

Both debug and release executables passed:
`powershell.exe -NoProfile -Sta -File tests/windows_clipboard.ps1`
(with `-Executable target/release/csv-editor.exe` for release). This exercises real
Windows Ctrl+V from external multiline TSV text, Ctrl+C to the system clipboard,
Ctrl+S while editing, cut/paste, one-step Undo, saving, closing and reopening.
The test failed at external paste before the clipboard feature was enabled.

Release artifact: `target/release/csv-editor.exe`, 17,242,112 bytes.
SHA-256: `982B6F717A7EE28F500161E6ED0B53F2379D29504815858BF15F378F50A76FFD`.
Optional owner smoke: paste into a representative file, save, and reopen it.
Cross-document or stale cuts intentionally paste copies with a status notice;
unchanged same-document cuts remain one-step undoable moves. Linux/macOS runtime
testing was not performed in this Windows session.
