# Plan: restore system clipboard shortcuts

Status: complete and verified 2026-10-30 as part of the consolidated repairs.
Release receipt: [pre-retest-fixes.md](pre-retest-fixes.md).

## Finding

The owner reports that Ctrl+V does not paste data. At `307ad00`, the workspace
disables eframe's default features, and `apps/csv-editor/Cargo.toml` does not
enable `clipboard`. The resolved feature graph confirms that neither
`eframe/clipboard` nor `egui-winit/clipboard` is enabled.

The installed egui-winit 0.37.0 implementation uses an internal string when its
clipboard backend is disabled. A real Ctrl+V reads that string and emits no
paste event when it is empty. Copy also writes to that internal clipboard.
The app's Edit > Paste uses its own arboard reader. Existing UI tests inject
`Event::Paste` directly, so they do not exercise the native clipboard bridge.

Local `master` and the remote `master` both resolve to
`307ad00eb7e0a3aa059389066c1cf4af9ad02523` at diagnosis. The baseline verification
suite and debug smoke subsequently passed; native clipboard reproduction remains
pending. See the [workflow audit](../reviews/2026-10-30-workflows.md) and
[consolidated repair plan](pre-retest-fixes.md) for the owner's follow-up request.

## Proposed implementation

1. Add `"clipboard"` to the eframe features in `apps/csv-editor/Cargo.toml`,
   with a brief comment explaining why it is required when defaults are off.
   Allow Cargo to update the lockfile for this feature's dependencies.
2. Verify the resolved feature graph includes the native clipboard backend.
   Exercise real system clipboard paste and copy in the Windows app, including
   a tab-separated multirow block. Compare behavior with the original build.
3. Run the repo verification entry point: fmt, workspace clippy with warnings
   denied, workspace tests, and a debug app smoke launch. Build the release
   executable for the owner's retest. Record results and any limitations.
4. Land one verified fix commit with these records directly on `master`, then
   push according to `.agents/push-policy.md` (always).

No new synthetic paste test is proposed: the existing test already covers the
grid operation, while this defect is in the native integration feature set.

## Verification receipt

The resolved feature graph now enables `eframe/clipboard`, `egui-winit/clipboard`
and its native backends. All 110 workspace tests, fmt, clippy and native debug
launch passed. `tests/windows_clipboard.ps1` failed on the old executable at the
external Ctrl+V assertion and passes on both rebuilt debug and release executables: external
multiline paste, external copy, Save during an active edit, cut/paste, one-step
undo and reopening the saved CSV (including leading zeros). Clipboard contents
and preferences are preserved by the test. The added lockfile entries support
the clipboard feature on other platforms; Linux/macOS runtime checks remain unrun.
