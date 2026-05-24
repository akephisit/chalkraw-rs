use crate::canvas::{CanvasCallback, CanvasGpu};
use crate::panels::{left_panel, right_panel};
use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, Flag, ImageFormat, Photo, PhotoId};
use chalkraw_io::{decode_image, decode_image_bytes, LinearImage};
use chalkraw_render::RenderDevice;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(100);

/// Embedded sample image used when no fixture path is supplied or the
/// supplied path doesn't exist.
const EMBEDDED_FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/sample.jpg");

// ── AppState ─────────────────────────────────────────────────────────────────

pub struct AppState {
    pub edit: EditState,
    pub image: LinearImage,
    pub photo_id: PhotoId,
    pub current_flag: Flag,
    pub catalog: Catalog,
    pub dirty_since: Option<Instant>,
    pub new_preset_name: String,
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
            new_preset_name: String::new(),
        })
    }

    pub fn switch_to_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
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

    pub fn save_preset(&self, name: String) -> anyhow::Result<()> {
        let dp = chalkraw_core::DevelopPreset::from(&self.edit);
        let preset = chalkraw_core::Preset::new(name, dp);
        self.catalog.insert_preset(&preset)?;
        Ok(())
    }

    pub fn apply_preset(&mut self, id: chalkraw_core::PresetId) -> anyhow::Result<()> {
        let preset = self.catalog
            .list_presets()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("preset {id} not found"))?;
        self.edit.apply_preset(&preset.develop);
        self.mark_dirty();
        Ok(())
    }

    pub fn delete_preset(&self, id: chalkraw_core::PresetId) -> anyhow::Result<()> {
        self.catalog.delete_preset(id).map_err(Into::into)
    }

    /// Navigate to the next (+1) or previous (−1) photo in the catalog.
    /// Wraps around at both ends (Lightroom convention).
    pub fn navigate(&mut self, delta: i32) -> anyhow::Result<()> {
        let photos = self.catalog.list_photos()?;
        if photos.is_empty() {
            return Ok(());
        }
        let current_idx = photos.iter().position(|p| p.id == self.photo_id).unwrap_or(0);
        let new_idx =
            ((current_idx as i32 + delta).rem_euclid(photos.len() as i32)) as usize;
        let new_path = photos[new_idx].original_path.clone();
        self.switch_to_path(new_path)
    }

    /// Gather batch items from the catalog. If `only_picks` is true, filters to
    /// photos flagged as Pick. Skips photos whose original_path doesn't exist.
    pub fn collect_batch_items(
        &self,
        only_picks: bool,
    ) -> anyhow::Result<Vec<chalkraw_export::BatchItem>> {
        let photos = self.catalog.list_photos()?;
        let mut items = Vec::new();
        for p in photos {
            if only_picks && p.flag != Flag::Pick {
                continue;
            }
            if p.original_path.as_os_str() == "<embedded>" {
                log::warn!(
                    "skip embedded fixture photo {}: cannot export embedded image",
                    p.id
                );
                continue;
            }
            if !p.original_path.exists() {
                log::warn!("skip {:?}: file not found at original_path", p.original_path);
                continue;
            }
            let edit = self.catalog.get_edit(p.id)?;
            let original_name = p
                .original_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "photo".to_string());
            items.push(chalkraw_export::BatchItem {
                source_path: p.original_path,
                edit,
                original_name,
            });
        }
        log::info!(
            "export filter: only_picks={only_picks}, catalog has {} photos; selected {} for export",
            self.catalog.list_photos()?.len(),
            items.len()
        );
        Ok(items)
    }
}

// ── Export dialog state ───────────────────────────────────────────────────────

