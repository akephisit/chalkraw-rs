use crate::canvas::{CanvasCallback, CanvasGpu};
use crate::panels::{left_panel, right_panel};
use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, Flag, ImageFormat, Photo, PhotoId};
use chalkraw_io::{decode_image, decode_image_bytes, LinearImage};
use chalkraw_render::RenderDevice;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(100);

/// Embedded sample image used when no fixture path is supplied or the
/// supplied path doesn't exist. Lets a downloaded standalone binary open
/// without crashing the moment a user double-clicks it.
const EMBEDDED_FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/sample.jpg");

pub struct AppState {
    pub edit: EditState,
    pub image: LinearImage,
    pub photo_id: PhotoId,
    pub current_flag: Flag,
    pub catalog: Catalog,
    pub dirty_since: Option<Instant>,
}

impl AppState {
    pub fn bootstrap(fixture: PathBuf, catalog_path: PathBuf) -> anyhow::Result<Self> {
        let (image, file_bytes, photo_path) = if fixture.exists() {
            let bytes = std::fs::read(&fixture)?;
            let img = decode_image(&fixture)
                .map_err(|e| anyhow::anyhow!("decode {fixture:?}: {e}"))?;
            (img, bytes, fixture.clone())
        } else {
            log::warn!("fixture {fixture:?} not found; loading embedded sample image");
            let img = decode_image_bytes(EMBEDDED_FIXTURE)
                .map_err(|e| anyhow::anyhow!("decode embedded fixture: {e}"))?;
            (img, EMBEDDED_FIXTURE.to_vec(), PathBuf::from("<embedded>"))
        };
        let catalog = match Catalog::open_or_create(&catalog_path, "default") {
            Ok(c) => c,
            Err(chalkraw_catalog::CatalogError::SchemaVersion { found, expected }) => {
                log::warn!(
                    "catalog schema {found} != expected {expected}; recreating {catalog_path:?}"
                );
                std::fs::remove_file(&catalog_path).ok();
                Catalog::open_or_create(&catalog_path, "default")?
            }
            Err(e) => return Err(e.into()),
        };

        // First-run: create a Photo row for the fixture if the catalog is empty.
        let existing = catalog.list_photos()?;
        let (photo, edit) = if let Some(p) = existing.into_iter().next() {
            let e = catalog.get_edit(p.id)?;
            (p, e)
        } else {
            let hash = *blake3::hash(&file_bytes).as_bytes();
            let thumb = chalkraw_io::make_thumbnail(&image).unwrap_or_default();
            let mut p = Photo::new(photo_path, hash, image.width, image.height, ImageFormat::Jpeg);
            p.thumbnail = thumb;
            catalog.insert_photo(&p)?;
            (p, EditState::default())
        };

        Ok(Self {
            edit,
            image,
            photo_id: photo.id,
            current_flag: photo.flag,
            catalog,
            dirty_since: None,
        })
    }

    /// Switch the current photo to one loaded from `path`. Flushes any pending
    /// autosave on the previous photo first, then loads, hashes, and either looks
    /// up an existing catalog row or inserts a new one.
    pub fn switch_to_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        // Make sure pending edits on the previous photo are committed BEFORE we
        // swap photo_id; otherwise they'd auto-save under the wrong id.
        self.dirty_since = Some(Instant::now() - DEBOUNCE);
        self.flush_if_due();

        let bytes = std::fs::read(&path)?;
        let image = decode_image(&path)
            .map_err(|e| anyhow::anyhow!("decode {path:?}: {e}"))?;
        let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();

        let (photo, edit) = match self.catalog.find_photo_by_hash(&hash)? {
            Some(p) => {
                let e = self.catalog.get_edit(p.id)?;
                (p, e)
            }
            None => {
                let p = Photo::new(path.clone(), hash, image.width, image.height, ImageFormat::Jpeg);
                self.catalog.insert_photo(&p)?;
                (p, EditState::default())
            }
        };

