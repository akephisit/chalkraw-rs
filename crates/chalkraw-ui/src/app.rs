use crate::canvas::{CanvasCallback, CanvasGpu};
use crate::panels::{left_panel, right_panel, EditChange};
use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, Flag, ImageFormat, Photo, PhotoId};
use chalkraw_io::{decode_image, decode_image_bytes, LinearImage};
use chalkraw_render::RenderDevice;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(100);
const DECODED_CACHE_CAPACITY: usize = 5;
const GPU_CACHE_CAPACITY: usize = 3;

/// Embedded sample image used when no fixture path is supplied or the
/// supplied path doesn't exist.
const EMBEDDED_FIXTURE: &[u8; 27889] = include_bytes!("../../../tests/fixtures/sample.jpg");

// ── AppState ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollectionFilter {
    #[default]
    All,
    Picks,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceMode {
    Library,
    #[default]
    Develop,
}

pub struct AppState {
    pub edit: EditState,
    pub image: LinearImage,
    pub photo_id: PhotoId,
    pub current_flag: Flag,
    pub catalog: Catalog,
    pub photos_cache: Vec<Photo>,
    pub photo_hashes: HashSet<[u8; 32]>,
    pub dirty_since: Option<Instant>,
    pub new_preset_name: String,
    pub folder_filter: Option<std::path::PathBuf>,
    pub collection_filter: CollectionFilter,
    pub watch_folder: Option<std::path::PathBuf>,
    pub last_watch_scan: Option<std::time::Instant>,
    /// Canvas zoom level. 1.0 = fit-to-panel. Range: 0.5..=16.0.
    pub canvas_zoom: f32,
    /// Canvas pan offset in screen pixels from the centred fit position.
    pub canvas_pan: egui::Vec2,
}

impl AppState {
    pub fn bootstrap(fixture: PathBuf, catalog_path: PathBuf) -> anyhow::Result<Self> {
        let (image, file_bytes, photo_path) = if fixture.exists() {
            let bytes = std::fs::read(&fixture)?;
            let img =
                decode_image(&fixture).map_err(|e| anyhow::anyhow!("decode {fixture:?}: {e}"))?;
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
            let mut p = Photo::new(
                photo_path,
                hash,
                image.width,
                image.height,
                ImageFormat::Jpeg,
            );
            p.thumbnail = thumb;
            catalog.insert_photo(&p)?;
            (p, EditState::default())
        };
        let photos_cache = catalog.list_photos()?;
        let photo_hashes = photos_cache.iter().map(|p| p.file_hash).collect();

        Ok(Self {
            edit,
            image,
            photo_id: photo.id,
            current_flag: photo.flag,
            catalog,
            photos_cache,
            photo_hashes,
            dirty_since: None,
            new_preset_name: String::new(),
            folder_filter: None,
            collection_filter: CollectionFilter::All,
            watch_folder: None,
            last_watch_scan: None,
            canvas_zoom: 1.0,
            canvas_pan: egui::Vec2::ZERO,
        })
    }

    pub fn switch_to_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        self.dirty_since = Some(Instant::now() - DEBOUNCE);
        self.flush_if_due();

        let bytes = std::fs::read(&path)?;
        let image = chalkraw_io::decode_image_from_bytes(&path, &bytes)
            .map_err(|e| anyhow::anyhow!("decode {path:?}: {e}"))?;
        let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();

        let (photo, edit) = match self
            .photos_cache
            .iter()
            .find(|p| p.file_hash == hash)
            .cloned()
        {
            Some(p) => {
                let e = self.catalog.get_edit(p.id)?;
                (p, e)
            }
            None => {
                let p = Photo::new(path.clone(), hash, image.width, image.height, image.format);
                self.catalog.insert_photo(&p)?;
                self.photos_cache.push(p.clone());
                self.photo_hashes.insert(hash);
                (p, EditState::default())
            }
        };

