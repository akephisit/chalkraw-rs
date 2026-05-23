# chalkraw-rs — Design Spec

- **Date:** 2026-05-23
- **Status:** Draft, pending user review
- **Author:** brainstormed with user via Claude

## 1. Overview

`chalkraw-rs` is a Lightroom-style non-destructive photo editor written in 100% Rust. It targets photographers who want to import a batch of photos, edit each one with professional develop controls (white balance, tone, color grading, etc.), apply reusable presets and watermarks, and export to JPEG/PNG/TIFF with GPU-accelerated processing.

### Goals

- Non-destructive editing: original files never modified, edits stored as state in a catalog database.
- 100% Rust toolchain: no C/C++ FFI dependencies (with one accepted exception, see §3.7).
- GPU acceleration via `wgpu` for both interactive preview and final export.
- Cross-platform: Windows, macOS, Linux from one codebase.
- Lightroom-style UX: left panel (folders/collections/presets), center canvas + filmstrip, right develop panel.
- Batch workflow: import multiple photos into one catalog, edit one at a time, batch export.

### Non-goals (v1)

- Local adjustments (masks, brushes, gradients) — Tier D, deferred to v2.
- Mobile / web versions.
- Cloud sync, sharing, social features.
- Tethered shooting, map, book, web modules.
- Rating system (only flag Pick/Reject).
- Wider color spaces (sRGB output only for v1; Display P3, Adobe RGB deferred).
- Output sharpening modes (Screen/Matte/Glossy).
- AI features (sky replace, subject mask, auto enhance).

## 2. Scope (v1)

**Tier B+ workflow** (batch with lightweight catalog) **with Tier C edit operations** (Advanced).

### File support

| Input | Mechanism |
|---|---|
| JPEG / PNG / TIFF (8-bit & 16-bit) | `image` crate |
| RAW: Canon CR2/CR3, Nikon NEF, Sony ARW, Fuji RAF, Pentax PEF, Olympus ORF | `rawloader` (pure Rust, subset of cameras) |

Cameras outside `rawloader`'s supported set are not supported in v1.

### Edit operations (Tier C)

- **Basic:** White Balance (temp, tint), Exposure, Contrast, Highlights, Shadows, Whites, Blacks
- **Presence:** Texture, Clarity, Dehaze
- **Color:** Vibrance, Saturation
- **Tone Curve:** RGB master + per-channel (R/G/B) spline editor
- **HSL:** 8 colors (red, orange, yellow, green, aqua, blue, purple, magenta) × hue/sat/lum
- **Color Grading:** color wheels for shadows / midtones / highlights / global
- **Detail:** Sharpening (amount, radius, detail, masking), Noise Reduction (luminance, color)
- **Effects:** Vignette (amount, midpoint, feather, roundness), Grain (amount, size, roughness)
- **Lens Correction:** distortion, vignetting (auto profile lookup when EXIF identifies lens)
- **Geometry:** rotate, crop (perspective transform deferred to v2)

### Presets

- Develop-only presets (color/tone settings; no crop, no watermark).
- Save / load / apply to current photo or batch.

### Watermarks

- Multi-layer per preset (e.g., logo PNG + text signature).
- Per layer: source (PNG image or text), anchor (9-point grid or free position), size (% of long edge), opacity, margin, rotation.
- WYSIWYG preview using the same GPU pipeline as export.

### Export

- Formats: JPEG (quality 1–100), PNG (lossless), TIFF 16-bit (lossless).
- Color space: sRGB only.
- Resize: original or long-edge in pixels.
- File naming: tokenized pattern (`{name}` `{ext}` `{date}` `{time}` `{counter}` `{camera}` `{lens}` `{iso}`).
- Output folder: custom path with optional subfolder.
- Watermark: apply saved watermark preset.
- Metadata: keep original EXIF, optional copyright field overlay.
- Batch: progress bar, safe cancel (finish current, stop next).

### UI

