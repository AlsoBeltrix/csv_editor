# Plan: Windows icon and installer

Status: installer deferred 2026-11-05 ("pass on the installer for now"; a web app
hosted internally is the likely later phase, see `../decisions.md`). Sections 2 and 3
below (MSI, static runtime, installation checks) are not approved and not started.
The icon (section 1) is still wanted and must scale from favicon size to any size
needed; it is planned separately in `csv-editor.md` when the owner says go.
Drafted 2026-10-30 after the owner accepted the workflow repairs ("good") and
requested an icon and installer.

## Scope and defaults

Deliver a Windows x64 MSI and an application icon. Windows is inferred from the
owner testing `target/release/csv-editor.exe` immediately before this request.
Use the existing product name, CSV Editor, and Cargo version, currently 0.1.0.

- Original green document-and-grid icon, recognizable at small sizes, with a
  folded corner and a highlighted cell. Keep an editable vector source and
  generated PNG/multi-resolution ICO assets in the repository.
- Show the icon in the native window/taskbar, executable, Start menu shortcut,
  and Windows installed-apps entry. Add executable version/product metadata.
- Per-user installation under `%LOCALAPPDATA%\Programs\CSV Editor`, with a setup
  wizard, Start menu shortcut, upgrade handling and normal Windows uninstall.
- Preserve documents and preferences when upgrading or uninstalling. File
  associations, publishing a release, certificates and other OS installers are
  outside this scope. The local deliverable will initially be unsigned.
- Keep the existing Windows OpenGL renderer and application workflows intact.

## Evidence and implementation

1. **Icon and executable resources.** Add the source/assets and an eframe
   viewport icon. Use a Windows resource build step for the ICO and metadata,
   guarded by the target OS so other desktop builds remain supported. Inspect
   the resulting icon at small and large sizes and in the running application.
2. **Standalone Windows build and MSI.** Add a repeatable PowerShell packaging
   command and WiX source. Read version information from Cargo, build a release
   executable for `x86_64-pc-windows-msvc` with the static C runtime, and verify
   that it no longer imports the developer-machine VC runtime DLL. Include the
   existing third-party notices and attribution. Generate the MSI and checksums
   under ignored `target/dist/`, with concise rebuild instructions.
3. **Installation verification and delivery.** Validate the MSI tables and
   perform install, launch, repair/upgrade and uninstall checks using disposable
   fixtures. Check shortcut/icon registration, exact payload and cleanup while
   preserving user data. Reuse `tests/windows_clipboard.ps1` against the
   installed executable to verify clipboard, Save, Undo and reopen behavior.
   Deliver links to the MSI and executable with the verification receipt.

Observed on 2026-10-30:

- Clean local `master` and remote both at
  `06ca0c602581645ac776da49bfac2698c697851e` before this plan.
- WiX 6.0.2 and its matching UI extension are installed. Incompatible global
  7.0.0 extension entries also exist; packaging must select the matching 6.0.2
  extension explicitly rather than changing the global tool installation.
- The existing release executable imports `VCRUNTIME140.dll`, so packaging the
  current binary alone would leave a runtime prerequisite.
- Python/Pillow are available for producing the icon assets. Ordinary builds
  should consume committed assets and not require image-generation tooling.
- There is no existing icon, installer source, resource build script or release
  pipeline in the repository.

Implementation references: [WiX package scope](https://docs.firegiant.com/wix/schema/wxs/packagescopetype/),
[WiX UI](https://docs.firegiant.com/wix/tools/wixext/wixui/),
[Windows resource API](https://docs.rs/winresource/latest/winresource/struct.WindowsResource.html),
[Rust static runtime linkage](https://doc.rust-lang.org/reference/linkage.html).

## Verification and completion

Run `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`, plus a debug UI smoke launch. Build and inspect the
release artifact and run the installation checks above. Any new behavioral
regression test must fail without its corresponding fix. Record checks that
cannot be performed, including the absence of a clean external Windows test PC.

Approval covers the implementation, disposable local installation tests,
verification and normal direct-to-master commit/push closeout. No branch merge
or deletion is needed. Completion requires the local MSI, repeatable packaging,
verification receipt and a clean pushed tree. Pending decision: approve this
Windows packaging scope and defaults.
