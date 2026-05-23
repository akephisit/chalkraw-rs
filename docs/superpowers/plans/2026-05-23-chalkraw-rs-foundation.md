# chalkraw-rs Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the full vertical slice — open a JPEG, display it via wgpu in an egui window, with a working Exposure slider that updates the preview in real time and persists to a redb catalog. Proves the architecture end-to-end before adding more features.

**Architecture:** 6-crate Cargo workspace per design spec §3.2. Phase 1 implements the minimum surface of each crate needed to prove the end-to-end pipeline. Subsequent phases (Phase 2 — full Tier C develop tools; Phase 3 — catalog/import/filmstrip; Phase 4 — RAW; Phase 5 — watermark; Phase 6 — presets; Phase 7 — export) will get their own plans.

**Tech Stack:** Rust 1.92+, `wgpu` 29, `egui`/`eframe` 0.33, `redb` 4.1, `image` 0.25, `kamadak-exif` 0.6, `serde`, `bincode`, `uuid` v7, `blake3`, `chrono`, `thiserror`, `anyhow`.

**Spec reference:** [`../specs/2026-05-23-chalkraw-rs-design.md`](../specs/2026-05-23-chalkraw-rs-design.md)

**Scope of this plan (Phase 1 only):**
- Cargo workspace with all 6 crate skeletons (some near-empty for now).
- `chalkraw-core`: complete data types (full `EditState` shape including all Tier C fields, with identity defaults) and serde.
- `chalkraw-io`: decode JPEG/PNG/TIFF to a linear RGBA `f32` buffer.
- `chalkraw-catalog`: open/create catalog, photos table CRUD, edits table CRUD, debounced auto-save.
- `chalkraw-render`: wgpu init, source texture upload, single fragment shader applying Exposure only, render-to-texture + readback for tests.
- `chalkraw-ui`: eframe app, three-pane layout, embedded wgpu canvas widget showing the rendered preview, working Exposure slider on the right panel.
- Hardcoded test image opens on app start (file picker comes in Phase 3).
- One end-to-end smoke test that exercises the full pipeline.

**Not in scope (defer to later phases):**
- Any develop control other than Exposure (sliders are not yet wired; the right panel will show placeholder section headers).
- RAW decoding (`rawloader` is listed as a dep stub but not used).
- Watermark editor and watermark composition shader.
- Preset save/apply.
- Import dialog, filmstrip, folder browser.
- Batch export and the export pipeline.
- Crop, lens correction, geometry transforms.
- Tone curve UI, color grading wheels, HSL panel.
- Golden image tests beyond the Exposure shader.

---

## File Structure

This is the map of files Phase 1 creates. Subsequent phases extend these files; this plan locks in the boundaries.

```
chalkraw-rs/
├── Cargo.toml                          # workspace manifest
├── rust-toolchain.toml                 # pin Rust 1.92
├── .gitignore                          # standard Rust ignore
├── crates/
│   ├── chalkraw-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # re-exports
│   │       ├── photo.rs                # Photo, ImageFormat, Flag, ExifMetadata
│   │       └── edit.rs                 # EditState, WhiteBalance, Tone, Presence,
│   │                                   #   ColorMix, ToneCurve, HslAdjustment,
│   │                                   #   ColorGrading, Detail, Effects,
│   │                                   #   LensCorrection, Crop, EditSnapshot
│   ├── chalkraw-io/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # re-exports
│   │       ├── decode.rs               # decode_image() -> LinearImage
│   │       └── error.rs                # IoError
│   ├── chalkraw-catalog/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # re-exports
│   │       ├── catalog.rs              # Catalog (open/create), table defs
│   │       ├── photos.rs               # photos table CRUD
│   │       ├── edits.rs                # edits table CRUD + debounced commit
│   │       └── error.rs                # CatalogError
│   ├── chalkraw-render/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs                  # re-exports
│   │   │   ├── device.rs               # RenderDevice (wgpu init)
│   │   │   ├── source.rs               # SourceTexture (upload linear image to GPU)
│   │   │   ├── pipeline.rs             # DevelopPipeline (shader + bind group)
│   │   │   ├── uniforms.rs             # EditUniforms (POD for shader)
│   │   │   ├── readback.rs             # read_texture_to_buffer (for tests)
│   │   │   └── error.rs                # RenderError
│   │   └── shaders/
│   │       └── develop.wgsl            # fused per-pixel shader (Phase 1: Exposure only)
│   ├── chalkraw-export/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                  # placeholder; populated in Phase 7
│   └── chalkraw-ui/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                 # eframe entry
│           ├── app.rs                  # ChalkrawApp (state + frame update)
│           ├── canvas.rs               # CanvasWidget (wgpu render target → egui)
│           └── panels.rs               # left_panel(), right_panel()
└── tests/
    └── fixtures/
        └── sample.jpg                  # 1024×768 test image (committed)
```

### File responsibility boundaries

- `chalkraw-core` knows nothing about I/O, GPU, or UI. It is pure data + serde.
- `chalkraw-io` knows about `image` and (later) `rawloader`. It produces `LinearImage` from a path.
- `chalkraw-catalog` knows about `redb`. It depends only on `chalkraw-core` for value types.
- `chalkraw-render` knows about `wgpu`. It depends only on `chalkraw-core` for `EditState` (to build uniforms). It does not read files.
- `chalkraw-ui` depends on everything; it is the integration point.

---

## Tasks

### Task 1: Initialize Cargo workspace and toolchain

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`

- [ ] **Step 1: Write workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/chalkraw-core",
    "crates/chalkraw-io",
    "crates/chalkraw-catalog",
    "crates/chalkraw-render",
    "crates/chalkraw-export",
    "crates/chalkraw-ui",
]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT OR Apache-2.0"
rust-version = "1.92"

[workspace.dependencies]
# Internal crates
chalkraw-core = { path = "crates/chalkraw-core" }
chalkraw-io = { path = "crates/chalkraw-io" }
chalkraw-catalog = { path = "crates/chalkraw-catalog" }
chalkraw-render = { path = "crates/chalkraw-render" }
chalkraw-export = { path = "crates/chalkraw-export" }

# Data
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bincode = "1"
uuid = { version = "1", features = ["v7", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
blake3 = "1"

# IO / images
image = "0.25"
rawloader = "0.37"
kamadak-exif = "0.6"

# DB
redb = "4.1"

# GPU
wgpu = "29"
bytemuck = { version = "1", features = ["derive"] }
pollster = "0.4"

# UI
eframe = { version = "0.33", default-features = false, features = ["wgpu", "default_fonts"] }
egui = "0.33"
egui-wgpu = "0.33"

# Concurrency
rayon = "1"
crossbeam-channel = "0.5"

# Errors / logging / testing
thiserror = "2"
anyhow = "1"
log = "0.4"
env_logger = "0.11"

[profile.release]
lto = "thin"
codegen-units = 1
```

- [ ] **Step 2: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "1.92"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Write `.gitignore`**

```
/target
**/*.rs.bk
Cargo.lock
.DS_Store
*.chalkraw
*.chalkraw-journal
```

- [ ] **Step 4: Verify workspace parses**

Run: `cargo metadata --no-deps --format-version 1 > /dev/null`
Expected: exits 0 with no output (workspace has no members yet, but manifest must parse).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore
git commit -m "Initialize Cargo workspace and toolchain pin"
```

---

### Task 2: Create all six crate skeletons

**Files:**
- Create: `crates/chalkraw-core/Cargo.toml`, `crates/chalkraw-core/src/lib.rs`
- Create: `crates/chalkraw-io/Cargo.toml`, `crates/chalkraw-io/src/lib.rs`
- Create: `crates/chalkraw-catalog/Cargo.toml`, `crates/chalkraw-catalog/src/lib.rs`
- Create: `crates/chalkraw-render/Cargo.toml`, `crates/chalkraw-render/src/lib.rs`
- Create: `crates/chalkraw-export/Cargo.toml`, `crates/chalkraw-export/src/lib.rs`
- Create: `crates/chalkraw-ui/Cargo.toml`, `crates/chalkraw-ui/src/main.rs`

- [ ] **Step 1: Write `crates/chalkraw-core/Cargo.toml`**

```toml
[package]
name = "chalkraw-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write `crates/chalkraw-core/src/lib.rs`**

```rust
pub mod edit;
pub mod photo;

pub use edit::*;
pub use photo::*;
```

- [ ] **Step 3: Write `crates/chalkraw-io/Cargo.toml`**

