# Decisions

Settled decisions, newest last. Each names the owner's words, the consequence and what changed.

## 2026-10-22 — No DirectX

Owner: "this shouldn't use directx." The Windows renderer is wgpu on OpenGL; the DirectX 12
probe, the DirectX-first backend choice, the OpenGL/DirectX retry and the FXC shader-compiler pin
are gone from `apps/csv-editor/src/graphics.rs`. Vulkan stays excluded on Windows (Intel driver
crash, see the module). `WGPU_BACKEND` still overrides the choice for troubleshooting.

Consequence: a Windows machine with no OpenGL driver (some virtual machines) gets a start-up
error instead of DirectX 12's software rasteriser. Measured on the owner's machine: OpenGL and
DirectX 12 draw the same picture; private memory with a small file 94 MB vs 201 MB.

## 2026-10-22 — New documents write CRLF on every platform

Asked whether a standard applies, the answer is RFC 4180: CRLF is the record separator, which is
also what Excel writes on Windows and macOS and what Python's csv module defaults to. Opened files
keep their own line ending (sniffed, shown in the badge, written back unchanged) and the dialect
dialog offers CRLF, LF and CR per file. No platform-dependent default.

## 2026-11-05 — A cut pasted into another open document moves the cells

Owner: "cut and ctrl-x doesn't cut, it just copies when cutting from one file to another open
file." This reverses the 2026-10-30 repair choice (F3, `plans/pre-retest-fixes.md`) that made a
cross-document cut paste a copy. The source document is blanked as its own undo step ("Cut")
when the paste lands, provided the source is still open and unchanged since the cut; a closed or
edited source still gets a copy and a status note, so a paste can never erase cells the cut did
not see. Per-document histories stay separate: undoing the paste does not undo the cut in the
other tab. Plan: slice 12 of `plans/csv-editor.md`.

## 2026-11-05 — No Windows installer; a web app is the likely next phase; one icon at every size

Owner: "pass on the installer for now. I think I'm going to make this a web app to host
internally, but that's a later phase. the icon needs to scale from favicon to whatever size is
needed." The MSI, static-runtime and installation-test parts of `plans/windows-distribution.md`
are deferred indefinitely. The icon remains wanted, and its requirement is now explicit: a single
design that reads at favicon size (16 px) and scales to any size the desktop or a browser asks
for (window and taskbar, executable, web manifest). The web app is not planned yet; nothing in
the codebase should assume it, but the icon assets should serve it when it comes.