        self.image = image;
        self.photo_id = photo.id;
        self.current_flag = photo.flag;
        self.edit = edit;
        self.dirty_since = None;
        self.canvas_zoom = 1.0;
        self.canvas_pan = egui::Vec2::ZERO;
        Ok(())
    }

    pub fn switch_to_photo_with_image(
        &mut self,
        photo_id: PhotoId,
        image: LinearImage,
    ) -> anyhow::Result<()> {
        self.dirty_since = Some(Instant::now() - DEBOUNCE);
        self.flush_if_due();

        let photo = self
            .photos_cache
            .iter()
            .find(|p| p.id == photo_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("photo {photo_id} is not in the catalog"))?;
        let edit = self.catalog.get_edit(photo.id)?;

        self.image = image;
        self.photo_id = photo.id;
        self.current_flag = photo.flag;
        self.edit = edit;
        self.dirty_since = None;
        self.canvas_zoom = 1.0;
        self.canvas_pan = egui::Vec2::ZERO;
        Ok(())
    }

    fn batch_items_from_photos(
        &self,
        photos: Vec<Photo>,
        source_label: &str,
    ) -> anyhow::Result<Vec<chalkraw_export::BatchItem>> {
        let photo_count = photos.len();
        let mut items = Vec::new();
        for p in photos {
            if p.original_path.as_os_str() == "<embedded>" {
                log::warn!(
                    "skip embedded fixture photo {}: cannot export embedded image",
                    p.id
                );
                continue;
            }
            if !p.original_path.exists() {
                log::warn!(
                    "skip {:?}: file not found at original_path",
                    p.original_path
                );
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
            "export source={source_label}, source has {} photos; selected {} for export",
            photo_count,
            items.len()
        );
        Ok(items)
    }

    pub fn import_candidates(
        &mut self,
        candidates: Vec<ImportCandidate>,
        summary: &mut ImportSummary,
    ) -> anyhow::Result<()> {
        let mut photos = Vec::new();
        for c in candidates {
            if self.photo_hashes.contains(&c.hash) {
                log::info!("skip {:?}: already imported", c.path);
                summary.duplicates += 1;
                continue;
            }
            self.photo_hashes.insert(c.hash);
            let mut p = Photo::new(c.path, c.hash, c.width, c.height, c.format);
            p.thumbnail = c.thumbnail;
            photos.push(p);
        }
        let inserted = photos.len();
        self.catalog.insert_photos(&photos)?;
        self.photos_cache.extend(photos);
        summary.inserted += inserted;
        Ok(())
    }

    pub fn remove_current_photo_from_catalog(&mut self) -> anyhow::Result<PhotoId> {
        let removed_id = self.photo_id;
        let removed_hash = self
            .photos_cache
            .iter()
            .find(|p| p.id == removed_id)
            .map(|p| p.file_hash)
            .ok_or_else(|| anyhow::anyhow!("current photo {removed_id} is not in the catalog"))?;

        if self.photos_cache.len() <= 1 {
            anyhow::bail!("cannot remove the only photo in the catalog");
        }

        let visible = self.visible_photos();
        let mut next_paths: Vec<PathBuf> = Vec::new();
        if visible.len() > 1 {
            let current_idx = visible.iter().position(|p| p.id == removed_id).unwrap_or(0);
            for offset in 1..visible.len() {
                let idx = (current_idx + offset) % visible.len();
                next_paths.push(visible[idx].original_path.clone());
            }
        }
        if next_paths.is_empty() {
            self.folder_filter = None;
            self.collection_filter = CollectionFilter::All;
            next_paths.extend(
                self.photos_cache
                    .iter()
                    .filter(|p| p.id != removed_id)
                    .map(|p| p.original_path.clone()),
            );
        }

        let mut last_error: Option<anyhow::Error> = None;
        for path in next_paths {
            match self.switch_to_path(path) {
                Ok(()) => {
                    self.catalog.remove_photo_with_edit(removed_id)?;
                    self.photos_cache.retain(|p| p.id != removed_id);
                    self.photo_hashes.remove(&removed_hash);
                    return Ok(removed_id);
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no replacement photo available")))
    }

    pub fn relink_current_photo(&mut self, new_path: PathBuf) -> anyhow::Result<PhotoId> {
        let bytes = std::fs::read(&new_path)?;
        let image = chalkraw_io::decode_image_from_bytes(&new_path, &bytes)
            .map_err(|e| anyhow::anyhow!("decode {new_path:?}: {e}"))?;
        let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();
        if self
            .photos_cache
            .iter()
            .any(|p| p.id != self.photo_id && p.file_hash == hash)
        {
            anyhow::bail!("replacement file is already imported");
        }

        let thumbnail = chalkraw_io::make_thumbnail(&image).unwrap_or_default();
        let old_hash = self
            .photos_cache
            .iter()
            .find(|p| p.id == self.photo_id)
            .map(|p| p.file_hash);
        let updated = self.catalog.update_photo_path(
            self.photo_id,
            chalkraw_catalog::PhotoPathUpdate {
                new_path,
                new_hash: hash,
                width: image.width,
                height: image.height,
                format: image.format,
                thumbnail,
            },
        )?;

        if let Some(old_hash) = old_hash {
            self.photo_hashes.remove(&old_hash);
        }
        self.photo_hashes.insert(hash);
        if let Some(photo) = self.photos_cache.iter_mut().find(|p| p.id == self.photo_id) {
            *photo = updated.clone();
        }
        self.image = image;
        self.current_flag = updated.flag;
        self.canvas_zoom = 1.0;
        self.canvas_pan = egui::Vec2::ZERO;
        Ok(updated.id)
    }

    pub fn set_current_flag(&mut self, flag: Flag) {
        if let Err(e) = self.catalog.update_flag(self.photo_id, flag) {
            log::warn!("set flag failed: {e}");
        } else {
            self.current_flag = flag;
            if let Some(photo) = self.photos_cache.iter_mut().find(|p| p.id == self.photo_id) {
                photo.flag = flag;
            }
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    pub fn flush_if_due(&mut self) {
        let due = self
            .dirty_since
            .map(|t| t.elapsed() >= DEBOUNCE)
            .unwrap_or(false);
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
        let preset = self
            .catalog
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

    pub fn take_due_watch_folder_scan(&mut self) -> Option<PathBuf> {
        let dir = self.watch_folder.clone()?;
        let now = std::time::Instant::now();
        if let Some(last) = self.last_watch_scan {
            if now.duration_since(last).as_secs() < 5 {
                return None;
            }
        }
        self.last_watch_scan = Some(now);
        Some(dir)
    }

    pub fn visible_photos(&self) -> Vec<Photo> {
        let mut photos = self.photos_cache.clone();
        if let Some(filter) = &self.folder_filter {
            photos.retain(|p| p.original_path.parent() == Some(filter.as_path()));
        }
        match self.collection_filter {
            CollectionFilter::All => {}
            CollectionFilter::Picks => photos.retain(|p| p.flag == Flag::Pick),
            CollectionFilter::Rejected => photos.retain(|p| p.flag == Flag::Reject),
        }
        photos
    }

    pub fn has_active_filter(&self) -> bool {
        self.folder_filter.is_some() || self.collection_filter != CollectionFilter::All
    }

    pub fn clear_filters(&mut self) {
        self.folder_filter = None;
        self.collection_filter = CollectionFilter::All;
    }

    pub fn filter_summary(&self) -> Option<String> {
        if !self.has_active_filter() {
            return None;
        }
        let mut parts = Vec::new();
        if let Some(folder) = &self.folder_filter {
            let label = folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.display().to_string());
            parts.push(format!("Folder: {label}"));
        }
        match self.collection_filter {
            CollectionFilter::All => {}
            CollectionFilter::Picks => parts.push("Collection: Picks".to_string()),
            CollectionFilter::Rejected => parts.push("Collection: Rejected".to_string()),
        }
        Some(parts.join(" | "))
    }

    pub fn photo_at_offset(&self, delta: i32) -> Option<Photo> {
        let photos = self.visible_photos();
        if photos.is_empty() {
            return None;
        }
        let current_idx = photos
            .iter()
            .position(|p| p.id == self.photo_id)
            .unwrap_or(0);
        let new_idx = ((current_idx as i32 + delta).rem_euclid(photos.len() as i32)) as usize;
        photos.get(new_idx).cloned()
    }

    pub fn neighbor_photos(&self) -> Vec<Photo> {
        let mut neighbors = Vec::new();
        for delta in [1, -1] {
            if let Some(photo) = self.photo_at_offset(delta) {
                if photo.id != self.photo_id && !neighbors.iter().any(|p: &Photo| p.id == photo.id)
                {
                    neighbors.push(photo);
                }
            }
        }
        neighbors
    }

    /// Gather batch items from the catalog. If `only_picks` is true, filters to
    /// photos flagged as Pick. Skips photos whose original_path doesn't exist.
    pub fn collect_batch_items(
        &self,
        only_picks: bool,
    ) -> anyhow::Result<Vec<chalkraw_export::BatchItem>> {
        let photos: Vec<Photo> = self
            .photos_cache
            .iter()
            .filter(|p| !only_picks || p.flag == Flag::Pick)
            .cloned()
            .collect();
        let source_label = if only_picks { "picks" } else { "all" };
        self.batch_items_from_photos(photos, source_label)
    }
}

#[derive(Debug, Clone)]
pub struct ImportCandidate {
    path: PathBuf,
    hash: [u8; 32],
    width: u32,
    height: u32,
    format: ImageFormat,
    thumbnail: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ImportFailure {
    path: PathBuf,
    reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    scanned: usize,
    decoded: usize,
    inserted: usize,
    duplicates: usize,
    failed: usize,
    failures: Vec<ImportFailure>,
}

impl ImportSummary {
    fn record_failure(&mut self, path: PathBuf, reason: impl Into<String>) {
        self.failed += 1;
        if self.failures.len() < 5 {
            self.failures.push(ImportFailure {
                path,
                reason: reason.into(),
            });
        }
    }

    fn message(&self) -> String {
        let mut message = format!(
            "Import complete: scanned {}, decoded {}, inserted {}, duplicates {}, failed {}",
            self.scanned, self.decoded, self.inserted, self.duplicates, self.failed
        );
        if !self.failures.is_empty() {
            let failures = self
                .failures
                .iter()
                .map(|failure| {
                    let name = failure
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| failure.path.display().to_string());
                    format!("{name}: {}", failure.reason)
                })
                .collect::<Vec<_>>()
                .join("; ");
            message.push_str(" | ");
            message.push_str(&failures);
            if self.failed > self.failures.len() {
                message.push_str(&format!("; +{} more", self.failed - self.failures.len()));
            }
        }
        message
    }
}

struct ImportProgress {
    current: usize,
    total: usize,
    name: String,
    done: bool,
    candidates: Vec<ImportCandidate>,
    summary: ImportSummary,
    error: Option<String>,
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

/// State for the watermark preset editor sub-dialog.
struct WatermarkEditorState {
    open: bool,
    /// Working copy of the preset being edited or created.
    current: chalkraw_core::WatermarkPreset,
    /// True when creating a brand-new preset (vs editing an existing one).
    /// Used in Phase 5B to decide whether to show "Create" or "Update" in the
    /// title bar. Retained in the data model to avoid a breaking change later.
    #[allow(dead_code)]
    is_new: bool,
    /// Which layer row is expanded (0-based index), None = all collapsed.
    expanded_layer: Option<usize>,
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
            rotation_deg: 0.0,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportSource {
    Current,
    Workbench,
    Picks,
    All,
}

struct ExportDialogState {
    // Format
    format_index: usize, // 0=JPEG, 1=PNG, 2=TIFF
    quality: u8,
    // Resize
    resize_long_edge: bool,
    long_edge: u32,
    // Batch source
    source: ExportSource,
    // Output
    output_dir: Option<PathBuf>,
    name_pattern: String,
    // Watermark — quick inline stamp (legacy single-layer)
    watermark: WatermarkDialogState,
    // Watermark preset selection
    watermark_preset_id: Option<chalkraw_core::WatermarkId>,
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
            source: ExportSource::Workbench,
            output_dir: None,
            name_pattern: "{name}_edited".to_string(),
            watermark: WatermarkDialogState::default(),
            watermark_preset_id: None,
            batch_progress: None,
            completion_message: None,
        }
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct ChalkrawApp {
    state: AppState,
    mode: WorkspaceMode,
    gpu: Option<Arc<CanvasGpu>>,
    gpu_cache: HashMap<PhotoId, Arc<CanvasGpu>>,
    gpu_cache_order: VecDeque<PhotoId>,
    decoded_cache: HashMap<PhotoId, LinearImage>,
    decoded_cache_order: VecDeque<PhotoId>,
    decode_prefetch_tx: Sender<(PhotoId, Result<LinearImage, String>)>,
    decode_prefetch_rx: Receiver<(PhotoId, Result<LinearImage, String>)>,
    pending_decode_prefetch: HashSet<PhotoId>,
    workbench_photos: HashSet<PhotoId>,
    thumb_textures: HashMap<PhotoId, egui::TextureHandle>,
    export_dialog: Option<ExportDialogState>,
    import_progress: Option<Arc<Mutex<ImportProgress>>>,
    import_message: Option<String>,
    watermark_editor: Option<WatermarkEditorState>,
    /// Cached egui textures for image layers shown in the watermark preview overlay.
    /// Keyed by the absolute PNG path; cleared when the editor opens or closes so
    /// stale entries don't accumulate across sessions.
    watermark_preview_textures: HashMap<PathBuf, egui::TextureHandle>,
    /// Guards the one-time INFO log that records which GPU adapter egui-wgpu picked.
    gpu_adapter_logged: bool,
}

impl ChalkrawApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        install_ui_fonts(&cc.egui_ctx);
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
        let mut decoded_cache = HashMap::new();
        let mut decoded_cache_order = VecDeque::new();
        decoded_cache.insert(state.photo_id, state.image.clone());
        decoded_cache_order.push_back(state.photo_id);
        let (decode_prefetch_tx, decode_prefetch_rx) = mpsc::channel();
        Ok(Self {
            state,
            mode: WorkspaceMode::Develop,
            gpu: None,
            gpu_cache: HashMap::new(),
            gpu_cache_order: VecDeque::new(),
            decoded_cache,
            decoded_cache_order,
            decode_prefetch_tx,
            decode_prefetch_rx,
            pending_decode_prefetch: HashSet::new(),
            workbench_photos: HashSet::new(),
            thumb_textures: HashMap::new(),
            export_dialog: None,
            import_progress: None,
            import_message: None,
            watermark_editor: None,
            watermark_preview_textures: HashMap::new(),
            gpu_adapter_logged: false,
        })
    }

    fn start_import_paths(&mut self, paths: Vec<PathBuf>, label: &'static str) {
        if self.import_progress.is_some() {
            log::warn!("import already running; ignoring new {label} request");
            return;
        }
        let progress = Arc::new(Mutex::new(ImportProgress {
            current: 0,
            total: paths.len(),
            name: String::new(),
            done: false,
            candidates: Vec::new(),
            summary: ImportSummary::default(),
            error: None,
        }));
        let progress_thread = progress.clone();
        let known_hashes = self.state.photo_hashes.clone();
        std::thread::spawn(move || {
            let (candidates, summary) = process_import_paths(paths, known_hashes, &progress_thread);
            let mut p = progress_thread.lock().unwrap();
            p.done = true;
            p.candidates = candidates;
            p.summary = summary;
        });
        self.import_progress = Some(progress);
        self.import_message = Some(format!("{label}: queued import"));
    }

    fn start_import_folder(&mut self, dir: PathBuf, label: &'static str) {
        if self.import_progress.is_some() {
            log::warn!("import already running; ignoring new {label} request");
            return;
        }
        let progress = Arc::new(Mutex::new(ImportProgress {
            current: 0,
            total: 0,
            name: dir.display().to_string(),
            done: false,
            candidates: Vec::new(),
            summary: ImportSummary::default(),
            error: None,
        }));
        let progress_thread = progress.clone();
        let known_hashes = self.state.photo_hashes.clone();
        std::thread::spawn(move || {
            let extensions = [
                "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef", "arw", "raf", "pef",
                "orf",
            ];
            let mut paths = Vec::new();
            walk_dir(&dir, &extensions, &mut paths);
            {
                let mut p = progress_thread.lock().unwrap();
                p.total = paths.len();
                p.current = 0;
            }
            let (candidates, summary) = process_import_paths(paths, known_hashes, &progress_thread);
            let mut p = progress_thread.lock().unwrap();
            p.done = true;
            p.candidates = candidates;
            p.summary = summary;
        });
        self.import_progress = Some(progress);
        self.import_message = Some(format!("{label}: scanning folder"));
    }

    fn finish_import_if_ready(&mut self) {
        let Some(progress_arc) = self.import_progress.as_ref() else {
            return;
        };
        let done = progress_arc.lock().unwrap().done;
        if !done {
            return;
        }
        let progress_arc = self.import_progress.take().unwrap();
        let (candidates, mut summary, error) = {
            let mut p = progress_arc.lock().unwrap();
            (
                std::mem::take(&mut p.candidates),
                std::mem::take(&mut p.summary),
                p.error.clone(),
            )
        };
        if let Some(error) = error {
            self.import_message = Some(format!("Import failed: {error}"));
            return;
        }
        match self.state.import_candidates(candidates, &mut summary) {
            Ok(()) => {
                self.thumb_textures.clear();
                self.import_message = Some(summary.message());
            }
            Err(e) => {
                self.import_message = Some(format!("Import failed: {e}"));
            }
        }
    }

    fn ensure_gpu(&mut self, frame: &eframe::Frame) {
        if self.gpu.is_some() {
            return;
        }
        if let Some(gpu) = self.gpu_cache.get(&self.state.photo_id).cloned() {
            Self::refresh_gpu_for_edit(&gpu, &self.state.edit);
            self.touch_gpu_cache(self.state.photo_id);
            self.gpu = Some(gpu);
            return;
        }
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };
        if !self.gpu_adapter_logged {
            let info = render_state.adapter.get_info();
            log::info!(
                "egui canvas GPU: {} ({:?}, backend {:?}, vendor 0x{:04X})",
                info.name,
                info.device_type,
                info.backend,
                info.vendor
            );
            self.gpu_adapter_logged = true;
        }
        let rd = RenderDevice::from_shared(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        );
        let format = render_state.target_format;
        log::info!("wgpu surface target_format = {format:?}");
        let gpu = CanvasGpu::new(&rd, &self.state.image, format);
        Self::refresh_gpu_for_edit(&gpu, &self.state.edit);
        let gpu = Arc::new(gpu);
        self.cache_gpu(self.state.photo_id, gpu.clone());
        self.gpu = Some(gpu);
    }

    fn refresh_gpu_for_edit(gpu: &CanvasGpu, edit: &EditState) {
        gpu.update(edit);
        gpu.upload_tone_curve(&edit.tone_curve.rgb.0);
        let nr_amount =
            (edit.detail.noise_reduction.luminance + edit.detail.noise_reduction.color) / 2.0;
        gpu.run_blurs(16.0, edit.detail.sharpening.radius, 5.0, nr_amount);
    }

    fn touch_decoded_cache(&mut self, photo_id: PhotoId) {
        self.decoded_cache_order.retain(|id| *id != photo_id);
        self.decoded_cache_order.push_back(photo_id);
    }

    fn cache_decoded_image(&mut self, photo_id: PhotoId, image: LinearImage) {
        self.decoded_cache.insert(photo_id, image);
        self.touch_decoded_cache(photo_id);
        while self.decoded_cache.len() > DECODED_CACHE_CAPACITY {
            let Some(oldest) = self.decoded_cache_order.pop_front() else {
                break;
            };
            if oldest == self.state.photo_id {
                self.decoded_cache_order.push_back(oldest);
                continue;
            }
            self.decoded_cache.remove(&oldest);
        }
    }

    fn touch_gpu_cache(&mut self, photo_id: PhotoId) {
        self.gpu_cache_order.retain(|id| *id != photo_id);
        self.gpu_cache_order.push_back(photo_id);
    }

    fn cache_gpu(&mut self, photo_id: PhotoId, gpu: Arc<CanvasGpu>) {
        self.gpu_cache.insert(photo_id, gpu);
        self.touch_gpu_cache(photo_id);
        while self.gpu_cache.len() > GPU_CACHE_CAPACITY {
            let Some(oldest) = self.gpu_cache_order.pop_front() else {
                break;
            };
            if oldest == self.state.photo_id {
                self.gpu_cache_order.push_back(oldest);
                continue;
            }
            self.gpu_cache.remove(&oldest);
        }
    }

    fn open_photo_path(&mut self, path: PathBuf) -> anyhow::Result<()> {
        self.state.switch_to_path(path)?;
        self.cache_decoded_image(self.state.photo_id, self.state.image.clone());
        self.gpu = self.gpu_cache.get(&self.state.photo_id).cloned();
        if let Some(gpu) = self.gpu.as_ref() {
            Self::refresh_gpu_for_edit(gpu, &self.state.edit);
        }
        self.schedule_neighbor_decode_prefetch();
        Ok(())
    }

    fn switch_to_photo_id(&mut self, photo_id: PhotoId) -> anyhow::Result<()> {
        let image = if let Some(image) = self.decoded_cache.get(&photo_id).cloned() {
            self.touch_decoded_cache(photo_id);
            image
        } else {
            let photo = self
                .state
                .photos_cache
                .iter()
                .find(|p| p.id == photo_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("photo {photo_id} is not in the catalog"))?;
            let image = decode_image(&photo.original_path)
                .map_err(|e| anyhow::anyhow!("decode {:?}: {e}", photo.original_path))?;
            self.cache_decoded_image(photo_id, image.clone());
            image
        };
        self.state.switch_to_photo_with_image(photo_id, image)?;
        self.gpu = self.gpu_cache.get(&photo_id).cloned();
        if let Some(gpu) = self.gpu.as_ref() {
            Self::refresh_gpu_for_edit(gpu, &self.state.edit);
        }
        self.schedule_neighbor_decode_prefetch();
        Ok(())
    }

    fn remove_current_photo_from_catalog(&mut self) {
        match self.state.remove_current_photo_from_catalog() {
            Ok(id) => {
                self.gpu = self.gpu_cache.get(&self.state.photo_id).cloned();
                self.gpu_cache.remove(&id);
                self.gpu_cache_order.retain(|cached_id| *cached_id != id);
                self.decoded_cache.remove(&id);
                self.decoded_cache_order
                    .retain(|cached_id| *cached_id != id);
                self.pending_decode_prefetch.remove(&id);
                self.workbench_photos.remove(&id);
                self.thumb_textures.remove(&id);
                self.cache_decoded_image(self.state.photo_id, self.state.image.clone());
                if let Some(gpu) = self.gpu.as_ref() {
                    Self::refresh_gpu_for_edit(gpu, &self.state.edit);
                }
                self.import_message =
                    Some("Removed current photo from catalog; original file kept".to_string());
                self.schedule_neighbor_decode_prefetch();
            }
            Err(e) => {
                self.import_message = Some(format!("Remove failed: {e}"));
                log::warn!("remove current photo failed: {e}");
            }
        }
    }

    fn collect_export_items(
        &self,
        source: ExportSource,
    ) -> anyhow::Result<Vec<chalkraw_export::BatchItem>> {
        match source {
            ExportSource::Current => {
                let photos: Vec<Photo> = self
                    .state
                    .photos_cache
                    .iter()
                    .filter(|p| p.id == self.state.photo_id)
                    .cloned()
                    .collect();
                self.state.batch_items_from_photos(photos, "current")
            }
            ExportSource::Workbench => {
                let photos: Vec<Photo> = self
                    .state
                    .photos_cache
                    .iter()
                    .filter(|p| self.workbench_photos.contains(&p.id))
                    .cloned()
                    .collect();
                self.state.batch_items_from_photos(photos, "workbench")
            }
            ExportSource::Picks => self.state.collect_batch_items(true),
            ExportSource::All => self.state.collect_batch_items(false),
        }
    }

    fn export_source_count(&self, source: ExportSource) -> usize {
        match source {
            ExportSource::Current => usize::from(!self.state.photos_cache.is_empty()),
            ExportSource::Workbench => self.workbench_photos.len(),
            ExportSource::Picks => self
                .state
                .photos_cache
                .iter()
                .filter(|p| p.flag == Flag::Pick)
                .count(),
            ExportSource::All => self.state.photos_cache.len(),
        }
    }

    fn collect_decode_prefetch_results(&mut self) {
        while let Ok((photo_id, result)) = self.decode_prefetch_rx.try_recv() {
            self.pending_decode_prefetch.remove(&photo_id);
            match result {
                Ok(image) => self.cache_decoded_image(photo_id, image),
                Err(e) => log::debug!("decode prefetch skipped for {photo_id}: {e}"),
            }
        }
    }

    fn schedule_neighbor_decode_prefetch(&mut self) {
        for photo in self.state.neighbor_photos() {
            if self.decoded_cache.contains_key(&photo.id)
                || self.pending_decode_prefetch.contains(&photo.id)
                || photo.original_path.as_os_str() == "<embedded>"
                || !photo.original_path.exists()
            {
                continue;
            }
            let tx = self.decode_prefetch_tx.clone();
            let photo_id = photo.id;
            let path = photo.original_path.clone();
            self.pending_decode_prefetch.insert(photo_id);
            std::thread::spawn(move || {
                let result = decode_image(&path).map_err(|e| format!("decode {path:?}: {e}"));
                let _ = tx.send((photo_id, result));
            });
        }
    }

    fn prefetch_one_neighbor_gpu(&mut self, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else {
            return;
        };
        let Some(photo) = self
            .state
            .neighbor_photos()
            .into_iter()
            .find(|photo| !self.gpu_cache.contains_key(&photo.id))
        else {
            return;
        };
        let Some(image) = self.decoded_cache.get(&photo.id) else {
            return;
        };
        let rd = RenderDevice::from_shared(
            Arc::new(render_state.device.clone()),
            Arc::new(render_state.queue.clone()),
        );
        let gpu = Arc::new(CanvasGpu::new(&rd, image, render_state.target_format));
        self.cache_gpu(photo.id, gpu);
    }

    fn show_library_grid(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Library");
            ui.separator();
            if ui.button("Import Photos...").clicked() {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter(
                        "Images",
                        &[
                            "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef", "arw", "raf",
                            "pef", "orf",
                        ],
                    )
                    .pick_files()
                {
                    self.start_import_paths(paths, "Import photos");
                }
            }
            if ui.button("Import Folder...").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.start_import_folder(dir, "Import folder");
                }
            }
            if ui.button("Export...").clicked() {
                self.export_dialog = Some(ExportDialogState::default());
            }
            ui.separator();
            ui.add_enabled_ui(self.state.photos_cache.len() > 1, |ui| {
                if ui.button("Remove Current From Catalog").clicked() {
                    self.remove_current_photo_from_catalog();
                }
            });
            if ui.button("Develop").clicked() {
                self.mode = WorkspaceMode::Develop;
            }
        });
        ui.horizontal(|ui| {
            ui.label(format!("Workbench: {} photos", self.workbench_photos.len()));
            if ui.button("Add Current").clicked() {
                self.workbench_photos.insert(self.state.photo_id);
            }
            if ui.button("Clear Workbench").clicked() {
                self.workbench_photos.clear();
            }
        });
        ui.separator();
        if let Some(summary) = self.state.filter_summary() {
            ui.horizontal(|ui| {
                ui.label(format!("Filter: {summary}"));
                if ui.button("Clear Filters").clicked() {
                    self.state.clear_filters();
                }
            });
        }

        let photos = self.state.visible_photos();
        if photos.is_empty() {
            if self.state.folder_filter.is_some() {
                ui.label("No photos in this folder.");
            } else if self.state.collection_filter != CollectionFilter::All {
                ui.label("No photos in this collection.");
            } else {
                ui.label("No photos yet. File -> Import Photos / Import Folder...");
            }
            return;
        }

        let current_id = self.state.photo_id;
        let tiles: Vec<(PhotoId, String, egui::TextureHandle, Flag)> = photos
            .iter()
            .map(|photo| {
                let name = photo
                    .original_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| photo.original_path.display().to_string());
                let tex = self.ensure_thumb(ctx, photo);
                (photo.id, name, tex, photo.flag)
            })
            .collect();

        let mut clicked: Option<PhotoId> = None;
        let mut open_develop: Option<PhotoId> = None;
        let mut toggle_workbench: Option<PhotoId> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (photo_id, name, tex, flag) in &tiles {
                    ui.vertical(|ui| {
                        let img = egui::Image::new(tex).max_width(180.0).max_height(130.0);
                        let response = ui.add(img.sense(egui::Sense::click()));
                        let flag_color = match flag {
                            Flag::Pick => Some(egui::Color32::from_rgb(80, 200, 80)),
                            Flag::Reject => Some(egui::Color32::from_rgb(220, 80, 80)),
                            Flag::None => None,
                        };
                        if let Some(c) = flag_color {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.0,
                                egui::Stroke::new(2.0, c),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if *photo_id == current_id {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.0,
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 200, 60)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if response.double_clicked() {
                            open_develop = Some(*photo_id);
                        } else if response.clicked() {
                            clicked = Some(*photo_id);
                        }
                        ui.add_sized(
                            [180.0, 18.0],
                            egui::Label::new(egui::RichText::new(name.as_str()).small()),
                        );
                        let mut on_workbench = self.workbench_photos.contains(photo_id);
                        if ui.checkbox(&mut on_workbench, "Workbench").changed() {
                            toggle_workbench = Some(*photo_id);
                        }
                    });
                    ui.add_space(8.0);
                }
            });
        });

        if let Some(photo_id) = toggle_workbench {
            if !self.workbench_photos.remove(&photo_id) {
                self.workbench_photos.insert(photo_id);
            }
        }

        let open_develop_requested = open_develop.is_some();
        let target = open_develop.or(clicked);
        if let Some(photo_id) = target {
            if let Err(e) = self.switch_to_photo_id(photo_id) {
                log::warn!("library selection failed: {e}");
            } else if open_develop_requested {
                self.mode = WorkspaceMode::Develop;
            }
        }
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

