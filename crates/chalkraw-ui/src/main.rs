mod app;
mod canvas;
mod panels;

use app::ChalkrawApp;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,chalkraw_ui::app=debug"),
    )
    .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("chalkraw"),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: prefer_discrete_gpu_wgpu_options(),
        ..Default::default()
    };

    eframe::run_native(
        "chalkraw",
        native_options,
        Box::new(|cc| Ok(Box::new(ChalkrawApp::new(cc)?))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

fn prefer_discrete_gpu_wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut config = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(ref mut setup) = config.wgpu_setup {
        setup.power_preference = wgpu::PowerPreference::HighPerformance;
        setup.native_adapter_selector = Some(Arc::new(|adapters, surface| {
            let adapter = adapters
                .iter()
                .filter(|adapter| adapter_supports_surface(adapter, surface))
                .find(|adapter| adapter.get_info().device_type == wgpu::DeviceType::DiscreteGpu)
                .or_else(|| {
                    adapters
                        .iter()
                        .filter(|adapter| adapter_supports_surface(adapter, surface))
                        .find(|adapter| {
                            adapter.get_info().device_type == wgpu::DeviceType::IntegratedGpu
                        })
                })
                .or_else(|| {
                    adapters
                        .iter()
                        .find(|adapter| adapter_supports_surface(adapter, surface))
                })
                .ok_or_else(|| "no wgpu adapter supports the window surface".to_owned())?;

            let info = adapter.get_info();
            log::info!(
                "selected wgpu adapter: {} ({:?}, backend {:?}, vendor 0x{:04X})",
                info.name,
                info.device_type,
                info.backend,
                info.vendor
            );
            Ok(adapter.clone())
        }));
    }
    config
}

fn adapter_supports_surface(adapter: &wgpu::Adapter, surface: Option<&wgpu::Surface<'_>>) -> bool {
    surface.is_none_or(|surface| adapter.is_surface_supported(surface))
}