- Single window, three zones (left panel / center canvas + filmstrip / right develop panel).
- Menu bar (File, Edit, Library, Develop, Export).
- Keyboard shortcuts: Lightroom-style (`R` crop, `\` before/after, `P/U/X` pick/unflag/reject, `[/]` prev/next, `Cmd+E` export, etc.).

## 3. Architecture

### 3.1 Layered structure

```
UI Layer (egui)
   │
App State Layer  ──── Catalog Store (redb)  ──── IO Layer (image-rs, rawloader)
   │                                                    ▲
   └──────────  Render Engine (wgpu) ───────────────────┘
```

### 3.2 Cargo workspace layout

```
chalkraw-rs/
├── Cargo.toml                # workspace
└── crates/
    ├── chalkraw-core/        # shared types: Edit, Photo, Preset, Watermark
    ├── chalkraw-catalog/     # redb wrapper, schema, CRUD
    ├── chalkraw-render/      # wgpu pipeline + WGSL shaders
    ├── chalkraw-io/          # image decode/encode, RAW handling
    ├── chalkraw-export/      # export orchestration (resize, watermark, encode, batch)
    └── chalkraw-ui/          # egui binary (the app itself)
```

Each crate has a single responsibility, is independently testable, and the dependency graph is a DAG (`ui` → `catalog` / `render` / `export` → `core` / `io`). `core` has no internal deps; `io` depends only on `core`.

### 3.3 Crate responsibilities

| Crate | Owns | Does not own |
|---|---|---|
| `chalkraw-core` | data types (`Photo`, `EditState`, `Preset`, `WatermarkPreset`), serde impls, schema version | I/O, GPU, UI |
| `chalkraw-catalog` | redb tables, CRUD API, schema migration | data types (uses core), GPU |
| `chalkraw-render` | wgpu device/queue, shader compilation, render passes, texture pools | image I/O, UI |
| `chalkraw-io` | decode (`image`, `rawloader`), encode (`image`), EXIF parse | catalog, GPU, UI |
| `chalkraw-export` | batch orchestration, pipeline parallelism (rayon + channels), file naming | direct rendering (uses render), direct DB (uses catalog) |
| `chalkraw-ui` | egui app, app state, event routing, dialogs | all the above are dependencies |

### 3.4 Dependency choices

Versions are minimums verified current as of 2026-05-23.

| Concern | Crate | Min version | Rationale |
|---|---|---|---|
| GUI | `egui` + `eframe` | 0.33 | fast dev, mature wgpu integration, image-editor ecosystem precedent. Uses the new `App::ui` API (replaced `App::update` in 0.33). MSRV 1.92. |
| GPU | `wgpu` | 29.0 | cross-platform (Vulkan/Metal/DX12), 100% Rust, near-native perf. New `CurrentSurfaceTexture` enum and `WriteOnly<[u8]>` mapping API. MSRV 1.87. |
| Image decode/encode | `image` | 0.25 | covers JPEG/PNG/TIFF/WebP, pure Rust |
| RAW decode | `rawloader` | 0.37 | pure Rust, Canon/Nikon/Sony/Fuji/Pentax/Olympus subset. `quickraw` is a viable alternative if rawloader gaps appear during development. |
| EXIF | `kamadak-exif` | 0.6 | pure Rust |
| Catalog DB | `redb` | 4.1 | pure Rust embedded KV store, ACID, copy-on-write B+tree. Recent perf improvements in 4.1. |
| Serde | `serde` + `bincode` (DB) / `serde_json` (preset export) | latest | standard |
| Hashing | `blake3` | 1.x | fast content hash for file relink detection |
| Parallelism | `rayon` + `crossbeam-channel` | latest | producer-consumer pipeline |
| Error handling | `thiserror` (libs) + `anyhow` (binary) | latest | Rust convention |
| Profiling | `puffin` | latest | dev-time perf inspection |
| Tests | `cargo-nextest` + custom golden image diff | latest | fast, deterministic |

The workspace's `rust-toolchain.toml` will pin to 1.92 or later to cover the strictest MSRV in the dependency set (egui).

### 3.5 Pure-Rust commitment

All listed dependencies are 100% Rust. `redb` is chosen over `rusqlite` specifically to avoid bundling SQLite's C source. The trade-off (no SQL query language) is accepted in exchange for strict purity.

## 4. Data Model (redb)

The catalog file (`*.chalkraw`) is a redb database with these tables:

### 4.1 `photos` table

- **Key:** `PhotoId` (UUID v7)
- **Value:**
  ```rust
  struct Photo {
      id: Uuid,
      original_path: PathBuf,        // absolute path to source
      file_hash: [u8; 32],           // BLAKE3 hash for relink detection
      imported_at: DateTime<Utc>,
      width: u32,
      height: u32,
      format: ImageFormat,           // Jpeg | Png | Tiff | Raw(RawFormat)
      exif: ExifMetadata,            // parsed subset (camera, lens, iso, shutter, etc.)
      thumbnail: Vec<u8>,            // pre-rendered 512px JPEG, ~30-80KB
      flag: Flag,                    // None | Pick | Reject
  }
  ```

### 4.2 `edits` table

- **Key:** `PhotoId` (1:1 with photo)
- **Value:** `EditState` containing all Tier C adjustments. Layout:
  ```rust
  struct EditState {
      white_balance: WhiteBalance,            // { temp_kelvin, tint }
      tone: Tone,                              // { exposure, contrast, highlights,
                                               //   shadows, whites, blacks }
      presence: Presence,                      // { texture, clarity, dehaze }
      color: ColorMix,                         // { vibrance, saturation }
      tone_curve: ToneCurve,                   // { rgb, red, green, blue: Vec<Point> }
      hsl: [HslAdjustment; 8],                 // 8 colors
      color_grading: ColorGrading,             // { shadows, mids, highs, global }
      detail: Detail,                          // { sharpening, noise_reduction }
      effects: Effects,                        // { vignette, grain }
      lens_correction: LensCorrection,
      crop: Option<Crop>,                      // { rect, rotation_deg }
      history: VecDeque<EditSnapshot>,         // undo stack, capped at 50
      version: u32,                            // schema version
  }
  ```

  Every field has a meaningful "identity" default (e.g., exposure = 0.0, vibrance = 0, curve = linear). A fresh photo with no edits has an `EditState::default()` whose render is pixel-identical to the source.

### 4.3 `presets` table

- **Key:** `PresetId` (UUID v7)
- **Value:**
  ```rust
  struct Preset {
      name: String,
      develop: DevelopSubset,        // color/tone fields only (no crop, no watermark)
      created_at: DateTime<Utc>,
  }
  ```

  `DevelopSubset` is a projection of `EditState` containing only fields that make sense to apply across different photos: `white_balance`, `tone`, `presence`, `color`, `tone_curve`, `hsl`, `color_grading`, `detail`, `effects`. It explicitly excludes `crop` and `lens_correction` (lens correction is per-photo).

### 4.4 `watermarks` table

- **Key:** `WatermarkId` (UUID v7)
- **Value:**
  ```rust
  struct WatermarkPreset {
      name: String,
      layers: Vec<WatermarkLayer>,
  }

  struct WatermarkLayer {
      source: WatermarkSource,       // Image(path, cached_bytes) | Text(string, font, color)
      anchor: Anchor,                // Grid9(row, col) | Free { x_pct, y_pct }
      size_pct: f32,                 // 0..1, relative to long edge
      opacity: f32,                  // 0..1
      rotation_deg: f32,
      margin_pct: f32,               // 0..1, relative to long edge
  }
  ```

### 4.5 `catalog_meta` table

- **Key:** `"meta"` (single fixed row)
- **Value:**
  ```rust
  struct CatalogMeta {
      name: String,
      created_at: DateTime<Utc>,
      app_version: semver::Version,
      schema_version: u32,
  }
  ```

### 4.6 Invariants

- Original files are never modified. The `original_path` and `file_hash` together let the app detect when a source has moved or been replaced.
- All slider changes commit to redb in an ACID transaction, with a 100ms debounce window to coalesce drag updates.
- `EditState::version` and `CatalogMeta::schema_version` enable forward migration on app upgrade. Opening a catalog from a newer app version surfaces a clear error.

## 5. GPU Pipeline

### 5.1 Stages

```
[Source file]
    ▼ (CPU decode: image-rs or rawloader)
[Linear RGBA f32 buffer]
    ▼ (upload to GPU)
[source_texture: RGBA16Float, linear]
    │
    ├── downsize ──▶ [preview_texture]  ◀── slider feedback (60fps target)
    │
    └── full size ──▶ [full_res_texture] ◀── export
                            │
                            ▼ SHADER CHAIN (passes)
                            ├ 1. lens_correct.wgsl   (sample distortion)
                            ├ 2. wb_tone_curve_hsl.wgsl  (per-pixel, fused)
                            ├ 3. presence.wgsl       (clarity/texture, needs blur)
                            ├ 4. detail.wgsl         (sharpen + NR)
                            ├ 5. effects.wgsl        (vignette + grain)
                            ├ 6. color_grade.wgsl    (shadow/mid/high color)
                            ├ 7. crop.wgsl           (ROI sampling)
                            └ 8. watermark.wgsl      (composite layers)
                            ▼
                  ┌─────────┴──────────┐
                  ▼                    ▼
            (sRGB encode →      (readback to CPU →
             egui texture →      image-rs encode →
             screen)              JPEG/PNG/TIFF file)
```

### 5.2 Precision

All intermediate textures are `RGBA16Float` in linear color space. No quantization happens between passes. Conversion to sRGB (8-bit) or TIFF (16-bit int) happens only at the very end before display or file encode. This is what "non-destructive, no quality loss" means concretely.

### 5.3 Per-pixel ops are fused

Steps 2, 5, 6 (white balance → exposure → tone curve → HSL, vignette + grain, color grading) are all per-pixel and can share a single shader invocation to minimize memory bandwidth. Multi-pixel ops (clarity, sharpening, NR, lens distortion) require separate passes with ping-pong textures.

### 5.4 Tone curve as 1D LUT

The tone curve UI generates a 256-element (or 1024-element) lookup table, uploaded as a 1D texture. The shader samples by linear interpolation. Cheaper than evaluating splines per pixel.

### 5.5 Preview vs export

- **Preview:** source texture is downscaled to fit the canvas (target ~2000px long edge) once. Slider drags re-run the shader chain on this smaller texture for 60fps responsiveness.
- **Export:** full-resolution texture is run through the chain once per photo.

### 5.6 Batch export pipelining (Quick wins)

```
Reader thread → Decoder pool → GPU queue → Encoder pool → Writer thread
  (1 thread)    (N threads)    (1 stream)   (N threads)    (1 thread)
```

- Decoder and encoder pools use `rayon` (N = num_cpus).
- Stages are connected by bounded `crossbeam` channels for backpressure.
- GPU resources (pipelines, sampler, bind group layouts, watermark texture if shared) are created once per batch and reused.
- GPU readback is async via `wgpu::Buffer::map_async`, so the next photo can render while the previous one is being read back.
- Memory-mapped writes (`memmap2`) for TIFF outputs only.

Expected effect: batch time approximates `max(decode/N, gpu_render, encode/N)` per photo rather than the sum.

## 6. UI Layout

```
┌──────────────────────────────────────────────────────────────────┐
│ File  Edit  Library  Develop  Export   [catalog: name.chalkraw]  │
├──────────────┬────────────────────────────────────┬──────────────┤
│              │                                    │              │
│ LEFT PANEL   │         CANVAS (wgpu)              │ RIGHT PANEL  │
│              │                                    │              │
│ Folders      │  [current photo render]            │ Histogram    │
│ Collections  │  pan/zoom                          │ Basic        │
│   - All      │  [fit] [1:1] [3:1] [B&A]           │ Presence     │
│   - Picks    │                                    │ Color        │
│   - Rejected │ ─────────────────────────────      │ Tone Curve   │
│              │                                    │ HSL          │
│ Presets      │ FILMSTRIP (current view)           │ Color Grading│
│   - User     │ [thumb][thumb][thumb] ...          │ Detail       │
│   - Built-in │ ⌗ 124/847                          │ Effects      │
│              │                                    │ Lens Correct │
│              │                                    │ Geometry     │
└──────────────┴────────────────────────────────────┴──────────────┘
```

Right-panel groups are individually collapsible. Special interactions:

- Drag canvas = pan; scroll = zoom; double-click = toggle 1:1.
- Double-click a slider label = reset to default.
- Right-click a slider = copy / paste value across photos.
- Alt-drag Highlights/Shadows/Whites/Blacks = show clipping mask overlay (Lightroom convention).

### 6.1 User flow

1. Start app → "Open Catalog" or "New Catalog".
2. (New) `File → Import` → select folder/files → preview list → "Import".
   - `photos` table populated.
   - Thumbnails generated in background via rayon.
3. Click photo in filmstrip → load full-res to GPU → adjust sliders.
   - Slider change → re-run preview pipeline (~5ms target).
   - Edit auto-saved to redb (debounced 100ms).
4. Apply preset → click in Left panel → develop fields update.
5. `P / U / X` → flag pick / unflag / reject.
6. `Export` → choose photos → Export dialog → batch run with progress bar.

### 6.2 Watermark editor

Reached via `Library → Watermarks…` or from inside the Export dialog. Single window with preview canvas on the left and layer editor on the right. Supports multiple layers per preset (image and/or text), 9-point anchor grid plus free positioning, percentage-based size and margin (so the same preset works across portrait and landscape).

### 6.3 Export dialog

Single modal with sections: Destination, File naming, Format, Image sizing, Watermark, Metadata. Live estimate of total output size and time. Progress UI during run shows current file, elapsed and remaining time, with a safe Cancel that finishes the current photo then stops.

### 6.4 Keyboard shortcuts (Lightroom-style)

| Key | Action |
|---|---|
| `J` | toggle highlight clip warning |
| `R` | enter crop mode |
| `\` | before/after compare |
| `P` / `U` / `X` | flag pick / unflag / reject |
| `[` / `]` | previous / next photo |
| `Cmd+E` / `Ctrl+E` | export selected |

## 7. Error Handling

Library crates (`core`, `catalog`, `render`, `io`, `export`) use `thiserror::Error` to define typed error enums. The `ui` binary uses `anyhow::Result` to combine them and presents user-readable dialogs.

### 7.1 User-facing error categories

| Category | Example | Handling |
|---|---|---|
| Source file missing | path in catalog no longer resolves | "Photo missing" badge in filmstrip; "Locate…" dialog |
| RAW format unsupported | camera outside rawloader's set | dialog "Unsupported camera"; offer to import as JPEG only |
| GPU init failed | no wgpu adapter | dialog "GPU required"; suggest software fallback off |
| GPU out of memory | gigapixel source | auto-downsize; warning toast |
| Disk write failed | output folder permissions | show error in export dialog before starting |
| Catalog corrupt | redb file damaged | dialog "Catalog damaged"; "Try recovery" using redb checkpoint |
| Schema version mismatch | catalog from newer app | clear blocking dialog; no auto-downgrade |

### 7.2 Crash safety

- redb is ACID. Each committed edit is durable.
- Slider drag changes are batched in a 100ms debounce window before commit. Worst case after a crash: lose < 100ms of slider movement.
- App startup detects an unclean shutdown and surfaces a non-blocking notice if redb performed recovery on open.

## 8. Testing Strategy

| Crate | Test type | Coverage |
|---|---|---|
| `chalkraw-core` | unit | serde round-trip, default values, schema migration paths |
| `chalkraw-catalog` | integration | CRUD against a temp redb file, schema version handling, transaction semantics |
| `chalkraw-render` | golden image | render reference fixtures with known edits, compare to baseline PNGs with pixel-diff threshold |
| `chalkraw-io` | integration | decode fixtures (Canon CR2, Nikon NEF, Sony ARW samples) → verify dimensions and pixel checksums |
| `chalkraw-export` | integration end-to-end | input fixture → apply preset → export JPEG → verify output exists, dimensions correct, watermark present |
| `chalkraw-ui` | smoke | headless start + main-flow click-through |

### 8.1 Golden image testing

Render tests load a known source, apply a known edit state, read back GPU output, and compare against a committed baseline PNG. Average per-channel diff must stay under a threshold (e.g., 0.5 on 8-bit) to account for tiny inter-vendor GPU differences. Intentional shader changes update the baseline in the same PR.

### 8.2 CI

GitHub Actions matrix across Ubuntu (Vulkan), macOS (Metal), and Windows (DX12). `cargo nextest` for parallel test execution. GPU tests on CI use `wgpu`'s software adapter (lavapipe / llvmpipe), which is slow but deterministic enough for golden tests.

## 9. Performance Targets

| Operation | Target | Reference hardware |
|---|---|---|
| Slider drag (preview 2000px) | < 16 ms/frame (60 fps) | GTX 1660 / M1 base |
| Open catalog (5,000 photos) | < 2 sec to usable filmstrip | thumbnails from redb cache |
| Import 100 photos (with thumbnails) | < 30 sec | rayon parallel decode + GPU thumb gen |
| Full-res render of one 24MP photo | < 100 ms | full pipeline once |
| Batch export 100 photos JPEG q92 | < 30 sec | CPU+GPU pipelined |
| Catalog filter+sort across 50,000 photos | < 100 ms | redb scan + in-memory filter |
| App cold start to catalog UI | < 1.5 sec | shader pre-compile + redb mmap |

Targets are enforced through optional benchmarks in CI (running on a dedicated runner with known hardware).

### 9.1 Profiling

The `puffin` profiler is wired in in debug builds and exposed under `Help → Performance`. Status bar can optionally display GPU frame time for the develop view.

## 10. Future Work (post-v1)

- Tier D local adjustments: masks, brushes, radial/linear gradients, spot heal.
- GPU-side RAW demosaicing (AHD/Malvar in WGSL) for faster RAW decode.
- Decoded RAW cache for repeated exports of the same photos.
- Wider color management: Display P3 and Adobe RGB output, ICC profile support.
- Output sharpening modes (Screen / Matte / Glossy).
- More RAW formats via additional pure-Rust decoders, as `rawloader` grows.
- Hardware JPEG encoders (NVJPEG, etc.) — only if a performance need justifies breaking pure-Rust commitment.
- Tone curve point-targeted adjustment (click image to set curve point).
- Cross-photo settings copy/paste.
- Perspective transform under Geometry.

## 11. Related Prior Art

[`RapidRAW`](https://github.com/CyberTimon/RapidRAW) (2026) is a recent and similar project: non-destructive, GPU-accelerated RAW editor in Rust, also using `wgpu` and `rawloader`. It differs from `chalkraw-rs` in its UI stack — RapidRAW uses Tauri + React, while `chalkraw-rs` uses `egui` for a pure-Rust UI with smaller binaries and fewer transitive dependencies. RapidRAW is a useful reference for shader strategy and is worth reviewing during implementation.

## 12. Open Questions

None at design close. All decisions in this document reflect choices the user explicitly confirmed during brainstorming.