fn install_ui_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto-sans-thai".to_string(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../../assets/fonts/NotoSansThai-Regular.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto-sans-thai".to_string());
    }
    ctx.set_fonts(fonts);
}

// ── Phase 5C: watermark preview overlay helpers ───────────────────────────────

/// Compute the top-left screen position for a watermark element of size
/// `(w, h)` relative to `image_rect`, honouring the `anchor` and a screen-space
/// `margin`.
fn anchor_pos(
    image_rect: egui::Rect,
    w: f32,
    h: f32,
    anchor: chalkraw_core::WatermarkAnchor,
    margin: f32,
) -> egui::Pos2 {
    use chalkraw_core::WatermarkAnchor::*;
    let (x, y) = match anchor {
        TopLeft => (image_rect.min.x + margin, image_rect.min.y + margin),
        TopCenter => (image_rect.center().x - w / 2.0, image_rect.min.y + margin),
        TopRight => (image_rect.max.x - w - margin, image_rect.min.y + margin),
        CenterLeft => (image_rect.min.x + margin, image_rect.center().y - h / 2.0),
        Center => (
            image_rect.center().x - w / 2.0,
            image_rect.center().y - h / 2.0,
        ),
        CenterRight => (
            image_rect.max.x - w - margin,
            image_rect.center().y - h / 2.0,
        ),
        BottomLeft => (image_rect.min.x + margin, image_rect.max.y - h - margin),
        BottomCenter => (
            image_rect.center().x - w / 2.0,
            image_rect.max.y - h - margin,
        ),
        BottomRight => (image_rect.max.x - w - margin, image_rect.max.y - h - margin),
    };
    egui::Pos2::new(x, y)
}