        self.image = image;
        self.photo_id = photo.id;
        self.current_flag = photo.flag;
        self.edit = edit;
        self.dirty_since = None;
        Ok(())
    }

    /// Import a batch of files into the catalog. Decodes each, hashes for
    /// dedup, generates a thumbnail, inserts the photo row. Does NOT switch
    /// the currently displayed photo. Returns the count of newly inserted rows.
    pub fn import_files(&self, paths: &[PathBuf]) -> anyhow::Result<usize> {
        let mut inserted = 0;
        for path in paths {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("skip {path:?}: {e}");
                    continue;
                }
            };
            let img = match decode_image(path) {
                Ok(i) => i,
                Err(e) => {
                    log::warn!("skip {path:?}: {e}");
                    continue;
                }
            };
            let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();
            if self.catalog.find_photo_by_hash(&hash)?.is_some() {
                log::info!("skip {path:?}: already imported");
                continue;
            }
            let thumb = chalkraw_io::make_thumbnail(&img).unwrap_or_default();
            let mut p = Photo::new(path.clone(), hash, img.width, img.height, ImageFormat::Jpeg);
            p.thumbnail = thumb;
            self.catalog.insert_photo(&p)?;
            inserted += 1;
        }
        Ok(inserted)
    }

    pub fn set_current_flag(&mut self, flag: Flag) {
        if let Err(e) = self.catalog.update_flag(self.photo_id, flag) {
            log::warn!("set flag failed: {e}");
        } else {
            self.current_flag = flag;
        }
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

struct ExportDialogState {
    format_index: usize,    // 0=JPEG, 1=PNG, 2=TIFF
    quality: u8,            // 1..100, JPEG only
    resize_long_edge: bool, // toggle
    long_edge: u32,         // 1..32768
}

impl Default for ExportDialogState {
    fn default() -> Self {
        Self { format_index: 0, quality: 92, resize_long_edge: false, long_edge: 2048 }
    }
}

pub struct ChalkrawApp {
    state: AppState,
    gpu: Option<Arc<CanvasGpu>>,
    thumb_textures: HashMap<PhotoId, egui::TextureHandle>,
    export_dialog: Option<ExportDialogState>,
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
        Ok(Self { state, gpu: None, thumb_textures: HashMap::new(), export_dialog: None })
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

    fn ensure_thumb(&mut self, ctx: &egui::Context, photo: &Photo) -> egui::TextureHandle {
        self.thumb_textures
            .entry(photo.id)
            .or_insert_with(|| {
                // Decode the JPEG bytes stored on Photo.thumbnail.
                let img = image::load_from_memory(&photo.thumbnail)
                    .map(|d| d.to_rgba8())
                    .ok();
                let color_image = match img {
                    Some(rgba) => {
                        let (w, h) = rgba.dimensions();
                        egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            rgba.as_raw(),
                        )
                    }
                    None => egui::ColorImage::new([1, 1], vec![egui::Color32::DARK_GRAY]),
                };
                ctx.load_texture(
                    format!("thumb-{}", photo.id),
                    color_image,
                    egui::TextureOptions::LINEAR,
                )
            })
            .clone()
    }
}