/// 9-point anchor grid index (row-major: 0=TopLeft … 8=BottomRight).
fn anchor_index_to_enum(i: usize) -> chalkraw_export::WatermarkAnchor {
    use chalkraw_export::WatermarkAnchor;
    match i {
        0 => WatermarkAnchor::TopLeft,
        1 => WatermarkAnchor::TopCenter,
        2 => WatermarkAnchor::TopRight,
        3 => WatermarkAnchor::CenterLeft,
        4 => WatermarkAnchor::Center,
        5 => WatermarkAnchor::CenterRight,
        6 => WatermarkAnchor::BottomLeft,
        7 => WatermarkAnchor::BottomCenter,
        _ => WatermarkAnchor::BottomRight,
    }
}

struct WatermarkDialogState {
    enabled: bool,
    png_path: Option<PathBuf>,
    /// 0..8, row-major
    anchor_idx: usize,
    /// 1..50
    size_pct: f32,
    /// 0..100 (stored as 0..100, converted to 0..1 on use)
    opacity_pct: f32,
    /// 0..20
    margin_pct: f32,
}

impl Default for WatermarkDialogState {
    fn default() -> Self {
        Self {
            enabled: false,
            png_path: None,
            anchor_idx: 8, // BottomRight
            size_pct: 15.0,
            opacity_pct: 80.0,
            margin_pct: 3.0,
        }
    }
}

impl WatermarkDialogState {
    fn to_stamp(&self) -> Option<chalkraw_export::WatermarkStamp> {
        if !self.enabled {
            return None;
        }
        let png_path = self.png_path.clone()?;
        Some(chalkraw_export::WatermarkStamp {
            png_path,
            anchor: anchor_index_to_enum(self.anchor_idx),
            size_pct: self.size_pct,
            opacity: self.opacity_pct / 100.0,
            margin_pct: self.margin_pct,
        })
    }
}

// ── Batch progress (shared with export thread) ────────────────────────────────

struct BatchProgress {
    current: usize,
    total: usize,
    name: String,
    done: bool,
    results: Vec<chalkraw_export::BatchItemResult>,
    error: Option<String>,
}

struct ExportDialogState {
    // Format
    format_index: usize, // 0=JPEG, 1=PNG, 2=TIFF
    quality: u8,
    // Resize
    resize_long_edge: bool,
    long_edge: u32,
    // Batch source
    only_picks: bool,
    // Output
    output_dir: Option<PathBuf>,
    name_pattern: String,
    // Watermark
    watermark: WatermarkDialogState,
    // Runtime state
    batch_progress: Option<Arc<Mutex<BatchProgress>>>,
    /// Set when the batch completed — holds counts for display.
    completion_message: Option<String>,
}