/// Draw all layers of `preset` on top of the canvas area defined by `image_rect`.
///
/// This is an **approximate** preview only — egui's font rasteriser and
/// image-scaling differ from the ab_glyph / image-crate composition done at
/// export time. Position, anchor, size %, and opacity are accurate; exact pixel
/// values are not.
fn draw_watermark_overlay(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    image_rect: egui::Rect,
    image_w: u32,
    image_h: u32,
    preset: &chalkraw_core::WatermarkPreset,
    image_layer_textures: &mut HashMap<PathBuf, egui::TextureHandle>,
) {
    let long_edge_screen = image_rect.width().max(image_rect.height());
    let long_edge_image = image_w.max(image_h) as f32;
    // How many screen pixels correspond to one source-image pixel.
    let scale = long_edge_screen / long_edge_image;

    for layer in &preset.layers {
        match layer {
            chalkraw_core::WatermarkLayer::Image(img_layer) => {
                // Load (and cache) the PNG as an egui texture if needed.
                if !image_layer_textures.contains_key(&img_layer.png_path)
                    && img_layer.png_path.exists()
                {
                    if let Ok(bytes) = std::fs::read(&img_layer.png_path) {
                        if let Ok(decoded) = image::load_from_memory(&bytes) {
                            let rgba = decoded.to_rgba8();
                            let (w, h) = rgba.dimensions();
                            let ci = egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                rgba.as_raw(),
                            );
                            let tex = ctx.load_texture(
                                format!("wm-{}", img_layer.png_path.display()),
                                ci,
                                egui::TextureOptions::LINEAR,
                            );
                            image_layer_textures.insert(img_layer.png_path.clone(), tex);
                        }
                    }
                }
                let Some(tex) = image_layer_textures.get(&img_layer.png_path) else {
                    continue;
                };

                let target_long_screen = img_layer.size_pct / 100.0 * long_edge_screen;
                let aspect = tex.aspect_ratio(); // w / h
                let (w_screen, h_screen) = if aspect >= 1.0 {
                    (target_long_screen, target_long_screen / aspect)
                } else {
                    (target_long_screen * aspect, target_long_screen)
                };
                let margin_screen = img_layer.margin_pct / 100.0 * long_edge_screen;
                let pos = anchor_pos(
                    image_rect,
                    w_screen,
                    h_screen,
                    img_layer.anchor,
                    margin_screen,
                );
                let layer_rect = egui::Rect::from_min_size(pos, egui::vec2(w_screen, h_screen));

                let alpha = (img_layer.opacity.clamp(0.0, 1.0) * 255.0) as u8;
                let tint = egui::Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha);
                let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                ui.painter().image(tex.id(), layer_rect, uv, tint);
            }

            chalkraw_core::WatermarkLayer::Text(text_layer) => {
                let px_size_image = text_layer.font_size_pct / 100.0 * long_edge_image;
                let px_size_screen = px_size_image * scale;
                let font_id = egui::FontId::proportional(px_size_screen);
                let alpha = (text_layer.color.a as f32 * text_layer.opacity.clamp(0.0, 1.0)) as u8;
                let text_color = egui::Color32::from_rgba_unmultiplied(
                    text_layer.color.r,
                    text_layer.color.g,
                    text_layer.color.b,
                    alpha,
                );
                // Measure the text so we can honour the anchor correctly.
                let galley =
                    ui.painter()
                        .layout_no_wrap(text_layer.text.clone(), font_id, text_color);
                let (w, h) = (galley.size().x, galley.size().y);
                let margin_screen = text_layer.margin_pct / 100.0 * long_edge_screen;
                let pos = anchor_pos(image_rect, w, h, text_layer.anchor, margin_screen);
                ui.painter().galley(pos, galley, text_color);
            }
        }
    }
}

