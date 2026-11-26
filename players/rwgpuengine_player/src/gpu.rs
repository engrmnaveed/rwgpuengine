//! The graphics adapter and its device, and what went wrong with them, as
//! diagnostics with the codes of `rwgpu explain E0501`..`E0503`.

use std::sync::{Arc, Mutex};

use rwgpuengine_core::codes;
use rwgpuengine_core::diag::Diagnostic;
use rwgpuengine_core::json::Json;
use rwgpuengine_gpu::wgpu;

/// An adapter and the device the renderer draws with.
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    /// The first error the device reported on its own.
    errors: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu").field("adapter", &self.info.name).finish()
    }
}

/// wgpu's instance, with its backends chosen as wgpu's own variables say
/// (`WGPU_BACKEND`).
pub fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// Run a future that never really waits: natively, wgpu's are ready at once.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

/// "Apple M5 Pro (Metal)".
pub fn describe(info: &wgpu::AdapterInfo) -> String {
    let backend = match info.backend {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "Direct3D 12",
        wgpu::Backend::Gl => "GL",
        wgpu::Backend::BrowserWebGpu => "WebGPU",
        _ => "no backend",
    };
    format!("{} ({backend}{})", info.name, if info.device_type == wgpu::DeviceType::Cpu { ", software" } else { "" })
}

impl Gpu {
    /// The adapter wgpu prefers (`HighPerformance`), one that can show a
    /// picture on `surface` when there is one, and a device with no features
    /// and the downlevel limits: all `rwgpuengine_gpu` needs (ADR 0021).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu, Diagnostic> {
        let software = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").is_ok_and(|v| v == "1");
        block_on(Gpu::request(instance, surface, software))
    }

    /// [`Gpu::open`] for a caller that can wait: a browser, where the adapter
    /// and the device come in promises. `software` asks for a fallback
    /// (software) adapter only.
    pub async fn request(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        software: bool,
    ) -> Result<Gpu, Diagnostic> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: software,
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|e| {
                let backend = std::env::var("WGPU_BACKEND").map(|b| format!(" (WGPU_BACKEND={b})")).unwrap_or_default();
                let what = if surface.is_some() { "that can show a picture in this window" } else { "that can draw" };
                Diagnostic::new(&codes::NO_GPU_ADAPTER, format!("no graphics adapter {what}{backend}: {e}")).hint(
                    "install or update the graphics driver (Vulkan, Metal or Direct3D 12); on Linux without a graphics \
                 card: `sudo apt-get install mesa-vulkan-drivers libvulkan1`",
                )
            })?;
        let info = adapter.get_info();
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() })
            .await
            .map_err(|e| failed(format!("{} gave no device: {e}", describe(&info))))?;
        let errors = Arc::new(Mutex::new(None));
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| note(&sink, e.to_string())));
        let sink = errors.clone();
        device.set_device_lost_callback(move |reason, message| {
            if reason != wgpu::DeviceLostReason::Destroyed {
                note(&sink, format!("the device was lost: {message}"));
            }
        });
        Ok(Gpu { adapter, device, queue, info, errors })
    }

    /// An error the device reported since it was opened, as a diagnostic.
    pub fn check(&self) -> Result<(), Diagnostic> {
        let seen = self.errors.lock().unwrap_or_else(|p| p.into_inner()).clone();
        match seen {
            None => Ok(()),
            Some(e) => Err(failed(format!("{}: {e}", describe(&self.info)))),
        }
    }

    /// Wait until the GPU has done everything submitted.
    pub fn wait(&self) -> Result<(), Diagnostic> {
        self.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .map(|_| ())
            .map_err(|e| failed(format!("{}: {e}", describe(&self.info))))?;
        self.check()
    }

    /// The adapter, for `--json`.
    pub fn json(&self) -> Json {
        Json::object()
            .with("name", self.info.name.as_str())
            .with("backend", format!("{:?}", self.info.backend).to_lowercase())
            .with("software", self.info.device_type == wgpu::DeviceType::Cpu)
            .with("driver", format!("{} {}", self.info.driver, self.info.driver_info).trim())
    }
}

fn note(sink: &Mutex<Option<String>>, e: String) {
    let mut seen = sink.lock().unwrap_or_else(|p| p.into_inner());
    if seen.is_none() {
        *seen = Some(e);
    }
}

/// E0503: the device refused something.
pub fn failed(message: String) -> Diagnostic {
    Diagnostic::new(&codes::GPU_FAILED, message)
        .hint("update the graphics driver, or try another backend with `WGPU_BACKEND` (vulkan, metal, dx12)")
}
