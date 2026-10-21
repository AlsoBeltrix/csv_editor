# Attribution

Code adapted from other projects. Each row names the file here, its source, and the licence it
came under. The licence texts are reproduced in `THIRD_PARTY_NOTICES.md`.

| File here | Source | Licence | Notes |
|---|---|---|---|
| `apps/csv-editor/src/graphics.rs` | GridCraft `apps/gridcraft/src/graphics.rs` (storytold/gridcraft v0.9.0) | MIT OR Apache-2.0 | Windows DirectX 12 / OpenGL backend selection and FXC shader compiler pin; renamed, otherwise as-is. |
| `crates/ui/src/theme.rs` | GridCraft `crates/ui-egui/src/theme.rs` (storytold/gridcraft v0.9.0) | MIT OR Apache-2.0 | Colour tokens and egui visuals setup; font loading not taken. |
| `crates/table/src/text.rs` | GridCraft `crates/xlsx/src/csv.rs` (storytold/gridcraft v0.9.0) | MIT OR Apache-2.0 | Byte decoding (BOMs, UTF-16, Windows-1252 fallback) and the delimiter sniffer. |
| `crates/table/src/selection.rs` | GridCraft `crates/engine/src/selection.rs` (storytold/gridcraft v0.9.0) | MIT OR Apache-2.0 | Selection model, Ctrl+arrow jump and current-region search, adapted to finite bounds. |
| `crates/ui/src/grid.rs` (scrollbars) | GridCraft `crates/ui-egui/src/grid.rs` (storytold/gridcraft v0.9.0) | MIT OR Apache-2.0 | The hand-drawn scrollbar technique; the grid itself is original. |

No images, icons, fonts or other assets are bundled.