// ── Recursive directory walker ────────────────────────────────────────────────

pub(crate) fn walk_dir(dir: &std::path::Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("cannot read {dir:?}: {e}");
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, extensions, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lc = ext.to_lowercase();
            if extensions.iter().any(|e| *e == ext_lc) {
                out.push(path);
            }
        }
    }
}

enum ImportPathResult {
    Candidate(ImportCandidate),
    Duplicate,
    Failure { path: PathBuf, reason: String },
}

fn process_import_paths(
    paths: Vec<PathBuf>,
    known_hashes: HashSet<[u8; 32]>,
    progress: &Arc<Mutex<ImportProgress>>,
) -> (Vec<ImportCandidate>, ImportSummary) {
    let total = paths.len();
    let mut summary = ImportSummary {
        scanned: total,
        ..ImportSummary::default()
    };
    let known_hashes = Arc::new(Mutex::new(known_hashes));
    let started = AtomicUsize::new(0);
    let results: Vec<ImportPathResult> = paths
        .into_par_iter()
        .map(|path| {
            let idx = started.fetch_add(1, Ordering::Relaxed) + 1;
            process_import_path(path, idx, total, &known_hashes, progress)
        })
        .collect();

    let mut candidates = Vec::new();
    for result in results {
        match result {
            ImportPathResult::Candidate(candidate) => {
                summary.decoded += 1;
                candidates.push(candidate);
            }
            ImportPathResult::Duplicate => {
                summary.duplicates += 1;
            }
            ImportPathResult::Failure { path, reason } => {
                summary.record_failure(path, reason);
            }
        }
    }
    (candidates, summary)
}

fn process_import_path(
    path: PathBuf,
    idx: usize,
    total: usize,
    known_hashes: &Arc<Mutex<HashSet<[u8; 32]>>>,
    progress: &Arc<Mutex<ImportProgress>>,
) -> ImportPathResult {
    {
        let mut p = progress.lock().unwrap();
        p.current = idx;
        p.total = total;
        p.name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
    }

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("skip {path:?}: {e}");
            return ImportPathResult::Failure {
                path,
                reason: e.to_string(),
            };
        }
    };
    let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();
    {
        let mut known_hashes = known_hashes.lock().unwrap();
        if !known_hashes.insert(hash) {
            log::info!("skip {path:?}: already imported");
            return ImportPathResult::Duplicate;
        }
    }

    let img = match chalkraw_io::decode_image_from_bytes(&path, &bytes) {
        Ok(i) => i,
        Err(e) => {
            log::warn!("skip {path:?}: {e}");
            return ImportPathResult::Failure {
                path,
                reason: e.to_string(),
            };
        }
    };
    let thumbnail = chalkraw_io::make_thumbnail(&img).unwrap_or_default();
    ImportPathResult::Candidate(ImportCandidate {
        path,
        hash,
        width: img.width,
        height: img.height,
        format: img.format,
        thumbnail,
    })
}