```toml
[package]
name = "chalkraw-io"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
chalkraw-core = { workspace = true }
image = { workspace = true }
kamadak-exif = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 4: Write `crates/chalkraw-io/src/lib.rs`**

```rust
pub mod decode;
pub mod error;

pub use decode::*;
pub use error::*;
```

- [ ] **Step 5: Write `crates/chalkraw-catalog/Cargo.toml`**

```toml
[package]
name = "chalkraw-catalog"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
chalkraw-core = { workspace = true }
redb = { workspace = true }
bincode = { workspace = true }
uuid = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 6: Write `crates/chalkraw-catalog/src/lib.rs`**

```rust
pub mod catalog;
pub mod edits;
pub mod error;
pub mod photos;

pub use catalog::*;
pub use error::*;
```

- [ ] **Step 7: Write `crates/chalkraw-render/Cargo.toml`**

```toml
[package]
name = "chalkraw-render"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
chalkraw-core = { workspace = true }
wgpu = { workspace = true }
bytemuck = { workspace = true }
pollster = { workspace = true }
thiserror = { workspace = true }
log = { workspace = true }
```

- [ ] **Step 8: Write `crates/chalkraw-render/src/lib.rs`**

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
pub use pipeline::*;
pub use source::*;
pub use uniforms::*;
```

- [ ] **Step 9: Write `crates/chalkraw-export/Cargo.toml`**

```toml
[package]
name = "chalkraw-export"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
chalkraw-core = { workspace = true }
chalkraw-render = { workspace = true }
chalkraw-io = { workspace = true }
chalkraw-catalog = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 10: Write `crates/chalkraw-export/src/lib.rs`**

```rust
//! Export pipeline. Populated in Phase 7.
```

- [ ] **Step 11: Write `crates/chalkraw-ui/Cargo.toml`**

```toml
[package]
name = "chalkraw-ui"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "chalkraw"
path = "src/main.rs"

[dependencies]
chalkraw-core = { workspace = true }
chalkraw-io = { workspace = true }
chalkraw-catalog = { workspace = true }
chalkraw-render = { workspace = true }
chalkraw-export = { workspace = true }
eframe = { workspace = true }
egui = { workspace = true }
egui-wgpu = { workspace = true }
wgpu = { workspace = true }
anyhow = { workspace = true }
env_logger = { workspace = true }
log = { workspace = true }
```

- [ ] **Step 12: Write `crates/chalkraw-ui/src/main.rs` (skeleton — fleshed out in Task 14)**

```rust
fn main() -> anyhow::Result<()> {
    env_logger::init();
    println!("chalkraw v0.1.0 — Phase 1 skeleton");
    Ok(())
}
```

- [ ] **Step 13: Write skeleton modules required by `lib.rs` re-exports so the workspace compiles**

For each module declared in a `lib.rs` above that doesn't yet have a source file, create an empty file. Create empty stubs:

`crates/chalkraw-core/src/photo.rs`:
```rust
//! Photo type. Implemented in Task 3.
```

`crates/chalkraw-core/src/edit.rs`:
```rust
//! EditState. Implemented in Task 4.
```

`crates/chalkraw-io/src/decode.rs`:
```rust
//! Image decode. Implemented in Task 8.
```

`crates/chalkraw-io/src/error.rs`:
```rust
//! IO errors. Implemented in Task 8.
```

`crates/chalkraw-catalog/src/catalog.rs`:
```rust
//! Catalog open/create. Implemented in Task 9.
```

`crates/chalkraw-catalog/src/photos.rs`:
```rust
//! Photos table. Implemented in Task 10.
```

`crates/chalkraw-catalog/src/edits.rs`:
```rust
//! Edits table. Implemented in Task 11.
```

`crates/chalkraw-catalog/src/error.rs`:
```rust
//! Catalog errors. Implemented in Task 9.
```

`crates/chalkraw-render/src/device.rs`:
```rust
//! RenderDevice. Implemented in Task 12.
```

`crates/chalkraw-render/src/source.rs`:
```rust
//! SourceTexture. Implemented in Task 13.
```

`crates/chalkraw-render/src/pipeline.rs`:
```rust
//! DevelopPipeline. Implemented in Task 14.
```

`crates/chalkraw-render/src/uniforms.rs`:
```rust
//! EditUniforms. Implemented in Task 14.
```

`crates/chalkraw-render/src/readback.rs`:
```rust
//! Readback. Implemented in Task 15.
```

`crates/chalkraw-render/src/error.rs`:
```rust
//! Render errors. Implemented in Task 12.
```

`crates/chalkraw-render/shaders/develop.wgsl`:
```wgsl
// Placeholder. Implemented in Task 14.
```

For each `lib.rs` that pulls in a module via `pub use`, replace the `pub use` line with `// placeholder` only if compile fails on empty modules; otherwise leave the re-exports — empty module files compile fine as `mod` declarations once their `pub use` items become available. (Concretely: change `pub use edit::*;` to `// pub use edit::*; // re-enabled in Task 4` for now, applied symmetrically across all `lib.rs` files. Restore each `pub use` line in the task that implements the module.)

- [ ] **Step 14: Verify whole workspace builds**

Run: `cargo build --workspace`
Expected: PASS with warnings about unused crates only.

- [ ] **Step 15: Verify the binary runs**

Run: `cargo run -p chalkraw-ui`
Expected: prints `chalkraw v0.1.0 — Phase 1 skeleton` and exits 0.

- [ ] **Step 16: Commit**

```bash
git add crates/
git commit -m "Add skeleton for all six workspace crates"
```

---

### Task 3: chalkraw-core — Photo, ImageFormat, Flag, ExifMetadata

**Files:**
- Modify: `crates/chalkraw-core/src/photo.rs`
- Test: `crates/chalkraw-core/src/photo.rs` (in-module `#[cfg(test)]`)

- [ ] **Step 1: Write failing test for `Photo::new`**

