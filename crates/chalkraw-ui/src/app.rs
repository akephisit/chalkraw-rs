use crate::canvas::{CanvasCallback, CanvasGpu};
use crate::panels::{left_panel, right_panel};
use chalkraw_core::EditState;
use chalkraw_io::{decode_image, LinearImage};
use chalkraw_render::RenderDevice;
use std::sync::Arc;

pub struct AppState {
    pub edit: EditState,
    pub image: LinearImage,
}

impl AppState {
    pub fn new(image: LinearImage) -> Self { Self { edit: EditState::default(), image } }
}

pub struct ChalkrawApp {
    state: AppState,
    gpu: Option<Arc<CanvasGpu>>,
}

impl ChalkrawApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        // Phase 1: hardcoded fixture path; Phase 3 will replace this with import flow.
        let fixture = std::env::var("CHALKRAW_FIXTURE")
            .unwrap_or_else(|_| {
                let mut p = std::env::current_dir().unwrap();
                p.push("tests/fixtures/sample.jpg");
                p.to_string_lossy().into_owned()
            });
        let image = decode_image(&fixture)
            .map_err(|e| anyhow::anyhow!("failed to load fixture {fixture}: {e}"))?;
        Ok(Self { state: AppState::new(image), gpu: None })
    }

    fn ensure_gpu(&mut self, frame: &eframe::Frame) {
        if self.gpu.is_some() { return; }

        // egui-wgpu 0.33.3: `frame.wgpu_render_state()` returns
        // `Option<&egui_wgpu::RenderState>`, NOT `Option<Arc<...>>`.
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };

        // egui-wgpu 0.33.3 / wgpu 27: `RenderState.device` and `.queue` are plain
        // `wgpu::Device` / `wgpu::Queue` (both derive `Clone`; the clone is a
        // cheap internal Arc clone), not wrapped in `Arc<...>` themselves.
        // `RenderDevice::from_shared` wants `Arc<Device>` + `Arc<Queue>`,
        // so we clone the handle and wrap it.
        let rd = RenderDevice::from_shared(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        );

        let format = render_state.target_format;
        let gpu = CanvasGpu::new(&rd, &self.state.image, format);
        gpu.update(&self.state.edit);
        self.gpu = Some(Arc::new(gpu));
    }
}

impl eframe::App for ChalkrawApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ensure_gpu(frame);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| { ui.label("(Phase 7)"); });
            });
        });

        egui::SidePanel::left("left").default_width(220.0).show(ctx, |ui| {
            left_panel(ui, &mut self.state);
        });

        let mut edit_changed = false;
        egui::SidePanel::right("right").default_width(280.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                edit_changed = right_panel(ui, &mut self.state.edit);
            });
        });

        egui::TopBottomPanel::bottom("filmstrip").default_height(120.0).show(ctx, |ui| {
            ui.label("Filmstrip (Phase 3)");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(gpu) = self.gpu.as_ref() {
                if edit_changed { gpu.update(&self.state.edit); }
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

                // egui-wgpu 0.33.3: `Callback::new_paint_callback` returns
                // `epaint::PaintCallback`, which does NOT implement `Into<Shape>`
                // directly. Wrap it in `egui::Shape::Callback(...)` to satisfy
                // `painter.add(impl Into<Shape>)`.
                ui.painter().add(egui::Shape::Callback(
                    egui_wgpu::Callback::new_paint_callback(
                        rect,
                        CanvasCallback { gpu: gpu.clone() },
                    )
                ));
            } else {
                ui.label("Initialising GPU…");
            }
        });
    }
}