impl eframe::App for ChalkrawApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.collect_decode_prefetch_results();
        self.finish_import_if_ready();
        if self.import_progress.is_none() {
            if let Some(dir) = self.state.take_due_watch_folder_scan() {
                self.start_import_folder(dir, "Watch folder");
            }
        }
        self.ensure_gpu(frame);
        self.schedule_neighbor_decode_prefetch();
        self.prefetch_one_neighbor_gpu(frame);

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
            } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::OpenBracket) {
                nav = Some(-1);
            }
        });
        if let Some(f) = to_set {
            self.state.set_current_flag(f);
        }
        if let Some(delta) = nav {
            if let Some(photo) = self.state.photo_at_offset(delta) {
                if let Err(e) = self.switch_to_photo_id(photo.id) {
                    log::warn!("navigate failed: {e}");
                }
            }
        }

        let mut edit_change = EditChange::default();
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.selectable_value(&mut self.mode, WorkspaceMode::Library, "Library");
                ui.selectable_value(&mut self.mode, WorkspaceMode::Develop, "Develop");
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.button("Open Photo…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &[
                                    "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef",
                                    "arw", "raf", "pef", "orf",
                                ],
                            )
                            .pick_file()
                        {
                            if let Err(e) = self.open_photo_path(path) {
                                log::warn!("open photo failed: {e}");
                            } else {
                                self.thumb_textures.clear();
                            }
                        }
                    }
                    if ui.button("Import Photos…").clicked() {
                        ui.close();
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &[
                                    "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef",
                                    "arw", "raf", "pef", "orf",
                                ],
                            )
                            .pick_files()
                        {
                            self.start_import_paths(paths, "Import photos");
                        }
                    }
                    if ui.button("Import Folder…").clicked() {
                        ui.close();
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.start_import_folder(dir, "Import folder");
                        }
                    }
                    if ui.button("Watch Folder…").clicked() {
                        ui.close();
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.state.watch_folder = Some(dir);
                            self.state.last_watch_scan = None; // force scan on next frame
                            log::info!("watching folder: {:?}", self.state.watch_folder);
                        }
                    }
                    if self.state.watch_folder.is_some() && ui.button("Stop Watching").clicked() {
                        ui.close();
                        self.state.watch_folder = None;
                    }
                    if ui.button("Batch Export…").clicked() {
                        ui.close();
                        self.export_dialog = Some(ExportDialogState::default());
                    }
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Library", |ui| {
                    if ui.button("Show Library Workspace").clicked() {
                        ui.close();
                        self.mode = WorkspaceMode::Library;
                    }
                    if ui.button("Import Photos…").clicked() {
                        ui.close();
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &[
                                    "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef",
                                    "arw", "raf", "pef", "orf",
                                ],
                            )
                            .pick_files()
                        {
                            self.start_import_paths(paths, "Import photos");
                        }
                    }
                    if ui.button("Import Folder…").clicked() {
                        ui.close();
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.start_import_folder(dir, "Import folder");
                        }
                    }
                    ui.separator();
                    let current_path = self
                        .state
                        .photos_cache
                        .iter()
                        .find(|p| p.id == self.state.photo_id)
                        .map(|p| p.original_path.clone());
                    if let Some(path) = &current_path {
                        if path.as_os_str() != "<embedded>" && !path.exists() {
                            ui.colored_label(
                                egui::Color32::YELLOW,
                                format!("Missing original: {}", path.display()),
                            );
                        }
                    }
                    if ui.button("Relink Current Photo…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Images",
                                &[
                                    "jpg", "jpeg", "png", "tif", "tiff", "cr2", "cr3", "nef",
                                    "arw", "raf", "pef", "orf",
                                ],
                            )
                            .pick_file()
                        {
                            match self.state.relink_current_photo(path) {
                                Ok(id) => {
                                    self.gpu = None;
                                    self.gpu_cache.remove(&id);
                                    self.gpu_cache_order.retain(|cached_id| *cached_id != id);
                                    self.decoded_cache.remove(&id);
                                    self.decoded_cache_order
                                        .retain(|cached_id| *cached_id != id);
                                    self.thumb_textures.remove(&id);
                                    self.cache_decoded_image(id, self.state.image.clone());
                                    self.import_message =
                                        Some("Relinked current photo".to_string());
                                }
                                Err(e) => {
                                    self.import_message = Some(format!("Relink failed: {e}"));
                                    log::warn!("relink current photo failed: {e}");
                                }
                            }
                        }
                    }
                    ui.add_enabled_ui(self.state.photos_cache.len() > 1, |ui| {
                        if ui.button("Remove Current From Catalog").clicked() {
                            ui.close();
                            self.remove_current_photo_from_catalog();
                        }
                    });
                    ui.separator();
                    if ui
                        .selectable_label(
                            self.state.collection_filter == CollectionFilter::All,
                            "All Photos",
                        )
                        .clicked()
                    {
                        self.state.collection_filter = CollectionFilter::All;
                    }
                    if ui
                        .selectable_label(
                            self.state.collection_filter == CollectionFilter::Picks,
                            "Picks",
                        )
                        .clicked()
                    {
                        self.state.collection_filter = CollectionFilter::Picks;
                    }
                    if ui
                        .selectable_label(
                            self.state.collection_filter == CollectionFilter::Rejected,
                            "Rejected",
                        )
                        .clicked()
                    {
                        self.state.collection_filter = CollectionFilter::Rejected;
                    }
                });
                ui.menu_button("Develop", |ui| {
                    if ui.button("Show Develop Workspace").clicked() {
                        ui.close();
                        self.mode = WorkspaceMode::Develop;
                    }
                    if ui.button("Reset All Edits").clicked() {
                        ui.close();
                        self.state.edit = EditState::default();
                        self.state.mark_dirty();
                        edit_change.merge(EditChange::all());
                    }
                    if ui.button("Reset Crop").clicked() {
                        ui.close();
                        self.state.edit.crop = None;
                        self.state.mark_dirty();
                        edit_change.merge(EditChange::all());
                    }
                    if ui.button("Reset Zoom").clicked() {
                        ui.close();
                        self.state.canvas_zoom = 1.0;
                        self.state.canvas_pan = egui::Vec2::ZERO;
                    }
                    ui.separator();
                    ui.add_enabled_ui(self.state.photos_cache.len() > 1, |ui| {
                        if ui.button("Remove Current From Catalog").clicked() {
                            ui.close();
                            self.remove_current_photo_from_catalog();
                        }
                    });
                });
                ui.menu_button("Export", |ui| {
                    if ui.button("Batch Export…").clicked() {
                        ui.close();
                        self.export_dialog = Some(ExportDialogState::default());
                    }
                });
                let path = self.state.catalog.path().display().to_string();
                ui.label(format!("  catalog: {path}  |  P=Pick  U=None  X=Reject"));
                if let Some(progress) = self.import_progress.as_ref() {
                    let p = progress.lock().unwrap();
                    let total = p.total;
                    let current = p.current.min(total);
                    let name = if p.name.is_empty() {
                        "scanning".to_string()
                    } else {
                        p.name.clone()
                    };
                    ui.label(format!("  import: {current}/{total} {name}"));
                } else if let Some(message) = &self.import_message {
                    ui.label(format!("  {message}"));
                }
            });
        });

        egui::SidePanel::left("left")
            .default_width(220.0)
            .show(ctx, |ui| {
                if left_panel(ui, &mut self.state) {
                    edit_change.merge(EditChange::all());
                }
            });

        if self.mode == WorkspaceMode::Develop {
            egui::SidePanel::right("right")
                .default_width(280.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        edit_change.merge(right_panel(ui, &mut self.state.edit));
                    });
                });
        }

        if self.mode == WorkspaceMode::Develop {
            egui::TopBottomPanel::bottom("filmstrip")
                .default_height(120.0)
                .show(ctx, |ui| {
                    if let Some(summary) = self.state.filter_summary() {
                        ui.horizontal(|ui| {
                            ui.label(format!("Filter: {summary}"));
                            if ui.button("Clear Filters").clicked() {
                                self.state.clear_filters();
                            }
                        });
                    }

                    let photos = self.state.visible_photos();
                    if photos.is_empty() {
                        if self.state.folder_filter.is_some() {
                            ui.label("No photos in this folder.");
                        } else if self.state.collection_filter != CollectionFilter::All {
                            ui.label("No photos in this collection.");
                        } else {
                            ui.label("No photos yet. File → Import Photos / Import Folder…");
                        }
                        return;
                    }
                    let current_id = self.state.photo_id;
                    let mut clicked: Option<PhotoId> = None;
                    let thumbs: Vec<(PhotoId, egui::TextureHandle, Flag)> = photos
                        .iter()
                        .map(|p| {
                            let tex = self.ensure_thumb(ctx, p);
                            (p.id, tex, p.flag)
                        })
                        .collect();
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (pid, tex, flag) in &thumbs {
                                let is_current = *pid == current_id;
                                let img = egui::Image::new(tex).max_height(100.0).max_width(140.0);
                                let response = ui.add(img.sense(egui::Sense::click()));
                                let flag_color = match flag {
                                    Flag::Pick => Some(egui::Color32::from_rgb(80, 200, 80)),
                                    Flag::Reject => Some(egui::Color32::from_rgb(220, 80, 80)),
                                    Flag::None => None,
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
                                        egui::Stroke::new(
                                            2.0,
                                            egui::Color32::from_rgb(220, 200, 60),
                                        ),
                                        egui::StrokeKind::Outside,
                                    );
                                }
                                if response.clicked() {
                                    clicked = Some(*pid);
                                }
                            }
                        });
                    });
                    if let Some(photo_id) = clicked {
                        if let Err(e) = self.switch_to_photo_id(photo_id) {
                            log::warn!("switch_to_photo_id failed: {e}");
                        }
                    }
                });
        }

        // Phase 5C: snapshot the working preset (if the editor is open) so we can
        // draw the overlay inside the CentralPanel closure without conflicting
        // borrows against self.watermark_editor.
        let wm_preview: Option<chalkraw_core::WatermarkPreset> =
            self.watermark_editor.as_ref().map(|e| e.current.clone());
        let image_w = self.state.image.width;
        let image_h = self.state.image.height;

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.mode == WorkspaceMode::Library {
                if edit_change.any() {
                    if let Some(gpu) = self.gpu.as_ref() {
                        Self::refresh_gpu_for_edit(gpu, &self.state.edit);
                    }
                    self.state.mark_dirty();
                }
                self.show_library_grid(ctx, ui);
                return;
            }
            if let Some(gpu) = self.gpu.as_ref() {
                if edit_change.any() {
                    if edit_change.uniforms {
                        gpu.update(&self.state.edit);
                    }
                    if edit_change.tone_curve {
                        gpu.upload_tone_curve(&self.state.edit.tone_curve.rgb.0);
                    }
                    if edit_change.blur_inputs {
                        let nr_amount = (self.state.edit.detail.noise_reduction.luminance
                            + self.state.edit.detail.noise_reduction.color)
                            / 2.0;
                        gpu.run_blurs(
                            16.0,
                            self.state.edit.detail.sharpening.radius,
                            5.0,
                            nr_amount,
                        );
                    }
                    self.state.mark_dirty();
                }

                // Compute fit dimensions (letterbox — preserves image aspect ratio).
                let available = ui.available_size();
                let img_aspect = image_w as f32 / image_h as f32;
                let avail_aspect = available.x / available.y;
                let (fit_w, fit_h) = if img_aspect >= avail_aspect {
                    // Image wider than panel — fit width, letterbox top/bottom.
                    (available.x, available.x / img_aspect)
                } else {
                    // Image taller than panel — fit height, letterbox left/right.
                    (available.y * img_aspect, available.y)
                };

                // Apply zoom to the fitted dimensions.
                let zoom = self.state.canvas_zoom;
                let zoomed_w = fit_w * zoom;
                let zoomed_h = fit_h * zoom;

                // Allocate the full panel area; canvas interactions come from this response.
                let (full_rect, response) =
                    ui.allocate_exact_size(available, egui::Sense::click_and_drag());

                // Phase 5C/canvas zoom (Ctrl + mouse wheel).
                let (scroll_y, ctrl_held, pointer_over_canvas) = ctx.input(|i| {
                    (
                        i.smooth_scroll_delta.y,
                        i.modifiers.ctrl,
                        i.pointer
                            .hover_pos()
                            .map(|p| full_rect.contains(p))
                            .unwrap_or(false),
                    )
                });
                if scroll_y.abs() > 0.0 && pointer_over_canvas {
                    log::debug!("canvas scroll: y={scroll_y:.2}, ctrl={ctrl_held}");
                }
                if pointer_over_canvas && ctrl_held && scroll_y.abs() > 0.0 {
                    // Anchor zoom around the mouse pointer for natural feel.
                    let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
                    let old_zoom = self.state.canvas_zoom;
                    let zoom_step = if scroll_y > 0.0 { 1.1_f32 } else { 1.0 / 1.1 };
                    let new_zoom = (old_zoom * zoom_step).clamp(0.25, 16.0);
                    if let Some(p) = pointer_pos {
                        // Translate canvas_pan so the pointed-at image pixel stays under the cursor.
                        let from_centre = p - full_rect.center() - self.state.canvas_pan;
                        let scale_change = new_zoom / old_zoom;
                        let new_from_centre = from_centre * scale_change;
                        self.state.canvas_pan += from_centre - new_from_centre;
                    }
                    self.state.canvas_zoom = new_zoom;
                    // Consume the event so the right-panel ScrollArea doesn't also see it.
                    ctx.input_mut(|i| {
                        i.smooth_scroll_delta = egui::Vec2::ZERO;
                        i.raw_scroll_delta = egui::Vec2::ZERO;
                    });
                }

                // Drag-pan when zoomed in (any zoom level, including <1.0).
                if response.dragged() && response.drag_delta().length() > 0.5 {
                    self.state.canvas_pan += response.drag_delta();
                }

                // Double-click resets zoom + pan.
                if response.double_clicked() {
                    self.state.canvas_zoom = 1.0;
                    self.state.canvas_pan = egui::Vec2::ZERO;
                }

                // Position the image rect centred in the panel, offset by pan.
                let centre = full_rect.center() + self.state.canvas_pan;
                let image_rect =
                    egui::Rect::from_center_size(centre, egui::vec2(zoomed_w, zoomed_h));

                ui.painter().add(egui::Shape::Callback(
                    egui_wgpu::Callback::new_paint_callback(
                        image_rect,
                        CanvasCallback { gpu: gpu.clone() },
                    ),
                ));

                // Phase 5C: draw watermark preview overlay when the editor is open.
                if let Some(ref preset) = wm_preview {
                    draw_watermark_overlay(
                        ctx,
                        ui,
                        image_rect,
                        image_w,
                        image_h,
                        preset,
                        &mut self.watermark_preview_textures,
                    );
                }
            } else {
                ui.label("Initialising GPU…");
            }
        });

        // ── Export dialog ─────────────────────────────────────────────────────
        let export_count_current = self.export_source_count(ExportSource::Current);
        let export_count_workbench = self.export_source_count(ExportSource::Workbench);
        let export_count_picks = self.export_source_count(ExportSource::Picks);
        let export_count_all = self.export_source_count(ExportSource::All);
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
                    ui.label("Source:");
                    ui.radio_value(
                        &mut dlg.source,
                        ExportSource::Workbench,
                        format!("Workbench / table ({export_count_workbench} photos)"),
                    );
                    ui.radio_value(
                        &mut dlg.source,
                        ExportSource::Current,
                        format!("Current photo ({export_count_current})"),
                    );
                    ui.radio_value(
                        &mut dlg.source,
                        ExportSource::Picks,
                        format!("Picks ({export_count_picks} photos)"),
                    );
                    ui.radio_value(
                        &mut dlg.source,
                        ExportSource::All,
                        format!("All catalog photos ({export_count_all} photos)"),
                    );

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
                            // ── Preset picker (top half) ──────────────────────
                            ui.label(egui::RichText::new("Saved Presets").strong());
                            let presets = self.state.catalog.list_watermarks().unwrap_or_default();
                            let preset_label = dlg.watermark_preset_id
                                .and_then(|id| presets.iter().find(|p| p.id == id))
                                .map(|p| p.name.as_str())
                                .unwrap_or("(none)");
                            egui::ComboBox::from_id_salt("wm_preset_combo")
                                .selected_text(preset_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut dlg.watermark_preset_id, None, "(none)").clicked();
                                    for p in &presets {
                                        ui.selectable_value(&mut dlg.watermark_preset_id, Some(p.id), &p.name);
                                    }
                                });
                            ui.horizontal(|ui| {
                                if ui.button("New Watermark…").clicked() {
                                    self.watermark_preview_textures.clear();
                                    self.watermark_editor = Some(WatermarkEditorState {
                                        open: true,
                                        current: chalkraw_core::WatermarkPreset::new("New Watermark".into()),
                                        is_new: true,
                                        expanded_layer: None,
                                    });
                                }
                                let can_edit = dlg.watermark_preset_id.is_some();
                                ui.add_enabled_ui(can_edit, |ui| {
                                    if ui.button("Edit…").clicked() {
                                        if let Some(id) = dlg.watermark_preset_id {
                                            if let Some(p) = presets.iter().find(|p| p.id == id) {
                                                self.watermark_preview_textures.clear();
                                                self.watermark_editor = Some(WatermarkEditorState {
                                                    open: true,
                                                    current: p.clone(),
                                                    is_new: false,
                                                    expanded_layer: None,
                                                });
                                            }
                                        }
                                    }
                                    if ui.button("Delete").clicked() {
                                        if let Some(id) = dlg.watermark_preset_id {
                                            if self.state.catalog.delete_watermark(id).is_ok() {
                                                dlg.watermark_preset_id = None;
                                            }
                                        }
                                    }
                                });
                            });

                            ui.separator();

                            // ── Quick stamp (bottom half, legacy single-layer) ─
                            ui.label(egui::RichText::new("Quick Stamp (single layer, no preset)").strong());
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
                            ui.label(
                                egui::RichText::new("Export uses preset if selected; otherwise quick stamp if enabled.")
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            should_close = true;
                        }
                        let selected_count = match dlg.source {
                            ExportSource::Current => export_count_current,
                            ExportSource::Workbench => export_count_workbench,
                            ExportSource::Picks => export_count_picks,
                            ExportSource::All => export_count_all,
                        };
                        let can_export = dlg.output_dir.is_some() && selected_count > 0;
                        ui.add_enabled_ui(can_export, |ui| {
                            if ui.button("Export").clicked() {
                                should_start_export = true;
                            }
                        });
                        if dlg.output_dir.is_none() {
                            ui.label(egui::RichText::new("← choose output folder first").color(egui::Color32::GRAY).small());
                        } else if selected_count == 0 {
                            ui.label(egui::RichText::new("← choose at least one source photo").color(egui::Color32::GRAY).small());
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
                        _ => chalkraw_export::ExportFormat::Jpeg {
                            quality: dlg_inner.quality,
                        },
                    };
                    let resize = if dlg_inner.resize_long_edge {
                        chalkraw_export::ExportResize::LongEdge(dlg_inner.long_edge)
                    } else {
                        chalkraw_export::ExportResize::Original
                    };
                    // Resolve preset: load from catalog by id if one is selected.
                    let watermark_preset = dlg_inner.watermark_preset_id.and_then(|id| {
                        self.state
                            .catalog
                            .list_watermarks()
                            .ok()
                            .and_then(|list| list.into_iter().find(|p| p.id == id))
                    });
                    let opts = chalkraw_export::BatchOptions {
                        format,
                        resize,
                        output_dir,
                        name_pattern: dlg_inner.name_pattern.clone(),
                        watermark: dlg_inner.watermark.to_stamp(),
                        watermark_preset,
                    };
                    let source = dlg_inner.source;

                    match self.collect_export_items(source) {
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

        // ── Watermark preset editor sub-dialog ────────────────────────────────
        if let Some(ref mut editor) = self.watermark_editor {
            let mut open = editor.open;
            let mut save_clicked = false;
            let mut close_clicked = false;
            let mut remove_layer: Option<usize> = None;
            let mut move_up_layer: Option<usize> = None;
            let mut move_down_layer: Option<usize> = None;
            let mut add_layer = false;

            egui::Window::new("Watermark Preset Editor")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(380.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.current.name)
                                .desired_width(240.0),
                        );
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Layers").strong());

                    let anchor_labels = [
                        "↖ TL", "↑ TC", "↗ TR", "← CL", "·  C", "→ CR", "↙ BL", "↓ BC", "↘ BR",
                    ];
                    fn anchor_to_idx(a: chalkraw_core::WatermarkAnchor) -> usize {
                        use chalkraw_core::WatermarkAnchor::*;
                        match a {
                            TopLeft => 0,
                            TopCenter => 1,
                            TopRight => 2,
                            CenterLeft => 3,
                            Center => 4,
                            CenterRight => 5,
                            BottomLeft => 6,
                            BottomCenter => 7,
                            BottomRight => 8,
                        }
                    }
                    fn idx_to_anchor(i: usize) -> chalkraw_core::WatermarkAnchor {
                        use chalkraw_core::WatermarkAnchor::*;
                        match i {
                            0 => TopLeft,
                            1 => TopCenter,
                            2 => TopRight,
                            3 => CenterLeft,
                            4 => Center,
                            5 => CenterRight,
                            6 => BottomLeft,
                            7 => BottomCenter,
                            _ => BottomRight,
                        }
                    }

                    fn text_color_to_egui(c: chalkraw_core::TextColor) -> egui::Color32 {
                        egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
                    }
                    fn egui_to_text_color(c: egui::Color32) -> chalkraw_core::TextColor {
                        let arr = c.to_array();
                        chalkraw_core::TextColor {
                            r: arr[0],
                            g: arr[1],
                            b: arr[2],
                            a: arr[3],
                        }
                    }

                    // Capture layer count before the mutable borrow in iter_mut().
                    let layer_count = editor.current.layers.len();
                    for (idx, layer) in editor.current.layers.iter_mut().enumerate() {
                        let is_expanded = editor.expanded_layer == Some(idx);
                        match layer {
                            chalkraw_core::WatermarkLayer::Image(ref mut img) => {
                                let header_text = format!(
                                    "Image: {}  [{}]  {}%  {}%",
                                    img.png_path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| "(unset)".into()),
                                    anchor_labels[anchor_to_idx(img.anchor)],
                                    img.size_pct as u32,
                                    (img.opacity * 100.0) as u32,
                                );
                                if egui::CollapsingHeader::new(header_text)
                                    .id_salt(format!("wm_layer_{idx}"))
                                    .open(if is_expanded { Some(true) } else { None })
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label("PNG:");
                                            if ui.button("Browse…").clicked() {
                                                if let Some(p) = rfd::FileDialog::new()
                                                    .add_filter("PNG", &["png"])
                                                    .pick_file()
                                                {
                                                    img.png_path = p;
                                                }
                                            }
                                        });
                                        let path_str = img.png_path.display().to_string();
                                        if path_str.is_empty() {
                                            ui.colored_label(
                                                egui::Color32::YELLOW,
                                                "(no PNG selected)",
                                            );
                                        } else {
                                            ui.label(&path_str);
                                        }
                                        ui.add_space(4.0);
                                        ui.label("Anchor:");
                                        let mut anchor_idx = anchor_to_idx(img.anchor);
                                        egui::Grid::new(format!("wm_anchor_{idx}"))
                                            .num_columns(3)
                                            .show(ui, |ui| {
                                                for (i, label) in anchor_labels.iter().enumerate() {
                                                    let selected = anchor_idx == i;
                                                    if ui
                                                        .add(
                                                            egui::Button::new(*label)
                                                                .selected(selected),
                                                        )
                                                        .clicked()
                                                    {
                                                        anchor_idx = i;
                                                    }
                                                    if i % 3 == 2 {
                                                        ui.end_row();
                                                    }
                                                }
                                            });
                                        img.anchor = idx_to_anchor(anchor_idx);
                                        ui.horizontal(|ui| {
                                            ui.label("Size (% long edge):");
                                            ui.add(
                                                egui::Slider::new(&mut img.size_pct, 1.0..=50.0)
                                                    .fixed_decimals(0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Opacity (%):");
                                            let mut op_pct = img.opacity * 100.0;
                                            ui.add(
                                                egui::Slider::new(&mut op_pct, 0.0..=100.0)
                                                    .fixed_decimals(0),
                                            );
                                            img.opacity = op_pct / 100.0;
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Margin (% long edge):");
                                            ui.add(
                                                egui::Slider::new(&mut img.margin_pct, 0.0..=20.0)
                                                    .fixed_decimals(0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Rotation");
                                            ui.add(
                                                egui::Slider::new(
                                                    &mut img.rotation_deg,
                                                    -180.0..=180.0,
                                                )
                                                .fixed_decimals(0)
                                                .suffix("°"),
                                            );
                                        });
                                        if ui.button("Remove layer").clicked() {
                                            remove_layer = Some(idx);
                                        }
                                    })
                                    .header_response
                                    .clicked()
                                {
                                    editor.expanded_layer =
                                        if is_expanded { None } else { Some(idx) };
                                }
                                // Reorder / delete buttons shown next to every layer row.
                                // Top of list = drawn first (background); bottom = drawn last (foreground).
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(idx > 0, |ui| {
                                        if ui
                                            .small_button("↑")
                                            .on_hover_text("Move layer earlier (further back)")
                                            .clicked()
                                        {
                                            move_up_layer = Some(idx);
                                        }
                                    });
                                    ui.add_enabled_ui(idx + 1 < layer_count, |ui| {
                                        if ui
                                            .small_button("↓")
                                            .on_hover_text("Move layer later (further front)")
                                            .clicked()
                                        {
                                            move_down_layer = Some(idx);
                                        }
                                    });
                                    if ui.small_button("✕").on_hover_text("Delete layer").clicked()
                                    {
                                        remove_layer = Some(idx);
                                    }
                                });
                            }
                            chalkraw_core::WatermarkLayer::Text(ref mut txt) => {
                                let header_text = format!(
                                    "Text: \"{}\"  [{}]  {:.1}%  {}%",
                                    if txt.text.len() > 20 {
                                        &txt.text[..20]
                                    } else {
                                        &txt.text
                                    },
                                    anchor_labels[anchor_to_idx(txt.anchor)],
                                    txt.font_size_pct,
                                    (txt.opacity * 100.0) as u32,
                                );
                                if egui::CollapsingHeader::new(header_text)
                                    .id_salt(format!("wm_text_layer_{idx}"))
                                    .open(if is_expanded { Some(true) } else { None })
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label("Text:");
                                            ui.add(
                                                egui::TextEdit::singleline(&mut txt.text)
                                                    .desired_width(200.0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Font size (% long edge):");
                                            ui.add(
                                                egui::Slider::new(
                                                    &mut txt.font_size_pct,
                                                    0.5..=10.0,
                                                )
                                                .fixed_decimals(1),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Colour:");
                                            let mut col = text_color_to_egui(txt.color);
                                            if egui::color_picker::color_edit_button_srgba(
                                                ui,
                                                &mut col,
                                                egui::color_picker::Alpha::OnlyBlend,
                                            )
                                            .changed()
                                            {
                                                txt.color = egui_to_text_color(col);
                                            }
                                        });
                                        ui.add_space(4.0);
                                        ui.label("Anchor:");
                                        let mut anchor_idx = anchor_to_idx(txt.anchor);
                                        egui::Grid::new(format!("wm_text_anchor_{idx}"))
                                            .num_columns(3)
                                            .show(ui, |ui| {
                                                for (i, label) in anchor_labels.iter().enumerate() {
                                                    let selected = anchor_idx == i;
                                                    if ui
                                                        .add(
                                                            egui::Button::new(*label)
                                                                .selected(selected),
                                                        )
                                                        .clicked()
                                                    {
                                                        anchor_idx = i;
                                                    }
                                                    if i % 3 == 2 {
                                                        ui.end_row();
                                                    }
                                                }
                                            });
                                        txt.anchor = idx_to_anchor(anchor_idx);
                                        ui.horizontal(|ui| {
                                            ui.label("Opacity (%):");
                                            let mut op_pct = txt.opacity * 100.0;
                                            ui.add(
                                                egui::Slider::new(&mut op_pct, 0.0..=100.0)
                                                    .fixed_decimals(0),
                                            );
                                            txt.opacity = op_pct / 100.0;
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Margin (% long edge):");
                                            ui.add(
                                                egui::Slider::new(&mut txt.margin_pct, 0.0..=20.0)
                                                    .fixed_decimals(0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Rotation");
                                            ui.add(
                                                egui::Slider::new(
                                                    &mut txt.rotation_deg,
                                                    -180.0..=180.0,
                                                )
                                                .fixed_decimals(0)
                                                .suffix("°"),
                                            );
                                        });
                                        if ui.button("Remove layer").clicked() {
                                            remove_layer = Some(idx);
                                        }
                                    })
                                    .header_response
                                    .clicked()
                                {
                                    editor.expanded_layer =
                                        if is_expanded { None } else { Some(idx) };
                                }
                                // Reorder / delete buttons shown next to every layer row.
                                // Top of list = drawn first (background); bottom = drawn last (foreground).
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(idx > 0, |ui| {
                                        if ui
                                            .small_button("↑")
                                            .on_hover_text("Move layer earlier (further back)")
                                            .clicked()
                                        {
                                            move_up_layer = Some(idx);
                                        }
                                    });
                                    ui.add_enabled_ui(idx + 1 < layer_count, |ui| {
                                        if ui
                                            .small_button("↓")
                                            .on_hover_text("Move layer later (further front)")
                                            .clicked()
                                        {
                                            move_down_layer = Some(idx);
                                        }
                                    });
                                    if ui.small_button("✕").on_hover_text("Delete layer").clicked()
                                    {
                                        remove_layer = Some(idx);
                                    }
                                });
                            }
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui.button("+ Add Image Layer").clicked() {
                            add_layer = true;
                        }
                        if ui.button("+ Add Text Layer").clicked() {
                            let new_idx = editor.current.layers.len();
                            editor
                                .current
                                .layers
                                .push(chalkraw_core::WatermarkLayer::Text(
                                    chalkraw_core::TextLayer::default(),
                                ));
                            editor.expanded_layer = Some(new_idx);
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            save_clicked = true;
                        }
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });

            if let Some(idx) = remove_layer {
                editor.current.layers.remove(idx);
                if editor.expanded_layer == Some(idx) {
                    editor.expanded_layer = None;
                }
            } else if let Some(idx) = move_up_layer {
                if idx > 0 {
                    editor.current.layers.swap(idx, idx - 1);
                    // Keep the expanded row tracking consistent after the swap.
                    if editor.expanded_layer == Some(idx) {
                        editor.expanded_layer = Some(idx - 1);
                    } else if editor.expanded_layer == Some(idx - 1) {
                        editor.expanded_layer = Some(idx);
                    }
                }
            } else if let Some(idx) = move_down_layer {
                let len = editor.current.layers.len();
                if idx + 1 < len {
                    editor.current.layers.swap(idx, idx + 1);
                    // Keep the expanded row tracking consistent after the swap.
                    if editor.expanded_layer == Some(idx) {
                        editor.expanded_layer = Some(idx + 1);
                    } else if editor.expanded_layer == Some(idx + 1) {
                        editor.expanded_layer = Some(idx);
                    }
                }
            }
            if add_layer {
                let new_idx = editor.current.layers.len();
                editor
                    .current
                    .layers
                    .push(chalkraw_core::WatermarkLayer::Image(
                        chalkraw_core::ImageLayer::default(),
                    ));
                editor.expanded_layer = Some(new_idx);
            }
            if save_clicked {
                let preset = editor.current.clone();
                if let Err(e) = self.state.catalog.insert_watermark(&preset) {
                    log::warn!("save watermark preset failed: {e}");
                } else {
                    // If editing an existing preset in the export dialog, keep selection.
                    if let Some(ref mut dlg) = self.export_dialog {
                        dlg.watermark_preset_id = Some(preset.id);
                    }
                    self.watermark_preview_textures.clear();
                    self.watermark_editor = None;
                }
            }
            if close_clicked || !open {
                self.watermark_preview_textures.clear();
                self.watermark_editor = None;
            }
        }

        // Keep the watch-folder poll alive when the app is idle.
        if self.state.watch_folder.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs(5));
        }
        if self.import_progress.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        // Debounced autosave.
        self.state.flush_if_due();
        if self.state.dirty_since.is_some() {
            ctx.request_repaint_after(DEBOUNCE + Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_dir_finds_recursively_filtered_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("b.png"), b"x").unwrap();
        std::fs::write(dir.path().join("c.txt"), b"x").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("d.cr2"), b"x").unwrap();
        std::fs::write(sub.join("e.heic"), b"x").unwrap();
        let mut out = Vec::new();
        walk_dir(dir.path(), &["jpg", "png", "cr2"], &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn process_import_paths_skips_known_duplicate_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.jpg");
        std::fs::write(&path, EMBEDDED_FIXTURE).unwrap();
        let known_hash = *blake3::hash(EMBEDDED_FIXTURE).as_bytes();
        let progress = Arc::new(Mutex::new(ImportProgress {
            current: 0,
            total: 0,
            name: String::new(),
            done: false,
            candidates: Vec::new(),
            summary: ImportSummary::default(),
            error: None,
        }));

        let (candidates, summary) =
            process_import_paths(vec![path], HashSet::from([known_hash]), &progress);

        assert!(candidates.is_empty());
        assert_eq!(summary.scanned, 1);
        assert_eq!(summary.decoded, 0);
        assert_eq!(summary.duplicates, 1);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn process_import_paths_deduplicates_within_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a.jpg");
        let path_b = dir.path().join("b.jpg");
        std::fs::write(&path_a, EMBEDDED_FIXTURE).unwrap();
        std::fs::write(&path_b, EMBEDDED_FIXTURE).unwrap();
        let progress = Arc::new(Mutex::new(ImportProgress {
            current: 0,
            total: 0,
            name: String::new(),
            done: false,
            candidates: Vec::new(),
            summary: ImportSummary::default(),
            error: None,
        }));

        let (candidates, summary) =
            process_import_paths(vec![path_a, path_b], HashSet::new(), &progress);

        assert_eq!(candidates.len(), 1);
        assert_eq!(summary.scanned, 2);
        assert_eq!(summary.decoded, 1);
        assert_eq!(summary.duplicates, 1);
        assert_eq!(summary.failed, 0);
    }
}
