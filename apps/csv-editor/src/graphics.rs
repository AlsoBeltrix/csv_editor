//! Windows graphics instance configuration, applied before wgpu can initialize any driver.
//!
//! Adapted from GridCraft's `apps/gridcraft/src/graphics.rs` (MIT, see `ATTRIBUTION.md`).
//!
//! Vulkan is never initialized unless `WGPU_BACKEND` asks for it: enumerating Vulkan adapters
//! crashes inside Intel's `igvk64.dll` on some UHD Graphics drivers, and that access violation
//! happens in the driver, where no Rust error handling can catch it.
//!
//! DirectX 12 shaders are compiled with FXC (`d3dcompiler_47.dll`, part of Windows), never with
//! whichever `dxcompiler.dll` the DLL search path happens to find: see [`dx12_compiler`].

use std::sync::Arc;

use eframe::{
    NativeOptions,
    egui_wgpu::WgpuSetup,
    wgpu::{Backends, DeviceDescriptor, DeviceType, Dx12Compiler, MemoryHints},
};

/// What the start-up probe learned about one DirectX 12 adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dx12Adapter {
    /// `Cpu` is WARP (Microsoft's software rasteriser): it works, but every frame is drawn on the
    /// processor, which makes the app lag on older machines.
    pub device_type: DeviceType,
    /// Whether the adapter offers the limits eframe will ask for when it creates the device.
    pub meets_limits: bool,
}

/// The backends to initialize on Windows.
///
/// An explicit `backend_override` (normally `Backends::from_env()`, i.e. `WGPU_BACKEND`) always
/// wins. Otherwise DirectX 12 only, as long as `dx12_adapters` (what a DirectX 12-only probe found)
/// contains a usable hardware GPU. Without one, OpenGL is the way to the GPU. When DirectX 12
/// offers only WARP, it stays enabled next to GL as a last resort for machines without an OpenGL
/// driver either (virtual machines). Vulkan is never part of the default.
pub fn choose_backends(backend_override: Option<Backends>, dx12_adapters: &[Dx12Adapter]) -> Backends {
    if let Some(backends) = backend_override {
        return backends;
    }
    let hardware = |a: &&Dx12Adapter| a.device_type != DeviceType::Cpu;
    if dx12_adapters.iter().filter(hardware).any(|a| a.meets_limits) {
        Backends::DX12
    } else if !dx12_adapters.is_empty() && !dx12_adapters.iter().any(|a| hardware(&a)) {
        Backends::DX12 | Backends::GL
    } else {
        Backends::GL
    }
}

/// The backends for a second start when the first one failed with a graphics error before the
/// app was created: the other one of DirectX 12 and OpenGL. `None` when `WGPU_BACKEND` chose the
/// backends or the first start already tried both. Never Vulkan.
pub fn fallback_backends(backend_override: Option<Backends>, first: Backends) -> Option<Backends> {
    if backend_override.is_some() {
        return None;
    }
    let rest = (Backends::DX12 | Backends::GL).difference(first);
    (!rest.is_empty()).then_some(rest)
}

/// The DirectX 12 shader compiler: `env_choice` (`WGPU_DX12_COMPILER`, for troubleshooting) or
/// else FXC, which is part of Windows and needs no extra DLL.
pub fn dx12_compiler(env_choice: Option<Dx12Compiler>) -> Dx12Compiler {
    env_choice.unwrap_or(Dx12Compiler::Fxc)
}

/// Restricts the wgpu instance eframe creates to `backends`, pins its DirectX 12 shader
/// compiler ([`dx12_compiler`]) and asks the device for small memory blocks ([`lean_memory`]).
pub fn configure_backends(options: &mut NativeOptions, backends: Backends) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backend_options.dx12.shader_compiler = dx12_compiler(Dx12Compiler::from_env());
        create.instance_descriptor.backends = backends;
        let base = Arc::clone(&create.device_descriptor);
        create.device_descriptor = Arc::new(move |adapter| lean_memory(base(adapter)));
    }
}

