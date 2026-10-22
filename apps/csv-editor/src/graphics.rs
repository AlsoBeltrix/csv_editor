//! Graphics backend configuration, applied before wgpu can initialize any driver.
//!
//! Adapted from GridCraft's `apps/gridcraft/src/graphics.rs` (MIT, see `ATTRIBUTION.md`), cut
//! down after the owner's decision not to use DirectX (`.agents/decisions.md`, 2026-10-22).
//!
//! Windows draws through OpenGL. DirectX 12 drew the same picture but its runtime and driver
//! cost this process about 100 MB more on an integrated GPU, and a grid of text needs nothing
//! DirectX 12 offers. Vulkan is never initialized unless `WGPU_BACKEND` asks for it: enumerating
//! Vulkan adapters crashes inside Intel's `igvk64.dll` on some UHD Graphics drivers, and that
//! access violation happens in the driver, where no Rust error handling can catch it. Other
//! platforms keep wgpu's own choice (Metal on macOS, Vulkan or OpenGL on Linux).

use std::sync::Arc;

use eframe::{
    NativeOptions,
    egui_wgpu::WgpuSetup,
    wgpu::{Backends, DeviceDescriptor, MemoryHints},
};

/// The backends to initialize: an explicit `backend_override` (normally `Backends::from_env()`,
/// i.e. `WGPU_BACKEND`) always wins; otherwise OpenGL on Windows and wgpu's default elsewhere
/// (`None`: leave eframe's instance descriptor alone).
pub fn choose_backends(backend_override: Option<Backends>) -> Option<Backends> {
    backend_override.or(if cfg!(target_os = "windows") { Some(Backends::GL) } else { None })
}

/// Applies [`choose_backends`] to the wgpu instance eframe will create and asks the device for
/// small memory blocks ([`lean_memory`]). Returns the backends chosen, for the log.
pub fn configure(options: &mut NativeOptions, backend_override: Option<Backends>) -> Option<Backends> {
    let backends = choose_backends(backend_override);
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        if let Some(backends) = backends {
            create.instance_descriptor.backends = backends;
        }
        let base = Arc::clone(&create.device_descriptor);
        create.device_descriptor = Arc::new(move |adapter| lean_memory(base(adapter)));
    }
    backends
}

/// The device descriptor with wgpu's memory-usage hint. wgpu's default (`Performance`) carves
/// GPU memory in 128–256 MB blocks and host-visible memory in 64–128 MB blocks, and on an
/// integrated GPU those heaps are this process's own memory: about 290 MB before a single cell
/// is drawn. `MemoryUsage` starts at 8 MB and 4 MB. A grid of text never fills either.
pub fn lean_memory(desc: DeviceDescriptor<'static>) -> DeviceDescriptor<'static> {
    DeviceDescriptor { memory_hints: MemoryHints::MemoryUsage, ..desc }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_backends(backend_override: Option<Backends>) -> Backends {
        let mut options = NativeOptions::default();
        configure(&mut options, backend_override);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.backends
    }

    #[test]
    fn windows_draws_through_opengl_never_directx() {
        if cfg!(target_os = "windows") {
            assert_eq!(choose_backends(None), Some(Backends::GL));
            assert_eq!(configured_backends(None), Backends::GL);
        } else {
            assert_eq!(choose_backends(None), None);
        }
        assert!(!configured_backends(None).contains(Backends::DX12));
        assert!(!configured_backends(None).contains(Backends::VULKAN));
    }

    #[test]
    fn an_explicit_override_wins() {
        for backends in [Backends::VULKAN, Backends::GL, Backends::DX12, Backends::VULKAN | Backends::DX12] {
            assert_eq!(choose_backends(Some(backends)), Some(backends));
            assert_eq!(configured_backends(Some(backends)), backends);
        }
    }

    #[test]
    fn devices_are_asked_for_small_memory_blocks() {
        let desc = lean_memory(DeviceDescriptor::default());
        assert!(matches!(desc.memory_hints, MemoryHints::MemoryUsage));
        assert!(matches!(DeviceDescriptor::default().memory_hints, MemoryHints::Performance), "wgpu's default moved; re-measure");
    }
}
