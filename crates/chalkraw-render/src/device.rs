use crate::error::RenderError;
use std::sync::Arc;

/// Owned wgpu device/queue for non-surface rendering (offscreen + tests).
///
/// In the UI, the same device/queue passed by `egui-wgpu` is reused — see
/// `RenderDevice::from_shared`.
pub struct RenderDevice {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

impl RenderDevice {
    /// Initialise a headless device suitable for offscreen rendering and tests.
    pub fn new_headless() -> Result<Self, RenderError> {
        pollster::block_on(Self::new_headless_async())
    }

    pub async fn new_headless_async() -> Result<Self, RenderError> {
        // wgpu 27: Instance::new takes &InstanceDescriptor (by reference).
        // InstanceDescriptor implements Default.
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        // wgpu 27: request_adapter returns Result<Adapter, RequestAdapterError>.
        // Map any error to RenderError::NoAdapter.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("chalkraw render device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
                ..Default::default()
            })
            .await?;
        Ok(Self { device: Arc::new(device), queue: Arc::new(queue) })
    }

    /// Wrap an externally-owned device/queue (e.g. from egui-wgpu).
    pub fn from_shared(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self { device, queue }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_device_initialises_or_skips_in_sandbox() {
        // CI containers without GPU access cannot satisfy this. Treat NoAdapter
        // as a skipped test rather than a hard failure.
        match RenderDevice::new_headless() {
            Ok(_) => {}
            Err(RenderError::NoAdapter(msg)) => {
                eprintln!("skipping: no GPU adapter available: {msg}");
            }
            Err(e) => panic!("unexpected init failure: {e}"),
        }
    }
}