/// The device descriptor with wgpu's memory-usage hint. wgpu's default (`Performance`) carves
/// GPU memory in 128–256 MB blocks and host-visible memory in 64–128 MB blocks, and on an
/// integrated GPU those heaps are this process's own memory: about 290 MB before a single cell
/// is drawn. `MemoryUsage` starts at 8 MB and 4 MB. A grid of text never fills either.
pub fn lean_memory(desc: DeviceDescriptor<'static>) -> DeviceDescriptor<'static> {
    DeviceDescriptor { memory_hints: MemoryHints::MemoryUsage, ..desc }
}

/// Chooses the backends ([`choose_backends`]), applies them to `options` and returns them.
/// Without a `backend_override` this first probes DirectX 12 with a short-lived instance of its
/// own, which is dropped before eframe creates the real one.
#[cfg(target_os = "windows")]
pub fn configure(options: &mut NativeOptions, backend_override: Option<Backends>) -> Backends {
    let backends = match backend_override {
        Some(backends) => {
            log::info!("graphics: WGPU_BACKEND override, backends {backends:?}");
            backends
        }
        None => {
            let adapters = probe_dx12(options);
            let backends = choose_backends(None, &adapters);
            log::info!("graphics: {} DirectX 12 adapter(s) {adapters:?}, backends {backends:?}", adapters.len());
            backends
        }
    };
    configure_backends(options, backends);
    backends
}

/// Lists the DirectX 12 adapters, checking each against the limits eframe's device descriptor
/// will require. Empty when this build has no DirectX 12 backend.
#[cfg(target_os = "windows")]
fn probe_dx12(options: &NativeOptions) -> Vec<Dx12Adapter> {
    use eframe::wgpu::{Instance, InstanceDescriptor};
    if !Instance::enabled_backend_features().contains(Backends::DX12) {
        return Vec::new();
    }
    let WgpuSetup::CreateNew(create) = &options.wgpu_options.wgpu_setup else {
        return Vec::new();
    };
    let instance = Instance::new(InstanceDescriptor { backends: Backends::DX12, ..InstanceDescriptor::new_without_display_handle() });
    pollster::block_on(instance.enumerate_adapters(Backends::DX12))
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            let required = (create.device_descriptor)(adapter).required_limits;
            let meets_limits = required.check_limits(&adapter.limits());
            log::info!("graphics: DirectX 12 adapter {:?} ({:?}, driver {:?}), limits ok: {meets_limits}", info.name, info.device_type, info.driver);
            Dx12Adapter { device_type: info.device_type, meets_limits }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_backends(backends: Backends) -> Backends {
        let mut options = NativeOptions::default();
        configure_backends(&mut options, backends);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.backends
    }

    #[test]
    fn devices_are_asked_for_small_memory_blocks() {
        let desc = lean_memory(DeviceDescriptor::default());
        assert!(matches!(desc.memory_hints, MemoryHints::MemoryUsage));
        assert!(matches!(DeviceDescriptor::default().memory_hints, MemoryHints::Performance), "wgpu's default moved; re-measure");
    }

    const fn adapter(device_type: DeviceType, meets_limits: bool) -> Dx12Adapter {
        Dx12Adapter { device_type, meets_limits }
    }

    #[test]
    fn windows_default_never_initializes_vulkan() {
        let gpu = [adapter(DeviceType::IntegratedGpu, true)];
        assert_eq!(configured_backends(choose_backends(None, &gpu)), Backends::DX12);
        let probes: [&[Dx12Adapter]; 4] = [&[], &gpu, &[adapter(DeviceType::Cpu, true)], &[adapter(DeviceType::IntegratedGpu, false)]];
        for adapters in probes {
            let first = choose_backends(None, adapters);
            assert!(!first.contains(Backends::VULKAN), "{adapters:?}");
            if let Some(second) = fallback_backends(None, first) {
                assert!(!second.contains(Backends::VULKAN), "{adapters:?}");
            }
        }
    }

    #[test]
    fn explicit_backend_override_is_preserved() {
        let warp_only = [adapter(DeviceType::Cpu, true)];
        for backends in [Backends::VULKAN, Backends::GL, Backends::DX12, Backends::VULKAN | Backends::DX12, Backends::empty()] {
            assert_eq!(choose_backends(Some(backends), &[]), backends);
            assert_eq!(choose_backends(Some(backends), &warp_only), backends);
            assert_eq!(configured_backends(backends), backends);
            assert_eq!(fallback_backends(Some(backends), backends), None);
        }
    }

    #[test]
    fn gl_when_dx12_has_no_usable_gpu() {
        let warp_only = [adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &warp_only), Backends::GL | Backends::DX12);
        assert_eq!(choose_backends(None, &[]), Backends::GL);
        let weak_gpu = [adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &weak_gpu), Backends::GL);
        let mixed = [adapter(DeviceType::Cpu, true), adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::DiscreteGpu, true)];
        assert_eq!(choose_backends(None, &mixed), Backends::DX12);
    }

    #[test]
    fn failed_start_retries_with_the_other_backend() {
        assert_eq!(fallback_backends(None, Backends::DX12), Some(Backends::GL));
        assert_eq!(fallback_backends(None, Backends::GL), Some(Backends::DX12));
        assert_eq!(fallback_backends(None, Backends::DX12 | Backends::GL), None);
    }

    #[test]
    fn dx12_compiles_shaders_with_fxc_unless_asked() {
        assert!(matches!(dx12_compiler(None), Dx12Compiler::Fxc));
        assert!(matches!(dx12_compiler(Some(Dx12Compiler::Auto)), Dx12Compiler::Auto));
    }
}
