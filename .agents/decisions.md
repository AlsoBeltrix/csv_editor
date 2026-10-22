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