impl eframe::App for ChalkrawApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ensure_gpu(frame);

        // Flag keyboard shortcuts: P=Pick, U=None, X=Reject (Lightroom convention).
        // Collect key presses inside the closure then dispatch outside to avoid
        // a borrow-checker conflict between ctx.input and self.state.
        let mut to_set: Option<Flag> = None;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::P) {
                to_set = Some(Flag::Pick);
            } else if i.key_pressed(egui::Key::U) {
                to_set = Some(Flag::None);
            } else if i.key_pressed(egui::Key::X) {
                to_set = Some(Flag::Reject);
            }
        });
        if let Some(f) = to_set {
            self.state.set_current_flag(f);
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Photo…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["jpg", "jpeg", "png", "tif", "tiff"])
                            .pick_file()
                        {
                            if let Err(e) = self.state.switch_to_path(path) {
                                log::warn!("open photo failed: {e}");
                            } else {
                                self.gpu = None;
                                self.thumb_textures.clear();
                            }
                        }
                    }
                    if ui.button("Import Photos…").clicked() {
                        ui.close();
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter("Images", &["jpg", "jpeg", "png", "tif", "tiff"])
                            .pick_files()
                        {
                            match self.state.import_files(&paths) {
                                Ok(n) => log::info!("imported {n} new photos"),
                                Err(e) => log::warn!("import failed: {e}"),
                            }
                            self.thumb_textures.clear();
                        }
                    }
                    if ui.button("Export…").clicked() {
                        ui.close();
                        self.export_dialog = Some(ExportDialogState::default());
                    }
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| { ui.label("(Phase 7)"); });
                let path = self.state.catalog.path().display().to_string();
                ui.label(format!("  catalog: {path}  |  P=Pick  U=None  X=Reject"));
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
            let photos = match self.state.catalog.list_photos() {
                Ok(p) => p,
                Err(e) => {
                    ui.label(format!("filmstrip error: {e}"));
                    return;
                }
            };
            if photos.is_empty() {
                ui.label("No photos yet. File → Import Photos…");
                return;
            }
            let current_id = self.state.photo_id;
            let mut clicked: Option<PathBuf> = None;
            // Collect thumbnails first so the mutable borrow of self (for
            // ensure_thumb) is released before entering the ScrollArea closure.
            let thumbs: Vec<(PhotoId, PathBuf, egui::TextureHandle, Flag)> = photos.iter()
                .map(|p| {
                    let tex = self.ensure_thumb(ctx, p);
                    (p.id, p.original_path.clone(), tex, p.flag)
                })
                .collect();
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (pid, path, tex, flag) in &thumbs {
                        let is_current = *pid == current_id;
                        let img = egui::Image::new(tex).max_height(100.0).max_width(140.0);
                        let response = ui.add(img.sense(egui::Sense::click()));
                        // Draw flag-colour outline first, then current-selection gold on top.
                        let flag_color = match flag {
                            Flag::Pick   => Some(egui::Color32::from_rgb(80, 200, 80)),
                            Flag::Reject => Some(egui::Color32::from_rgb(220, 80, 80)),
                            Flag::None   => None,
                        };
                        if let Some(c) = flag_color {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.0,
                                egui::Stroke::new(2.0, c),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if is_current {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.0,
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 200, 60)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if response.clicked() {
                            clicked = Some(path.clone());
                        }
                    }
                });
            });
            if let Some(path) = clicked {
                if let Err(e) = self.state.switch_to_path(path) {
                    log::warn!("switch_to_path failed: {e}");
                } else {
                    self.gpu = None;
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(gpu) = self.gpu.as_ref() {
                if edit_changed {
                    gpu.update(&self.state.edit);
                    self.state.mark_dirty();
                }
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

        // Export dialog — rendered after all panels so it floats on top.
        if let Some(dlg) = self.export_dialog.as_mut() {
            let mut open = true;
            let mut should_export = false;
            let mut should_close = false;
            egui::Window::new("Export")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Format");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut dlg.format_index, 0, "JPEG");
                        ui.radio_value(&mut dlg.format_index, 1, "PNG");
                        ui.radio_value(&mut dlg.format_index, 2, "TIFF");
                    });
                    if dlg.format_index == 0 {
                        ui.label("Quality");
                        ui.add(egui::Slider::new(&mut dlg.quality, 1..=100));
                    }
                    ui.separator();
                    ui.checkbox(&mut dlg.resize_long_edge, "Resize long edge (px)");
                    if dlg.resize_long_edge {
                        ui.add(egui::Slider::new(&mut dlg.long_edge, 256..=8192));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            should_close = true;
                        }
                        if ui.button("Export").clicked() {
                            should_export = true;
                        }
                    });
                });
            if !open || should_close {
                self.export_dialog = None;
            } else if should_export {
                let opts = chalkraw_export::ExportOptions {
                    format: match dlg.format_index {
                        1 => chalkraw_export::ExportFormat::Png,
                        2 => chalkraw_export::ExportFormat::Tiff,
                        _ => chalkraw_export::ExportFormat::Jpeg { quality: dlg.quality },
                    },
                    resize: if dlg.resize_long_edge {
                        chalkraw_export::ExportResize::LongEdge(dlg.long_edge)
                    } else {
                        chalkraw_export::ExportResize::Original
                    },
                };
                // Capture the data we need before moving into the export branch,
                // to avoid borrow conflicts with self inside the if-let.
                let image_clone = self.state.image.clone();
                let edit_clone = self.state.edit.clone();
                let photo_id = self.state.photo_id;
                // Suggested default filename derived from the current photo's original path.
                let default_name = self.state
                    .catalog
                    .get_photo(photo_id)
                    .ok()
                    .and_then(|p| p.original_path.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "export".to_string());
                let ext = match opts.format {
                    chalkraw_export::ExportFormat::Jpeg { .. } => "jpg",
                    chalkraw_export::ExportFormat::Png => "png",
                    chalkraw_export::ExportFormat::Tiff => "tiff",
                };
                let dst = rfd::FileDialog::new()
                    .set_file_name(format!("{default_name}_edited.{ext}"))
                    .add_filter(ext, &[ext])
                    .save_file();
                if let Some(path) = dst {
                    // Build a fresh RenderDevice for offscreen export. We can't easily
                    // reuse the egui-wgpu device here because we need its Arc<Device>;
                    // a separate headless device is simpler and only costs a one-shot
                    // adapter request.
                    match chalkraw_render::RenderDevice::new_headless() {
                        Ok(rd) => {
                            if let Err(e) = chalkraw_export::export_current(
                                &rd,
                                &image_clone,
                                &edit_clone,
                                &path,
                                opts,
                            ) {
                                log::warn!("export failed: {e}");
                            } else {
                                log::info!("exported {path:?}");
                            }
                        }
                        Err(e) => log::warn!("export render device unavailable: {e}"),
                    }
                }
                self.export_dialog = None;
            }
        }

        // Debounced autosave; request a repaint slightly past the debounce so we
        // get woken even when the user has stopped interacting.
        self.state.flush_if_due();
        if self.state.dirty_since.is_some() {
            ctx.request_repaint_after(DEBOUNCE + Duration::from_millis(20));
        }
    }
}