impl Default for ExportDialogState {
    fn default() -> Self {
        Self {
            format_index: 0,
            quality: 92,
            resize_long_edge: false,
            long_edge: 2048,
            only_picks: false,
            output_dir: None,
            name_pattern: "{name}_edited".to_string(),
            watermark: WatermarkDialogState::default(),
            batch_progress: None,
            completion_message: None,
        }
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

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
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };
        let rd = RenderDevice::from_shared(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        );
        let format = render_state.target_format;
        log::info!("wgpu surface target_format = {format:?}");
        let gpu = CanvasGpu::new(&rd, &self.state.image, format);
        gpu.update(&self.state.edit);
        self.gpu = Some(Arc::new(gpu));
    }

    fn ensure_thumb(&mut self, ctx: &egui::Context, photo: &Photo) -> egui::TextureHandle {
        self.thumb_textures
            .entry(photo.id)
            .or_insert_with(|| {
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

        let mut to_set: Option<Flag> = None;
        let mut nav: Option<i32> = None;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::P) {
                to_set = Some(Flag::Pick);
            } else if i.key_pressed(egui::Key::U) {
                to_set = Some(Flag::None);
            } else if i.key_pressed(egui::Key::X) {
                to_set = Some(Flag::Reject);
            }

            // Issue 5: Lightroom-style photo navigation.
            // ArrowRight / ] → next photo.  ArrowLeft / [ → previous photo.
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::CloseBracket) {
                nav = Some(1);
            } else if i.key_pressed(egui::Key::ArrowLeft)
                || i.key_pressed(egui::Key::OpenBracket)
            {
                nav = Some(-1);
            }
        });
        if let Some(f) = to_set {
            self.state.set_current_flag(f);
        }
        if let Some(delta) = nav {
            if let Err(e) = self.state.navigate(delta) {
                log::warn!("navigate failed: {e}");
            } else {
                self.gpu = None; // force CanvasGpu rebuild for new source
            }
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
                    if ui.button("Batch Export…").clicked() {
                        ui.close();
                        self.export_dialog = Some(ExportDialogState::default());
                    }
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| {
                    if ui.button("Batch Export…").clicked() {
                        ui.close();
                        self.export_dialog = Some(ExportDialogState::default());
                    }
                });
                let path = self.state.catalog.path().display().to_string();
                ui.label(format!("  catalog: {path}  |  P=Pick  U=None  X=Reject"));
            });
        });

        let mut edit_changed = false;
        egui::SidePanel::left("left").default_width(220.0).show(ctx, |ui| {
            edit_changed |= left_panel(ui, &mut self.state);
        });

        egui::SidePanel::right("right").default_width(280.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                edit_changed |= right_panel(ui, &mut self.state.edit);
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
                    // Re-run all four blurs on every edit change. Clarity uses a fixed
                    // sigma=16; Sharpening uses the radius slider as sigma; Texture
                    // uses a fixed sigma=5; NR uses a fixed sigma=2.
                    gpu.run_blurs(16.0, self.state.edit.detail.sharpening.radius, 5.0, 2.0);
                    self.state.mark_dirty();
                }
                // Issue 2: letterbox — preserve image aspect ratio rather than
                // stretching to fill the entire central panel.
                let available = ui.available_size();
                let img_aspect = self.state.image.width as f32 / self.state.image.height as f32;
                let avail_aspect = available.x / available.y;
                let (rect_w, rect_h) = if img_aspect >= avail_aspect {
                    // Image wider than panel — fit width, letterbox top/bottom.
                    (available.x, available.x / img_aspect)
                } else {
                    // Image taller than panel — fit height, letterbox left/right.
                    (available.y * img_aspect, available.y)
                };
                let (full_rect, _) =
                    ui.allocate_exact_size(available, egui::Sense::drag());
                let image_rect = egui::Rect::from_center_size(
                    full_rect.center(),
                    egui::vec2(rect_w, rect_h),
                );
                ui.painter().add(egui::Shape::Callback(
                    egui_wgpu::Callback::new_paint_callback(
                        image_rect,
                        CanvasCallback { gpu: gpu.clone() },
                    ),
                ));
            } else {
                ui.label("Initialising GPU…");
            }
        });

        // ── Export dialog ─────────────────────────────────────────────────────
        if let Some(dlg) = self.export_dialog.as_mut() {
            let mut open = true;
            let mut should_close = false;
            let mut should_start_export = false;

            egui::Window::new("Batch Export")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .show(ctx, |ui| {
                    // ── Check if batch running ────────────────────────────────
                    if let Some(ref progress_arc) = dlg.batch_progress {
                        let prog = progress_arc.lock().unwrap();
                        if prog.done {
                            // Finished — show results
                            if let Some(ref msg) = dlg.completion_message {
                                ui.label(msg);
                            } else {
                                let ok = prog.results.iter().filter(|r| r.error.is_none()).count();
                                let err = prog.results.iter().filter(|r| r.error.is_some()).count();
                                let msg = format!(
                                    "Done: {ok} exported, {err} failed.",
                                );
                                drop(prog);
                                dlg.completion_message = Some(msg.clone());
                                ui.label(msg);
                            }
                            if ui.button("Close").clicked() {
                                should_close = true;
                            }
                        } else {
                            // In progress
                            ui.label(format!(
                                "Exporting {}/{} — {}",
                                prog.current, prog.total, prog.name
                            ));
                            ui.add(egui::ProgressBar::new(
                                if prog.total > 0 {
                                    prog.current.saturating_sub(1) as f32 / prog.total as f32
                                } else {
                                    0.0
                                },
                            ));
                            if let Some(ref e) = prog.error {
                                ui.colored_label(egui::Color32::RED, e);
                            }
                            drop(prog);
                            if ui.button("Cancel (closes dialog; export continues in background)").clicked() {
                                should_close = true;
                            }
                            ctx.request_repaint_after(Duration::from_millis(200));
                        }
                        return;
                    }

                    // ── Normal controls ───────────────────────────────────────
                    egui::CollapsingHeader::new("Format & Size")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut dlg.format_index, 0, "JPEG");
                                ui.radio_value(&mut dlg.format_index, 1, "PNG");
                                ui.radio_value(&mut dlg.format_index, 2, "TIFF");
                            });
                            if dlg.format_index == 0 {
                                ui.horizontal(|ui| {
                                    ui.label("Quality");
                                    ui.add(egui::Slider::new(&mut dlg.quality, 1..=100));
                                });
                            }
                            ui.checkbox(&mut dlg.resize_long_edge, "Resize long edge (px)");
                            if dlg.resize_long_edge {
                                ui.add(egui::Slider::new(&mut dlg.long_edge, 256..=8192));
                            }
                        });

                    ui.separator();

                    // Source toggle
                    ui.horizontal(|ui| {
                        ui.label("Source:");
                        ui.radio_value(&mut dlg.only_picks, false, "All photos");
                        ui.radio_value(&mut dlg.only_picks, true, "Only Picks");
                    });

                    ui.separator();

                    // Output folder
                    ui.horizontal(|ui| {
                        ui.label("Output folder:");
                        if ui.button("Choose…").clicked() {
                            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                dlg.output_dir = Some(folder);
                            }
                        }
                    });
                    match &dlg.output_dir {
                        Some(p) => { ui.label(p.display().to_string()); }
                        None    => { ui.colored_label(egui::Color32::YELLOW, "(no folder selected)"); }
                    }

                    // File name pattern
                    ui.horizontal(|ui| {
                        ui.label("File name pattern:");
                        ui.add(
                            egui::TextEdit::singleline(&mut dlg.name_pattern)
                                .desired_width(180.0)
                                .hint_text("{name}_edited"),
                        );
                    });
                    ui.label(
                        egui::RichText::new("Tokens: {name}  {date}  {ext}")
                            .small()
                            .color(egui::Color32::GRAY),
                    );

                    ui.separator();

                    // Watermark section
                    egui::CollapsingHeader::new("Watermark")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.checkbox(&mut dlg.watermark.enabled, "Enable PNG watermark stamp");
                            if dlg.watermark.enabled {
                                ui.horizontal(|ui| {
                                    ui.label("PNG file:");
                                    if ui.button("Browse…").clicked() {
                                        if let Some(p) = rfd::FileDialog::new()
                                            .add_filter("PNG", &["png"])
                                            .pick_file()
                                        {
                                            dlg.watermark.png_path = Some(p);
                                        }
                                    }
                                });
                                match &dlg.watermark.png_path {
                                    Some(p) => { ui.label(p.display().to_string()); }
                                    None    => { ui.colored_label(egui::Color32::YELLOW, "(no PNG selected)"); }
                                }

                                ui.add_space(4.0);
                                ui.label("Anchor (click to choose):");
                                // 3×3 anchor grid
                                let labels = [
                                    "↖ TL", "↑ TC", "↗ TR",
                                    "← CL", "·  C", "→ CR",
                                    "↙ BL", "↓ BC", "↘ BR",
                                ];
                                egui::Grid::new("anchor_grid").num_columns(3).show(ui, |ui| {
                                    for (i, label) in labels.iter().enumerate() {
                                        let selected = dlg.watermark.anchor_idx == i;
                                        if ui.add(egui::Button::new(*label).selected(selected)).clicked() {
                                            dlg.watermark.anchor_idx = i;
                                        }
                                        if i % 3 == 2 { ui.end_row(); }
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Size (% long edge):");
                                    ui.add(egui::Slider::new(&mut dlg.watermark.size_pct, 1.0..=50.0).fixed_decimals(0));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Opacity (%):");
                                    ui.add(egui::Slider::new(&mut dlg.watermark.opacity_pct, 0.0..=100.0).fixed_decimals(0));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Margin (% long edge):");
                                    ui.add(egui::Slider::new(&mut dlg.watermark.margin_pct, 0.0..=20.0).fixed_decimals(0));
                                });
                            }
                        });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            should_close = true;
                        }
                        let can_export = dlg.output_dir.is_some();
                        ui.add_enabled_ui(can_export, |ui| {
                            if ui.button("Export").clicked() {
                                should_start_export = true;
                            }
                        });
                        if !can_export {
                            ui.label(egui::RichText::new("← choose output folder first").color(egui::Color32::GRAY).small());
                        }
                    });
                });

            if !open || should_close {
                self.export_dialog = None;
            } else if should_start_export {
                // Build batch options from dialog state.
                if let Some(ref dlg_inner) = self.export_dialog {
                    let output_dir = dlg_inner.output_dir.clone().unwrap();
                    let format = match dlg_inner.format_index {
                        1 => chalkraw_export::ExportFormat::Png,
                        2 => chalkraw_export::ExportFormat::Tiff,
                        _ => chalkraw_export::ExportFormat::Jpeg { quality: dlg_inner.quality },
                    };
                    let resize = if dlg_inner.resize_long_edge {
                        chalkraw_export::ExportResize::LongEdge(dlg_inner.long_edge)
                    } else {
                        chalkraw_export::ExportResize::Original
                    };
                    let opts = chalkraw_export::BatchOptions {
                        format,
                        resize,
                        output_dir,
                        name_pattern: dlg_inner.name_pattern.clone(),
                        watermark: dlg_inner.watermark.to_stamp(),
                    };
                    let only_picks = dlg_inner.only_picks;

                    match self.state.collect_batch_items(only_picks) {
                        Ok(items) => {
                            let total = items.len();
                            let progress = Arc::new(Mutex::new(BatchProgress {
                                current: 0,
                                total,
                                name: String::new(),
                                done: false,
                                results: Vec::new(),
                                error: None,
                            }));
                            let progress_thread = progress.clone();
                            std::thread::spawn(move || {
                                let rd = match RenderDevice::new_headless() {
                                    Ok(rd) => rd,
                                    Err(e) => {
                                        let mut p = progress_thread.lock().unwrap();
                                        p.done = true;
                                        p.error = Some(format!("export device unavailable: {e}"));
                                        return;
                                    }
                                };
                                let results = chalkraw_export::export_batch(
                                    &rd,
                                    &items,
                                    &opts,
                                    |i, n, name| {
                                        let mut p = progress_thread.lock().unwrap();
                                        p.current = i;
                                        p.total = n;
                                        p.name = name.to_string();
                                    },
                                );
                                let mut p = progress_thread.lock().unwrap();
                                p.done = true;
                                p.results = results;
                            });
                            if let Some(ref mut dlg_mut) = self.export_dialog {
                                dlg_mut.batch_progress = Some(progress);
                                // Force label to show 0/N immediately
                                if let Some(ref prog_arc) = dlg_mut.batch_progress {
                                    let mut prog = prog_arc.lock().unwrap();
                                    prog.total = total;
                                }
                            }
                            ctx.request_repaint_after(Duration::from_millis(200));
                        }
                        Err(e) => {
                            log::warn!("collect batch items failed: {e}");
                        }
                    }
                }
            }
        }

        // Debounced autosave.
        self.state.flush_if_due();
        if self.state.dirty_since.is_some() {
            ctx.request_repaint_after(DEBOUNCE + Duration::from_millis(20));
        }
    }
}