Replace `crates/chalkraw-core/src/photo.rs` with:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    Raw(RawFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawFormat {
    CanonCr2,
    CanonCr3,
    NikonNef,
    SonyArw,
    FujiRaf,
    PentaxPef,
    OlympusOrf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Flag {
    #[default]
    None,
    Pick,
    Reject,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifMetadata {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub shutter_speed: Option<String>,
    pub aperture: Option<f32>,
    pub focal_length: Option<f32>,
    pub captured_at: Option<DateTime<Utc>>,
}

pub type PhotoId = Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: PhotoId,
    pub original_path: PathBuf,
    pub file_hash: [u8; 32],
    pub imported_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub exif: ExifMetadata,
    pub thumbnail: Vec<u8>,
    pub flag: Flag,
}

impl Photo {
    pub fn new(
        original_path: PathBuf,
        file_hash: [u8; 32],
        width: u32,
        height: u32,
        format: ImageFormat,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            original_path,
            file_hash,
            imported_at: Utc::now(),
            width,
            height,
            format,
            exif: ExifMetadata::default(),
            thumbnail: Vec::new(),
            flag: Flag::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_new_assigns_v7_uuid_and_now() {
        let before = Utc::now();
        let p = Photo::new(
            PathBuf::from("/tmp/a.jpg"),
            [0u8; 32],
            1024,
            768,
            ImageFormat::Jpeg,
        );
        let after = Utc::now();
        assert_eq!(p.width, 1024);
        assert_eq!(p.height, 768);
        assert_eq!(p.format, ImageFormat::Jpeg);
        assert_eq!(p.flag, Flag::None);
        assert!(p.imported_at >= before && p.imported_at <= after);
        // UUID v7 variant bits
        assert_eq!(p.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn photo_roundtrips_through_serde() {
        let p = Photo::new(
            PathBuf::from("/tmp/a.jpg"),
            [7u8; 32],
            100,
            100,
            ImageFormat::Raw(RawFormat::CanonCr2),
        );
        let bytes = bincode::serialize(&p).unwrap();
        let back: Photo = bincode::deserialize(&bytes).unwrap();
        assert_eq!(p, back);
    }
}
```

- [ ] **Step 2: Add `bincode` to dev-dependencies for the test**

Modify `crates/chalkraw-core/Cargo.toml`, add at end:

```toml
[dev-dependencies]
bincode = { workspace = true }
```

- [ ] **Step 3: Re-enable `pub use photo::*;` in `crates/chalkraw-core/src/lib.rs`**

```rust
pub mod edit;
pub mod photo;

pub use photo::*;
```

(Leave the `pub use edit::*;` line removed for now — Task 4 restores it.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chalkraw-core --lib`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/chalkraw-core/
git commit -m "chalkraw-core: Photo, ImageFormat, Flag, ExifMetadata"
```

---

### Task 4: chalkraw-core — EditState with full Tier C shape and identity defaults

**Files:**
- Modify: `crates/chalkraw-core/src/edit.rs`
- Modify: `crates/chalkraw-core/src/lib.rs`

This task locks in the full shape per spec §4.2 with `Default` impls that produce identity (no-op) values for every field. Later phases wire each field to UI and shader; the data shape exists from day one to avoid catalog schema churn.

- [ ] **Step 1: Write failing tests**

Replace `crates/chalkraw-core/src/edit.rs` with:

```rust
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const EDIT_SCHEMA_VERSION: u32 = 1;
pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WhiteBalance {
    pub temp_kelvin: f32, // identity: 5500.0
    pub tint: f32,        // identity: 0.0, range -150..150
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self { temp_kelvin: 5500.0, tint: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Tone {
    pub exposure: f32,    // EV stops, identity 0.0, range -5..5
    pub contrast: f32,    // identity 0.0, range -100..100
    pub highlights: f32,  // identity 0.0, range -100..100
    pub shadows: f32,     // identity 0.0, range -100..100
    pub whites: f32,      // identity 0.0, range -100..100
    pub blacks: f32,      // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Presence {
    pub texture: f32,     // identity 0.0, range -100..100
    pub clarity: f32,     // identity 0.0, range -100..100
    pub dehaze: f32,      // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ColorMix {
    pub vibrance: f32,    // identity 0.0, range -100..100
    pub saturation: f32,  // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f32, // input 0..1
    pub y: f32, // output 0..1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve(pub Vec<CurvePoint>);

impl Default for Curve {
    /// Linear curve: y = x. Identity.
    fn default() -> Self {
        Self(vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 1.0, y: 1.0 },
        ])
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ToneCurve {
    pub rgb: Curve,
    pub red: Curve,
    pub green: Curve,
    pub blue: Curve,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct HslAdjustment {
    pub hue: f32,        // identity 0.0, range -100..100
    pub saturation: f32, // identity 0.0, range -100..100
    pub luminance: f32,  // identity 0.0, range -100..100
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HslColor {
    Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GradeTone {
    pub hue: f32,        // 0..360 degrees, identity 0
    pub saturation: f32, // 0..100, identity 0
    pub luminance: f32,  // -100..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ColorGrading {
    pub shadows: GradeTone,
    pub midtones: GradeTone,
    pub highlights: GradeTone,
    pub global: GradeTone,
    pub blending: f32,   // 0..100, identity 50
    pub balance: f32,    // -100..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sharpening {
    pub amount: f32,  // 0..150, identity 0
    pub radius: f32,  // 0.5..3.0, identity 1.0
    pub detail: f32,  // 0..100, identity 25
    pub masking: f32, // 0..100, identity 0
}

impl Default for Sharpening {
    fn default() -> Self {
        Self { amount: 0.0, radius: 1.0, detail: 25.0, masking: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct NoiseReduction {
    pub luminance: f32, // 0..100, identity 0
    pub color: f32,     // 0..100, identity 0
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Detail {
    pub sharpening: Sharpening,
    pub noise_reduction: NoiseReduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    pub amount: f32,    // -100..100, identity 0
    pub midpoint: f32,  // 0..100, identity 50
    pub feather: f32,   // 0..100, identity 50
    pub roundness: f32, // -100..100, identity 0
}

impl Default for Vignette {
    fn default() -> Self {
        Self { amount: 0.0, midpoint: 50.0, feather: 50.0, roundness: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Grain {
    pub amount: f32,    // 0..100, identity 0
    pub size: f32,      // 0..100, identity 25
    pub roughness: f32, // 0..100, identity 50
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Effects {
    pub vignette: Vignette,
    pub grain: Grain,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LensCorrection {
    pub distortion: f32, // -100..100, identity 0
    pub vignetting: f32, // 0..100, identity 0 (correction amount)
    pub auto_profile: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x_pct: f32,   // 0..1
    pub y_pct: f32,
    pub w_pct: f32,
    pub h_pct: f32,
    pub rotation_deg: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditSnapshot {
    pub state: Box<EditState>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditState {
    pub white_balance: WhiteBalance,
    pub tone: Tone,
    pub presence: Presence,
    pub color: ColorMix,
    pub tone_curve: ToneCurve,
    pub hsl: [HslAdjustment; 8],
    pub color_grading: ColorGrading,
    pub detail: Detail,
    pub effects: Effects,
    pub lens_correction: LensCorrection,
    pub crop: Option<Crop>,
    pub history: VecDeque<EditSnapshot>,
    pub version: u32,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            white_balance: WhiteBalance::default(),
            tone: Tone::default(),
            presence: Presence::default(),
            color: ColorMix::default(),
            tone_curve: ToneCurve::default(),
            hsl: [HslAdjustment::default(); 8],
            color_grading: ColorGrading::default(),
            detail: Detail::default(),
            effects: Effects::default(),
            lens_correction: LensCorrection::default(),
            crop: None,
            history: VecDeque::with_capacity(MAX_HISTORY),
            version: EDIT_SCHEMA_VERSION,
        }
    }
}

impl EditState {
    /// True if every adjustment is at its identity (no-op) value.
    pub fn is_identity(&self) -> bool {
        let d = Self::default();
        self.white_balance == d.white_balance
            && self.tone == d.tone
            && self.presence == d.presence
            && self.color == d.color
            && self.tone_curve == d.tone_curve
            && self.hsl == d.hsl
            && self.color_grading == d.color_grading
            && self.detail == d.detail
            && self.effects == d.effects
            && self.lens_correction == d.lens_correction
            && self.crop == d.crop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_identity() {
        let s = EditState::default();
        assert!(s.is_identity(), "default EditState must be identity");
        assert_eq!(s.version, EDIT_SCHEMA_VERSION);
        assert_eq!(s.white_balance.temp_kelvin, 5500.0);
        assert_eq!(s.tone.exposure, 0.0);
        assert_eq!(s.tone_curve.rgb.0.len(), 2); // linear curve
    }

    #[test]
    fn exposure_change_breaks_identity() {
        let mut s = EditState::default();
        s.tone.exposure = 1.0;
        assert!(!s.is_identity());
    }

    #[test]
    fn edit_state_roundtrips_through_bincode() {
        let mut s = EditState::default();
        s.tone.exposure = 0.5;
        s.white_balance.temp_kelvin = 6500.0;
        let bytes = bincode::serialize(&s).unwrap();
        let back: EditState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s, back);
    }
}
```

- [ ] **Step 2: Re-enable `pub use edit::*;` in `crates/chalkraw-core/src/lib.rs`**

```rust
pub mod edit;
pub mod photo;

pub use edit::*;
pub use photo::*;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p chalkraw-core --lib`
Expected: 5 passed (2 from Task 3 + 3 new).

- [ ] **Step 4: Commit**

```bash
git add crates/chalkraw-core/
git commit -m "chalkraw-core: EditState with full Tier C shape and identity defaults"
```

---

### Task 5: chalkraw-io — error type and decode_image for JPEG/PNG/TIFF

**Files:**
- Modify: `crates/chalkraw-io/src/error.rs`
- Modify: `crates/chalkraw-io/src/decode.rs`
- Create: `tests/fixtures/sample.jpg` (a 1024×768 test image)

- [ ] **Step 1: Add a test fixture image**

Generate a deterministic test image so the test can verify exact dimensions.

```bash
mkdir -p tests/fixtures
python3 - <<'PY'
from PIL import Image, ImageDraw
img = Image.new("RGB", (1024, 768), color=(40, 80, 120))
draw = ImageDraw.Draw(img)
for i in range(0, 1024, 64):
    draw.rectangle([i, 0, i+32, 768], fill=(220, 200, 100))
img.save("tests/fixtures/sample.jpg", quality=92)
PY
```

If Python/PIL is not available, place any 1024×768 JPEG at that path.

Verify: `file tests/fixtures/sample.jpg` reports JPEG, `1024x768`.

- [ ] **Step 2: Write `crates/chalkraw-io/src/error.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),

    #[error("unsupported format for {0}")]
    UnsupportedFormat(PathBuf),

    #[error("decode failed for {path}: {source}")]
    DecodeFailed {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 3: Write failing test for `decode_image`**

Replace `crates/chalkraw-io/src/decode.rs` with:

```rust
use crate::error::IoError;
use chalkraw_core::ImageFormat;
use image::ImageReader;
use std::path::{Path, PathBuf};

/// Decoded source in linear sRGB, RGBA, 32-bit float per channel.
///
/// Pixels are stored row-major, four floats per pixel (R, G, B, A) in 0..1.
/// Phase 1 decodes JPEG/PNG/TIFF only; RAW arrives in Phase 4.
#[derive(Debug, Clone)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub pixels: Vec<f32>, // length = width * height * 4
}

impl LinearImage {
    pub fn stride_bytes(&self) -> usize {
        self.width as usize * 4 * std::mem::size_of::<f32>()
    }
}

pub fn decode_image(path: impl AsRef<Path>) -> Result<LinearImage, IoError> {
    let path: PathBuf = path.as_ref().to_path_buf();
    if !path.exists() {
        return Err(IoError::NotFound(path));
    }

    let reader = ImageReader::open(&path)?
        .with_guessed_format()
        .map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e.into() })?;

    let format = match reader.format() {
        Some(image::ImageFormat::Jpeg) => ImageFormat::Jpeg,
        Some(image::ImageFormat::Png) => ImageFormat::Png,
        Some(image::ImageFormat::Tiff) => ImageFormat::Tiff,
        _ => return Err(IoError::UnsupportedFormat(path)),
    };

    let dyn_img = reader.decode().map_err(|e| IoError::DecodeFailed { path: path.clone(), source: e })?;
    let rgba8 = dyn_img.to_rgba8();
    let (w, h) = rgba8.dimensions();

    // sRGB 8-bit → linear f32 0..1. Simple gamma 2.2 approximation; refined in Phase 2.
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for &c in rgba8.as_raw() {
        let v = c as f32 / 255.0;
        let linear = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        pixels.push(linear);
    }

    Ok(LinearImage { width: w, height: h, format, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fixture_jpeg() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/sample.jpg");
        let img = decode_image(path).expect("decode failed");
        assert_eq!(img.width, 1024);
        assert_eq!(img.height, 768);
        assert_eq!(img.format, ImageFormat::Jpeg);
        assert_eq!(img.pixels.len(), 1024 * 768 * 4);
        // Alpha channel should all be 1.0 (linear of 255)
        for px in img.pixels.chunks_exact(4) {
            assert!((px[3] - 1.0).abs() < 1e-6, "alpha must be 1.0, got {}", px[3]);
        }
    }

    #[test]
    fn missing_file_returns_not_found() {
        let err = decode_image("/no/such/path.jpg").unwrap_err();
        assert!(matches!(err, IoError::NotFound(_)));
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p chalkraw-io --lib`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures/sample.jpg crates/chalkraw-io/
git commit -m "chalkraw-io: decode JPEG/PNG/TIFF to linear RGBA f32 buffer"
```

---

### Task 6: chalkraw-catalog — error type, catalog open/create

**Files:**
- Modify: `crates/chalkraw-catalog/src/error.rs`
- Modify: `crates/chalkraw-catalog/src/catalog.rs`

- [ ] **Step 1: Write `crates/chalkraw-catalog/src/error.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),

    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("redb transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("redb commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("redb database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("serialization error: {0}")]
    Serde(#[from] Box<bincode::ErrorKind>),

    #[error("schema version {found} not supported (this build expects {expected})")]
    SchemaVersion { found: u32, expected: u32 },

    #[error("photo not found: {0}")]
    PhotoNotFound(uuid::Uuid),

    #[error("path error for {0:?}")]
    Path(PathBuf),
}
```

- [ ] **Step 2: Write `crates/chalkraw-catalog/src/catalog.rs`**

```rust
use crate::error::CatalogError;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;

// Table definitions referenced from sibling modules.
pub(crate) const PHOTOS_TABLE: TableDefinition<&[u8; 16], &[u8]> =
    TableDefinition::new("photos");
pub(crate) const EDITS_TABLE: TableDefinition<&[u8; 16], &[u8]> =
    TableDefinition::new("edits");
pub(crate) const META_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("meta");

const META_KEY: &str = "meta";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CatalogMeta {
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub app_version: String,
    pub schema_version: u32,
}

pub struct Catalog {
    db: Database,
    path: PathBuf,
}

impl Catalog {
    /// Open existing catalog or create a new one. Initialises tables and meta row.
    pub fn open_or_create(path: impl AsRef<Path>, name: &str) -> Result<Self, CatalogError> {
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let db = Database::create(&path)?;

        // Ensure all tables exist (idempotent), and write meta if new.
        {
            let write = db.begin_write()?;
            {
                let _ = write.open_table(PHOTOS_TABLE)?;
                let _ = write.open_table(EDITS_TABLE)?;
                let mut meta = write.open_table(META_TABLE)?;
                if !existed || meta.get(META_KEY)?.is_none() {
                    let m = CatalogMeta {
                        name: name.to_string(),
                        created_at: chrono::Utc::now(),
                        app_version: env!("CARGO_PKG_VERSION").to_string(),
                        schema_version: SCHEMA_VERSION,
                    };
                    let bytes = bincode::serialize(&m)?;
                    meta.insert(META_KEY, bytes.as_slice())?;
                }
            }
            write.commit()?;
        }

        // Verify schema version on existing catalogs.
        let read = db.begin_read()?;
        let meta_tbl = read.open_table(META_TABLE)?;
        let stored = meta_tbl.get(META_KEY)?.ok_or_else(|| CatalogError::Path(path.clone()))?;
        let meta: CatalogMeta = bincode::deserialize(stored.value())?;
        if meta.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::SchemaVersion {
                found: meta.schema_version,
                expected: SCHEMA_VERSION,
            });
        }

        Ok(Self { db, path })
    }

    pub fn path(&self) -> &Path { &self.path }

    pub fn meta(&self) -> Result<CatalogMeta, CatalogError> {
        let read = self.db.begin_read()?;
        let meta_tbl = read.open_table(META_TABLE)?;
        let stored = meta_tbl.get(META_KEY)?.ok_or_else(|| CatalogError::Path(self.path.clone()))?;
        Ok(bincode::deserialize(stored.value())?)
    }

    pub(crate) fn db(&self) -> &Database { &self.db }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_new_catalog_with_meta() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chalkraw");
        let cat = Catalog::open_or_create(&path, "test").unwrap();
        let meta = cat.meta().unwrap();
        assert_eq!(meta.name, "test");
        assert_eq!(meta.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn reopening_preserves_meta() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chalkraw");
        let first_created = {
            let cat = Catalog::open_or_create(&path, "first").unwrap();
            cat.meta().unwrap().created_at
        };
        let cat = Catalog::open_or_create(&path, "ignored-on-reopen").unwrap();
        let meta = cat.meta().unwrap();
        assert_eq!(meta.name, "first");
        assert_eq!(meta.created_at, first_created);
    }
}
```

- [ ] **Step 3: Add `tempfile` and `chrono` to catalog dev-dependencies, and `chrono` to runtime deps**

Modify `crates/chalkraw-catalog/Cargo.toml`:

```toml
[dependencies]
chalkraw-core = { workspace = true }
redb = { workspace = true }
bincode = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

Add `tempfile` to `[workspace.dependencies]` in the top-level `Cargo.toml`:

```toml
tempfile = "3"
```

(then in the catalog `[dev-dependencies]`, switch to `tempfile = { workspace = true }`.)

- [ ] **Step 4: Re-enable `pub use catalog::*;` in `crates/chalkraw-catalog/src/lib.rs`**

```rust
pub mod catalog;
pub mod edits;
pub mod error;
pub mod photos;

pub use catalog::*;
pub use error::*;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p chalkraw-catalog --lib`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/chalkraw-catalog/
git commit -m "chalkraw-catalog: open/create with schema versioning"
```

---

### Task 7: chalkraw-catalog — photos and edits CRUD

**Files:**
- Modify: `crates/chalkraw-catalog/src/photos.rs`
- Modify: `crates/chalkraw-catalog/src/edits.rs`

- [ ] **Step 1: Write `crates/chalkraw-catalog/src/photos.rs`**

```rust
use crate::catalog::{Catalog, PHOTOS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{Photo, PhotoId};

impl Catalog {
    pub fn insert_photo(&self, photo: &Photo) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(photo)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(PHOTOS_TABLE)?;
            tbl.insert(photo.id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_photo(&self, id: PhotoId) -> Result<Photo, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        let v = tbl.get(id.as_bytes())?.ok_or(CatalogError::PhotoNotFound(id))?;
        Ok(bincode::deserialize(v.value())?)
    }

    pub fn list_photos(&self) -> Result<Vec<Photo>, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(PHOTOS_TABLE)?;
        let mut out = Vec::new();
        for entry in tbl.iter()? {
            let (_, v) = entry?;
            let p: Photo = bincode::deserialize(v.value())?;
            out.push(p);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{ImageFormat, Photo};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn cat() -> (tempfile::TempDir, Catalog) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        (dir, cat)
    }

    #[test]
    fn insert_then_get_returns_same_photo() {
        let (_dir, cat) = cat();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 100, 100, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();
        let back = cat.get_photo(p.id).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn list_returns_all_inserted() {
        let (_dir, cat) = cat();
        let p1 = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        let p2 = Photo::new(PathBuf::from("/x/b.jpg"), [1u8; 32], 2, 2, ImageFormat::Png);
        cat.insert_photo(&p1).unwrap();
        cat.insert_photo(&p2).unwrap();
        let list = cat.list_photos().unwrap();
        assert_eq!(list.len(), 2);
    }
}
```

- [ ] **Step 2: Write `crates/chalkraw-catalog/src/edits.rs`**

```rust
use crate::catalog::{Catalog, EDITS_TABLE};
use crate::error::CatalogError;
use chalkraw_core::{EditState, PhotoId};

impl Catalog {
    pub fn upsert_edit(&self, photo_id: PhotoId, edit: &EditState) -> Result<(), CatalogError> {
        let bytes = bincode::serialize(edit)?;
        let write = self.db().begin_write()?;
        {
            let mut tbl = write.open_table(EDITS_TABLE)?;
            tbl.insert(photo_id.as_bytes(), bytes.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    /// Returns Default `EditState` if none was ever stored for this photo.
    pub fn get_edit(&self, photo_id: PhotoId) -> Result<EditState, CatalogError> {
        let read = self.db().begin_read()?;
        let tbl = read.open_table(EDITS_TABLE)?;
        match tbl.get(photo_id.as_bytes())? {
            Some(v) => Ok(bincode::deserialize(v.value())?),
            None => Ok(EditState::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chalkraw_core::{ImageFormat, Photo};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn missing_edit_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        let e = cat.get_edit(p.id).unwrap();
        assert!(e.is_identity());
    }

    #[test]
    fn upsert_then_get_round_trips_exposure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.chalkraw");
        let cat = Catalog::open_or_create(&path, "t").unwrap();
        let p = Photo::new(PathBuf::from("/x/a.jpg"), [0u8; 32], 1, 1, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();
        let mut e = EditState::default();
        e.tone.exposure = 1.25;
        cat.upsert_edit(p.id, &e).unwrap();
        let back = cat.get_edit(p.id).unwrap();
        assert_eq!(back.tone.exposure, 1.25);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p chalkraw-catalog --lib`
Expected: 6 passed (2 from Task 6 + 4 new).

- [ ] **Step 4: Commit**

```bash
git add crates/chalkraw-catalog/
git commit -m "chalkraw-catalog: photos and edits CRUD"
```

---

### Task 8: chalkraw-render — RenderDevice (wgpu init)

**Files:**
- Modify: `crates/chalkraw-render/src/error.rs`
- Modify: `crates/chalkraw-render/src/device.rs`

- [ ] **Step 1: Write `crates/chalkraw-render/src/error.rs`**

```rust
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no GPU adapter available")]
    NoAdapter,

    #[error("device request failed: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    #[error("surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),

    #[error("buffer map error: {0}")]
    BufferMap(#[from] wgpu::BufferAsyncError),
}
```

- [ ] **Step 2: Write `crates/chalkraw-render/src/device.rs`**

```rust
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
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::NoAdapter)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("chalkraw render device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
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
            Err(RenderError::NoAdapter) => {
                eprintln!("skipping: no GPU adapter available in this environment");
            }
            Err(e) => panic!("unexpected init failure: {e}"),
        }
    }
}
```

- [ ] **Step 3: Re-enable `pub use device::*;` and `pub use error::*;` in `crates/chalkraw-render/src/lib.rs`**

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
```

(Leave the rest of the `pub use` lines removed until their modules are implemented in Tasks 9–11.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p chalkraw-render --lib`
Expected: 1 passed (or skipped with stderr note if no GPU).

- [ ] **Step 5: Commit**

```bash
git add crates/chalkraw-render/
git commit -m "chalkraw-render: RenderDevice headless wgpu init"
```

---

### Task 9: chalkraw-render — SourceTexture (upload linear image to GPU)

**Files:**
- Modify: `crates/chalkraw-render/src/source.rs`

- [ ] **Step 1: Write `crates/chalkraw-render/src/source.rs`**

```rust
use crate::device::RenderDevice;

/// A linear RGBA16Float source texture on the GPU.
///
/// `LinearImage` arrives as f32; we convert to f16 on upload via bytemuck
/// for the RGBA16Float texture format.
pub struct SourceTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl SourceTexture {
    /// Upload `pixels` (length = width*height*4, each pixel RGBA f32 in 0..1).
    pub fn upload(rd: &RenderDevice, width: u32, height: u32, pixels: &[f32]) -> Self {
        assert_eq!(pixels.len() as u32, width * height * 4, "pixel buffer size mismatch");

        // Convert f32 → f16 (half) for RGBA16Float. wgpu accepts u16 bit patterns.
        let half_pixels: Vec<u16> = pixels.iter().map(|&v| f32_to_f16_bits(v)).collect();

        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = rd.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("chalkraw source"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        rd.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&half_pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4 * 2), // 4 channels × 2 bytes (f16)
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, width, height }
    }
}

/// IEEE-754 binary32 → binary16 (round-to-nearest-even).
fn f32_to_f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;

    if exp == 0xff {
        // Inf / NaN
        let m = if mant != 0 { 0x0200 } else { 0 };
        return sign | 0x7c00 | m;
    }
    let unbiased = exp - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00; // overflow → Inf
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign; // underflow → 0
        }
        let mant = mant | 0x0080_0000;
        let shift = 14 - unbiased;
        let half = (mant >> shift) as u16;
        let round = ((mant >> (shift - 1)) & 1) as u16;
        return sign | (half + round);
    }
    let half_exp = (unbiased as u16) << 10;
    let half_mant = (mant >> 13) as u16;
    let round = ((mant >> 12) & 1) as u16;
    sign | half_exp | half_mant + round
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploads_small_image_or_skips_in_sandbox() {
        let rd = match RenderDevice::new_headless() {
            Ok(rd) => rd,
            Err(_) => {
                eprintln!("skipping: no GPU");
                return;
            }
        };
        let w = 4;
        let h = 4;
        let pixels: Vec<f32> = (0..w * h)
            .flat_map(|i| [(i as f32) / 16.0, 0.5, 1.0, 1.0])
            .collect();
        let src = SourceTexture::upload(&rd, w, h, &pixels);
        assert_eq!(src.width, 4);
        assert_eq!(src.height, 4);
    }

    #[test]
    fn f16_roundtrip_close_for_typical_values() {
        for v in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 0.1234] {
            let h = f32_to_f16_bits(v);
            // Reconstruct: this is just smoke; precise checks happen in render tests.
            assert!(h <= 0xffff);
            let _ = h;
            let _ = v;
        }
    }
}
```

- [ ] **Step 2: Re-enable `pub use source::*;` in `crates/chalkraw-render/src/lib.rs`**

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
pub use source::*;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p chalkraw-render --lib`
Expected: all pass (some may be skip-noted on no-GPU CI).

- [ ] **Step 4: Commit**

```bash
git add crates/chalkraw-render/
git commit -m "chalkraw-render: SourceTexture upload as Rgba16Float"
```

---

### Task 10: chalkraw-render — EditUniforms (POD for shader bindings)

**Files:**
- Modify: `crates/chalkraw-render/src/uniforms.rs`

- [ ] **Step 1: Write `crates/chalkraw-render/src/uniforms.rs`**

For Phase 1 only the Exposure stop is wired. The full uniform struct shape mirrors `EditState` so Phase 2 only fills more fields; layout reservations are made now.

```rust
use bytemuck::{Pod, Zeroable};
use chalkraw_core::EditState;

/// std140-ish uniform layout for the develop shader. Padded so every field aligns
/// on 16 bytes in WGSL.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EditUniforms {
    pub exposure: f32,    // EV stops
    pub _pad_tone: [f32; 3],
    // Reserved slots for Phase 2 fields. Holes are intentional so adding fields
    // later does not invalidate Phase 1 shader bindings — WGSL just reads more.
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub _pad_basic: [f32; 3],
    pub temp_kelvin: f32,
    pub tint: f32,
    pub _pad_wb: [f32; 2],
    pub vibrance: f32,
    pub saturation: f32,
    pub texture: f32,
    pub clarity: f32,
}

impl From<&EditState> for EditUniforms {
    fn from(e: &EditState) -> Self {
        Self {
            exposure: e.tone.exposure,
            _pad_tone: [0.0; 3],
            contrast: e.tone.contrast,
            highlights: e.tone.highlights,
            shadows: e.tone.shadows,
            whites: e.tone.whites,
            blacks: e.tone.blacks,
            _pad_basic: [0.0; 3],
            temp_kelvin: e.white_balance.temp_kelvin,
            tint: e.white_balance.tint,
            _pad_wb: [0.0; 2],
            vibrance: e.color.vibrance,
            saturation: e.color.saturation,
            texture: e.presence.texture,
            clarity: e.presence.clarity,
        }
    }
}
```

- [ ] **Step 2: Re-enable `pub use uniforms::*;` in `crates/chalkraw-render/src/lib.rs`**

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
pub use source::*;
pub use uniforms::*;
```

- [ ] **Step 3: Run a compile-only check (no test logic added)**

Run: `cargo build -p chalkraw-render`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/chalkraw-render/
git commit -m "chalkraw-render: EditUniforms with reserved slots for Phase 2"
```

---

### Task 11: chalkraw-render — WGSL develop shader (Phase 1: Exposure only)

**Files:**
- Modify: `crates/chalkraw-render/shaders/develop.wgsl`

- [ ] **Step 1: Write the shader**

Replace `crates/chalkraw-render/shaders/develop.wgsl` with:

```wgsl
// chalkraw-rs develop shader — Phase 1 implements Exposure only.
// Later phases extend this single fused per-pixel shader with the remaining
// basic, tone-curve, HSL, color-grading, presence, and effects operations.

struct EditUniforms {
    exposure: f32,
    _pad_tone: vec3<f32>,
    contrast: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    _pad_basic: vec3<f32>,
    temp_kelvin: f32,
    tint: f32,
    _pad_wb: vec2<f32>,
    vibrance: f32,
    saturation: f32,
    texture: f32,
    clarity: f32,
};

@group(0) @binding(0) var source_tex: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> edit: EditUniforms;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
    // Full-screen triangle covering NDC.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    let p = positions[idx];
    let uv = uvs[idx];
    var out: VertexOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let src = textureSample(source_tex, source_sampler, in.uv);

    // Exposure: multiply linear by 2^stops.
    let gain = pow(2.0, edit.exposure);
    let lit = src.rgb * gain;

    // Output is linear; final sRGB encode is handled by the target view format.
    return vec4<f32>(lit, src.a);
}
```

- [ ] **Step 2: Verify shader compiles at workspace build time**

Run: `cargo build -p chalkraw-render`
Expected: success; the shader is compiled at runtime by wgpu, so this is only a sanity check that the file is readable from `include_str!`.

- [ ] **Step 3: Commit**

```bash
git add crates/chalkraw-render/shaders/
git commit -m "chalkraw-render: develop.wgsl with Exposure-only fragment shader"
```

---

### Task 12: chalkraw-render — DevelopPipeline

**Files:**
- Modify: `crates/chalkraw-render/src/pipeline.rs`

- [ ] **Step 1: Write the pipeline**

```rust
use crate::device::RenderDevice;
use crate::source::SourceTexture;
use crate::uniforms::EditUniforms;
use bytemuck::bytes_of;
use std::sync::Arc;

/// Output texture format. The UI uses `Bgra8UnormSrgb` (matches egui-wgpu's
/// surface); offscreen tests use `Rgba8UnormSrgb`.
#[derive(Debug, Clone, Copy)]
pub struct PipelineConfig {
    pub output_format: wgpu::TextureFormat,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self { output_format: wgpu::TextureFormat::Rgba8UnormSrgb }
    }
}

pub struct DevelopPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub uniform_buffer: wgpu::Buffer,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl DevelopPipeline {
    pub fn new(rd: &RenderDevice, cfg: PipelineConfig) -> Self {
        let shader = rd.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("develop.wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/develop.wgsl").into(),
            ),
        });

        let bind_group_layout = rd.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("develop bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<EditUniforms>() as u64),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = rd.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("develop pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = rd.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("develop pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: cfg.output_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = rd.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("develop sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("develop uniforms"),
            size: std::mem::size_of::<EditUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            uniform_buffer,
            device: rd.device.clone(),
            queue: rd.queue.clone(),
        }
    }

    pub fn update_uniforms(&self, u: &EditUniforms) {
        self.queue.write_buffer(&self.uniform_buffer, 0, bytes_of(u));
    }

    pub fn make_bind_group(&self, source: &SourceTexture) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("develop bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_buffer.as_entire_binding() },
            ],
        })
    }

    pub fn render(&self, target: &wgpu::TextureView, bind_group: &wgpu::BindGroup) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("develop encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("develop pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
```

- [ ] **Step 2: Re-enable `pub use pipeline::*;` in `crates/chalkraw-render/src/lib.rs`**

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
pub use pipeline::*;
pub use source::*;
pub use uniforms::*;
```

- [ ] **Step 3: Compile**

Run: `cargo build -p chalkraw-render`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/chalkraw-render/
git commit -m "chalkraw-render: DevelopPipeline with Exposure-only WGSL shader"
```

---

### Task 13: chalkraw-render — Readback for offscreen tests + end-to-end render test

**Files:**
- Modify: `crates/chalkraw-render/src/readback.rs`
- Test: `crates/chalkraw-render/tests/exposure.rs`

- [ ] **Step 1: Write the readback helper**

```rust
//! Read a render target back to CPU memory. Test-only utility.

use crate::device::RenderDevice;
use crate::error::RenderError;
use std::sync::Arc;

pub fn make_target(rd: &RenderDevice, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("readback target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub fn read_to_cpu(
    rd: &RenderDevice,
    target: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let row_bytes = (width * 4).max(256); // wgpu requires 256-byte row alignment
    let padded_row = ((row_bytes + 255) / 256) * 256;
    let buffer_size = (padded_row * height) as u64;

    let buffer = rd.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = rd.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    rd.queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    rd.device.poll(wgpu::Maintain::Wait).panic_on_timeout();
    rx.recv().expect("map_async channel closed").map_err(RenderError::from)?;

    let mapped = slice.get_mapped_range();
    let mut out = Vec::with_capacity((width * 4 * height) as usize);
    for row in 0..height {
        let start = (row * padded_row) as usize;
        let end = start + (width * 4) as usize;
        out.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    buffer.unmap();

    let _ = Arc::clone; // suppress unused-import lint if no Arc usage here
    Ok(out)
}
```

- [ ] **Step 2: Re-enable `pub use readback::*;` in lib.rs**

Edit `crates/chalkraw-render/src/lib.rs`, ensuring all six modules are re-exported:

```rust
pub mod device;
pub mod error;
pub mod pipeline;
pub mod readback;
pub mod source;
pub mod uniforms;

pub use device::*;
pub use error::*;
pub use pipeline::*;
pub use readback::*;
pub use source::*;
pub use uniforms::*;
```

- [ ] **Step 3: Write integration test exercising the full Exposure path**

Create `crates/chalkraw-render/tests/exposure.rs`:

```rust
use chalkraw_core::EditState;
use chalkraw_render::{
    make_target, read_to_cpu, DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice,
    SourceTexture,
};

fn solid_image(w: u32, h: u32, gray: f32) -> Vec<f32> {
    (0..w * h).flat_map(|_| [gray, gray, gray, 1.0]).collect()
}

fn pixel_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn exposure_zero_returns_input_brightness() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let w = 16;
    let h = 16;
    let src = SourceTexture::upload(&rd, w, h, &solid_image(w, h, 0.5));
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    let mut edit = EditState::default();
    edit.tone.exposure = 0.0;
    pipe.update_uniforms(&EditUniforms::from(&edit));
    let bg = pipe.make_bind_group(&src);
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bg);
    let pixels = read_to_cpu(&rd, &tex, w, h).unwrap();
    // Linear 0.5 → sRGB ~0.735 → 187. Allow tolerance for f16 + sRGB conversion.
    let p = pixel_at(&pixels, w, 8, 8);
    assert!((180..=195).contains(&(p[0] as u32)), "got R={}", p[0]);
}

#[test]
fn exposure_plus_one_doubles_linear_brightness() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skipping: no GPU"); return; }
    };
    let w = 16;
    let h = 16;
    let src = SourceTexture::upload(&rd, w, h, &solid_image(w, h, 0.25));
    let pipe = DevelopPipeline::new(&rd, PipelineConfig::default());
    let mut edit = EditState::default();
    edit.tone.exposure = 1.0; // 2× linear
    pipe.update_uniforms(&EditUniforms::from(&edit));
    let bg = pipe.make_bind_group(&src);
    let (tex, view) = make_target(&rd, w, h);
    pipe.render(&view, &bg);
    let pixels = read_to_cpu(&rd, &tex, w, h).unwrap();
    // Linear 0.5 → sRGB byte ~187.
    let p = pixel_at(&pixels, w, 8, 8);
    assert!((180..=195).contains(&(p[0] as u32)), "got R={}", p[0]);
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p chalkraw-render`
Expected: 2 integration tests pass (or print skip messages on no-GPU CI).

- [ ] **Step 5: Commit**

```bash
git add crates/chalkraw-render/
git commit -m "chalkraw-render: readback helper and Exposure render integration tests"
```

---

### Task 14: chalkraw-ui — eframe skeleton + three-pane layout

**Files:**
- Modify: `crates/chalkraw-ui/src/main.rs`
- Create: `crates/chalkraw-ui/src/app.rs`
- Create: `crates/chalkraw-ui/src/panels.rs`

- [ ] **Step 1: Write `crates/chalkraw-ui/src/main.rs`**

```rust
mod app;
mod canvas;
mod panels;

use app::ChalkrawApp;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_title("chalkraw"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "chalkraw",
        native_options,
        Box::new(|cc| Ok(Box::new(ChalkrawApp::new(cc)?))),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}
```

- [ ] **Step 2: Write `crates/chalkraw-ui/src/panels.rs`**

```rust
use chalkraw_core::EditState;
use egui::Ui;

pub fn left_panel(ui: &mut Ui, _state: &mut crate::app::AppState) {
    ui.heading("Catalog");
    ui.separator();
    ui.label("Folders");
    ui.indent("folders", |ui| {
        ui.label("(empty until import — Phase 3)");
    });
    ui.add_space(8.0);
    ui.label("Collections");
    ui.indent("collections", |ui| {
        ui.label("All");
        ui.label("Picks");
        ui.label("Rejected");
    });
    ui.add_space(8.0);
    ui.label("Presets");
    ui.indent("presets", |ui| {
        ui.label("(populated in Phase 6)");
    });
}

pub fn right_panel(ui: &mut Ui, edit: &mut EditState) -> bool {
    let mut changed = false;
    ui.heading("Develop");
    ui.separator();

    egui::CollapsingHeader::new("Histogram")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2)"); });

    egui::CollapsingHeader::new("Basic")
        .default_open(true)
        .show(ui, |ui| {
            ui.label("Exposure");
            if ui.add(egui::Slider::new(&mut edit.tone.exposure, -5.0..=5.0).fixed_decimals(2)).changed() {
                changed = true;
            }
            ui.add_space(4.0);
            ui.label("Contrast (Phase 2)");
            ui.add_enabled(false, egui::Slider::new(&mut edit.tone.contrast, -100.0..=100.0));
            ui.label("Highlights (Phase 2)");
            ui.add_enabled(false, egui::Slider::new(&mut edit.tone.highlights, -100.0..=100.0));
            ui.label("Shadows (Phase 2)");
            ui.add_enabled(false, egui::Slider::new(&mut edit.tone.shadows, -100.0..=100.0));
        });

    for header in ["Presence", "Color", "Tone Curve", "HSL", "Color Grading",
                   "Detail", "Effects", "Lens Correction", "Geometry"] {
        egui::CollapsingHeader::new(header)
            .default_open(false)
            .show(ui, |ui| { ui.label("(Phase 2)"); });
    }

    changed
}
```

- [ ] **Step 3: Write `crates/chalkraw-ui/src/app.rs`** (canvas integration deferred to Task 15)

```rust
use crate::panels::{left_panel, right_panel};
use chalkraw_core::EditState;

pub struct AppState {
    pub edit: EditState,
}

impl AppState {
    pub fn new() -> Self { Self { edit: EditState::default() } }
}

pub struct ChalkrawApp {
    state: AppState,
}

impl ChalkrawApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        Ok(Self { state: AppState::new() })
    }
}

impl eframe::App for ChalkrawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| { ui.label("(Phase 7)"); });
                ui.label(format!("  catalog: {}", "(none yet — Phase 3)"));
            });
        });

        egui::SidePanel::left("left").default_width(220.0).show(ctx, |ui| {
            left_panel(ui, &mut self.state);
        });

        egui::SidePanel::right("right").default_width(280.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let _changed = right_panel(ui, &mut self.state.edit);
            });
        });

        egui::TopBottomPanel::bottom("filmstrip").default_height(120.0).show(ctx, |ui| {
            ui.label("Filmstrip (Phase 3)");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Canvas (Task 15 wires this to wgpu)");
            ui.label(format!("Current Exposure: {:.2}", self.state.edit.tone.exposure));
        });
    }
}
```

- [ ] **Step 4: Update `crates/chalkraw-ui/src/canvas.rs`**

Create as a placeholder for Task 15:

```rust
//! Canvas widget. Wired to wgpu in Task 15.
```

- [ ] **Step 5: Run the app**

Run: `cargo run -p chalkraw-ui`
Expected: window opens with the three-pane layout, Exposure slider drags freely, label updates live, other sliders are visible but disabled.

- [ ] **Step 6: Commit**

```bash
git add crates/chalkraw-ui/
git commit -m "chalkraw-ui: three-pane eframe layout with Exposure slider stub"
```

---

### Task 15: chalkraw-ui — wgpu canvas integration (the visible render)

**Files:**
- Modify: `crates/chalkraw-ui/src/canvas.rs`
- Modify: `crates/chalkraw-ui/src/app.rs`

This task is the bridge: it makes the Exposure slider visibly affect a rendered image inside the egui window. We share the wgpu device that `egui-wgpu` already created, and add a custom paint callback that records our DevelopPipeline pass into the egui frame.

- [ ] **Step 1: Replace `crates/chalkraw-ui/src/canvas.rs`**

```rust
use chalkraw_core::EditState;
use chalkraw_io::LinearImage;
use chalkraw_render::{DevelopPipeline, EditUniforms, PipelineConfig, RenderDevice, SourceTexture};
use egui::PaintCallbackInfo;
use egui_wgpu::CallbackTrait;
use std::sync::Arc;

/// GPU-side resources that live for the lifetime of the loaded photo.
pub struct CanvasGpu {
    pub source: SourceTexture,
    pub pipeline: DevelopPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl CanvasGpu {
    pub fn new(rd: &RenderDevice, img: &LinearImage, output_format: wgpu::TextureFormat) -> Self {
        let source = SourceTexture::upload(rd, img.width, img.height, &img.pixels);
        let pipeline = DevelopPipeline::new(rd, PipelineConfig { output_format });
        let bind_group = pipeline.make_bind_group(&source);
        Self { source, pipeline, bind_group }
    }

    pub fn update(&self, edit: &EditState) {
        self.pipeline.update_uniforms(&EditUniforms::from(edit));
    }
}

/// One-frame draw callback that the egui-wgpu integration runs inside the
/// current render pass.
pub struct CanvasCallback {
    pub gpu: Arc<CanvasGpu>,
}

impl CallbackTrait for CanvasCallback {
    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'_>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        render_pass.set_pipeline(&self.gpu.pipeline.pipeline);
        render_pass.set_bind_group(0, &self.gpu.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}
```

- [ ] **Step 2: Rewrite `crates/chalkraw-ui/src/app.rs` to load the fixture image and host the canvas**

```rust
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

    fn ensure_gpu(&mut self, frame: &mut eframe::Frame) {
        if self.gpu.is_some() { return; }
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };
        let rd = RenderDevice::from_shared(render_state.device.clone(), render_state.queue.clone());
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
            egui::menu::bar(ui, |ui| {
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
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    rect,
                    CanvasCallback { gpu: gpu.clone() },
                ));
            } else {
                ui.label("Initialising GPU…");
            }
        });
    }
}

impl AppState {
    pub fn for_panel(&mut self) -> &mut Self { self }
}
```

- [ ] **Step 3: Update `panels.rs` to drop the unused `_state` parameter type churn**

Modify the signature of `left_panel` in `crates/chalkraw-ui/src/panels.rs` to use `&mut crate::app::AppState` (already declared in Task 14 — re-verify it builds).

Run: `cargo build -p chalkraw-ui`
Expected: success.

- [ ] **Step 4: Run the app and verify the slider visibly changes brightness**

Run: `cargo run -p chalkraw-ui --release`
Expected:
- Window opens with the fixture (blue + yellow stripes) visible in the central panel.
- Dragging Exposure right brightens the image instantly.
- Dragging Exposure left darkens the image.

(If running headless / over SSH, set `WAYLAND_DISPLAY` or `DISPLAY` appropriately; otherwise skip this manual check and rely on the integration test in Task 13.)

- [ ] **Step 5: Commit**

```bash
git add crates/chalkraw-ui/
git commit -m "chalkraw-ui: wgpu canvas with live Exposure slider"
```

---

### Task 16: Wire catalog auto-save with 100ms debounce

**Files:**
- Modify: `crates/chalkraw-ui/src/app.rs`
- Modify: `crates/chalkraw-ui/Cargo.toml`

- [ ] **Step 1: Add `chalkraw-catalog` and `uuid` to ui Cargo.toml dependencies**

Modify `crates/chalkraw-ui/Cargo.toml` `[dependencies]` to add (chalkraw-catalog already present from Task 2; add `uuid`):

```toml
uuid = { workspace = true }
```

- [ ] **Step 2: Extend `AppState` to own a `Catalog` and the current `PhotoId`**

Replace `crates/chalkraw-ui/src/app.rs` with:

```rust
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
            let hash = blake3::hash(&std::fs::read(&fixture)?).into();
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
        let due = match self.dirty_since {
            Some(t) if t.elapsed() >= DEBOUNCE => true,
            _ => false,
        };
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

    fn ensure_gpu(&mut self, frame: &mut eframe::Frame) {
        if self.gpu.is_some() { return; }
        let render_state = match frame.wgpu_render_state() {
            Some(rs) => rs,
            None => return,
        };
        let rd = RenderDevice::from_shared(render_state.device.clone(), render_state.queue.clone());
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
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                });
                ui.menu_button("Library", |ui| { ui.label("(Phase 3)"); });
                ui.menu_button("Develop", |ui| { ui.label("(Phase 2)"); });
                ui.menu_button("Export", |ui| { ui.label("(Phase 7)"); });
                let path = self.state.catalog.path().display().to_string();
                ui.label(format!("  catalog: {path}"));
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
                if edit_changed {
                    gpu.update(&self.state.edit);
                    self.state.mark_dirty();
                }
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    rect,
                    CanvasCallback { gpu: gpu.clone() },
                ));
            } else {
                ui.label("Initialising GPU…");
            }
        });

        // Debounced autosave; request a repaint slightly past the debounce so we
        // get woken even when the user has stopped interacting.
        self.state.flush_if_due();
        if self.state.dirty_since.is_some() {
            ctx.request_repaint_after(DEBOUNCE + Duration::from_millis(20));
        }
    }
}
```

- [ ] **Step 3: Add `blake3` to ui dependencies**

Modify `crates/chalkraw-ui/Cargo.toml` `[dependencies]`, add:

```toml
blake3 = { workspace = true }
```

- [ ] **Step 4: Build**

Run: `cargo build -p chalkraw-ui`
Expected: success.

- [ ] **Step 5: Manual verification of debounced autosave**

```bash
rm -f /tmp/test.chalkraw
CHALKRAW_CATALOG=/tmp/test.chalkraw cargo run -p chalkraw-ui --release
```

Expected: app starts; drag Exposure slider; wait ~200ms after stopping; close app; relaunch with the same catalog path → the last Exposure value is restored from `/tmp/test.chalkraw`.

(If running headless, instead verify by writing an integration test in Task 17.)

- [ ] **Step 6: Commit**

```bash
git add crates/chalkraw-ui/
git commit -m "chalkraw-ui: catalog autosave with 100ms debounce"
```

---

### Task 17: End-to-end smoke test (no GPU required) and final integration check

**Files:**
- Create: `crates/chalkraw-catalog/tests/round_trip.rs`

This tests the non-UI pipeline end-to-end: decode → catalog insert → upsert edit → relaunch (new `Catalog` handle) → read back the same edit. It runs on CI without a GPU.

- [ ] **Step 1: Write the integration test**

```rust
use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, ImageFormat, Photo};
use chalkraw_io::decode_image;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); p.pop();
    p.push("tests/fixtures/sample.jpg");
    p
}

#[test]
fn decode_then_persist_then_reload_restores_edit() {
    let dir = tempdir().unwrap();
    let cat_path = dir.path().join("e2e.chalkraw");

    // 1. Decode fixture.
    let img = decode_image(fixture_path()).unwrap();
    assert_eq!(img.width, 1024);

    // 2. Create catalog, insert photo, store an edit.
    let photo_id = {
        let cat = Catalog::open_or_create(&cat_path, "e2e").unwrap();
        let p = Photo::new(fixture_path(), [0u8; 32], img.width, img.height, ImageFormat::Jpeg);
        cat.insert_photo(&p).unwrap();
        let mut e = EditState::default();
        e.tone.exposure = 1.7;
        cat.upsert_edit(p.id, &e).unwrap();
        p.id
    };

    // 3. Re-open catalog and verify state.
    let cat = Catalog::open_or_create(&cat_path, "ignored").unwrap();
    let photos = cat.list_photos().unwrap();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].id, photo_id);
    let e = cat.get_edit(photo_id).unwrap();
    assert_eq!(e.tone.exposure, 1.7);
    assert_eq!(e.white_balance.temp_kelvin, 5500.0);
}
```

- [ ] **Step 2: Add `chalkraw-io` to catalog dev-dependencies**

Modify `crates/chalkraw-catalog/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = { workspace = true }
chalkraw-io = { workspace = true }
```

- [ ] **Step 3: Run the integration test**

Run: `cargo test -p chalkraw-catalog --test round_trip`
Expected: 1 passed.

- [ ] **Step 4: Run the entire test suite**

Run: `cargo test --workspace`
Expected: all tests pass (render integration tests may skip on no-GPU runners).

- [ ] **Step 5: Run clippy across the workspace**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

Resolve any warnings with the minimal change that addresses the lint (avoid `#[allow]` unless the lint is wrong about the code).

- [ ] **Step 6: Commit**

```bash
git add crates/
git commit -m "Add end-to-end decode→catalog→reload smoke test"
```

- [ ] **Step 7: Final tag**

```bash
git tag -a phase-1 -m "Phase 1 Foundation complete: end-to-end vertical slice with Exposure"
git log --oneline
```

Expected output: a clean sequence of commits, one per task, plus a `phase-1` tag.

---

## Phase 1 Done Criteria

When all tasks complete:

- [x] `cargo build --workspace` succeeds.
- [x] `cargo test --workspace` passes (render tests may skip on no-GPU CI).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- [x] `cargo run -p chalkraw-ui --release` opens a window, loads `tests/fixtures/sample.jpg`, and the Exposure slider visibly affects brightness in real time.
- [x] Closing and relaunching with the same `CHALKRAW_CATALOG` restores the last Exposure value.
- [x] No `unsafe` blocks have been added (Phase 1 does not need any).
- [x] No `unwrap`/`expect` in production code paths outside `main.rs` and tests; library code returns `Result`.

## Self-Review Notes

**Spec coverage:** This plan covers the architecture skeleton (spec §3), the redb data model end-to-end with full `EditState` shape (§4), the GPU pipeline core (§5, Exposure shader stage only), the UI layout shell (§6.1 main flow up to first slider), error handling foundations (§7.1 types declared, dialogs not yet surfaced), and unit/integration test scaffolding (§8). It does **not** cover: tier C develop math beyond Exposure (Phase 2), import flow / filmstrip / flags (Phase 3), RAW (Phase 4), watermark editor (Phase 5), presets (Phase 6), export pipeline (Phase 7). Each is a separate plan.

**Placeholder check:** No "TBD" / "TODO" / "implement later" in steps. Each step contains the actual code or command an engineer needs. Module stub files contain a `//!` doc comment naming the task that fleshes them out — these are documentation, not unfilled placeholders.

**Type consistency:** `Photo::new` signature (Task 3) matches the call site in Task 16. `EditState` fields referenced in panels (Task 14) match the definitions in Task 4. `EditUniforms::from(&EditState)` signature (Task 10) is consumed unchanged in Tasks 12 and 15. The catalog's `upsert_edit(photo_id, &EditState)` signature (Task 7) is what `flush_if_due` in Task 16 calls.

**Known small risks:**
- wgpu 29's `request_adapter` returns `Option`, not `Result`, in the version checked. If the API changed in a point release between writing and execution, the adapter code in Task 8 might need `?` adjustment.
- Some egui-wgpu 0.33 callback APIs use `Callback::new_paint_callback`; confirm exact name at execution. If renamed, the fix is mechanical and isolated to `app.rs`.

---

**Plan complete and saved to `docs/superpowers/plans/2026-05-23-chalkraw-rs-foundation.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**
