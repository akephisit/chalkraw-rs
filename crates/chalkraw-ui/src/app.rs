use crate::canvas::{CanvasCallback, CanvasGpu};
use crate::panels::{left_panel, right_panel};
use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, ImageFormat, Photo, PhotoId};
use chalkraw_io::{decode_image, LinearImage};
use chalkraw_render::RenderDevice;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(100);

pub struct AppState {
    pub edit: EditState,
    pub image: LinearImage,
    pub photo_id: PhotoId,
    pub catalog: Catalog,
    pub dirty_since: Option<Instant>,
}

impl AppState {
    pub fn bootstrap(fixture: PathBuf, catalog_path: PathBuf) -> anyhow::Result<Self> {
        let image = decode_image(&fixture)
            .map_err(|e| anyhow::anyhow!("decode {fixture:?}: {e}"))?;
        let catalog = Catalog::open_or_create(&catalog_path, "default")?;

        // First-run: create a Photo row for the fixture if the catalog is empty.
        let existing = catalog.list_photos()?;
        let (photo, edit) = if let Some(p) = existing.into_iter().next() {
            let e = catalog.get_edit(p.id)?;
            (p, e)
        } else {
            let hash = *blake3::hash(&std::fs::read(&fixture)?).as_bytes();
            let p = Photo::new(fixture.clone(), hash, image.width, image.height, ImageFormat::Jpeg);
            catalog.insert_photo(&p)?;
            (p, EditState::default())
        };

        Ok(Self {
            edit,
            image,
            photo_id: photo.id,
            catalog,
            dirty_since: None,
        })
    }

    pub fn mark_dirty(&mut self) { self.dirty_since = Some(Instant::now()); }

    pub fn flush_if_due(&mut self) {
        let due = self.dirty_since.map(|t| t.elapsed() >= DEBOUNCE).unwrap_or(false);
        if due {
            if let Err(e) = self.catalog.upsert_edit(self.photo_id, &self.edit) {
                log::warn!("autosave failed: {e}");
            } else {
                self.dirty_since = None;
                log::debug!("autosave committed");
            }
        }
    }
}

pub struct ChalkrawApp {
    state: AppState,
    gpu: Option<Arc<CanvasGpu>>,
}

impl ChalkrawApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        let fixture: PathBuf = std::env::var_os("CHALKRAW_FIXTURE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let mut p = std::env::current_dir().unwrap();
                p.push("tests/fixtures/sample.jpg");
                p
            });
        let catalog_path: PathBuf = std::env::var_os("CHALKRAW_CATALOG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("default.chalkraw"));
        let state = AppState::bootstrap(fixture, catalog_path)?;
        Ok(Self { state, gpu: None })
    }

    fn ensure_gpu(&mut self, frame: &eframe::Frame) {
        if self.gpu.is_some() { return; }

        // egui-wgpu 0.34: `frame.wgpu_render_state()` returns
        // `Option<&egui_wgpu::RenderState>`, NOT `Option<Arc<...>>`.
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };

        // egui-wgpu 0.34 / wgpu 29: `RenderState.device` and `.queue` are plain
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
    /// Non-UI work: GPU initialisation and debounced autosave.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ensure_gpu(frame);

        // Debounced autosave; request a repaint slightly past the debounce so we
        // get woken even when the user has stopped interacting.
        self.state.flush_if_due();
        if self.state.dirty_since.is_some() {
            ctx.request_repaint_after(DEBOUNCE + Duration::from_millis(20));
        }
    }

    /// UI: panel layout using egui 0.34's show_inside API.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show_inside(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| { ui.label("(Phase 7)"); });
                let path = self.state.catalog.path().display().to_string();
                ui.label(format!("  catalog: {path}"));
            });
        });

        egui::Panel::left("left").default_size(220.0).show_inside(ui, |ui| {
            left_panel(ui, &mut self.state);
        });

        let mut edit_changed = false;
        egui::Panel::right("right").default_size(280.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                edit_changed = right_panel(ui, &mut self.state.edit);
            });
        });

        egui::Panel::bottom("filmstrip").default_size(120.0).show_inside(ui, |ui| {
            ui.label("Filmstrip (Phase 3)");
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(gpu) = self.gpu.as_ref() {
                if edit_changed {
                    gpu.update(&self.state.edit);
                    self.state.mark_dirty();
                }
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());

                // egui-wgpu 0.34 / epaint 0.34: `Callback::new_paint_callback`
                // returns `epaint::PaintCallback`, which implements `From<PaintCallback>
                // for Shape`, so it can be passed directly to `painter.add(...)`.
                ui.painter().add(
                    egui_wgpu::Callback::new_paint_callback(
                        rect,
                        CanvasCallback { gpu: gpu.clone() },
                    )
                );
            } else {
                ui.label("Initialising GPU…");
            }
        });
    }
}
