# chalkraw-rs Library + Develop Usability Plan

> **For agentic workers:** Execute this plan task-by-task. Keep checkboxes current as
> work lands. Every code-touching task should end with `cargo fmt --check`,
> `cargo check --workspace --all-targets`, and
> `cargo clippy --workspace --all-targets -- -D warnings`. Docs-only edits do not
> need cargo verification.

**Date:** 2026-05-30

**Goal:** Turn the partially implemented Phase 2/3 functionality into a usable
editing workflow: import photos, filter/navigate the library, make develop edits,
persist them reliably, and export the selected batch without placeholder menus or
dead controls.

**Why this plan exists:** The existing foundation plan is Phase 1 only. The codebase
has already moved beyond it: develop shaders and controls, import/watch-folder,
filmstrip, presets, watermark presets, RAW decode, and batch export exist in
partial form. The immediate problem is not crate scaffolding; it is usability and
wiring.

**Current state snapshot:**

- Phase 1 is complete and tagged through the normal versioned release flow.
- Develop controls exist in the right panel for Basic, Presence, Color, Tone Curve,
  HSL, Color Grading, Detail, Effects, Lens Correction, and Geometry.
- Library import, folder import, watch folder, filmstrip thumbnails, flags, and
  batch export exist.
- Before this plan, the top `Library` and `Develop` menus were still placeholders,
  and left-panel Collections were label-only.
- Working tree currently has UI wiring changes in:
  - `crates/chalkraw-ui/src/app.rs`
  - `crates/chalkraw-ui/src/panels.rs`
- This plan file is also new and should be committed with the first usability
  wiring commit unless intentionally split into a docs-only commit.

---

## Task 0: Land Current UI Wiring

**Purpose:** Preserve the already-started fix that makes Library/Develop visible and
usable from the UI instead of only from side panels or shortcuts.

- [ ] Verify the current working-tree changes:
  - `Library` menu has Import Photos, Import Folder, All Photos, Picks, Rejected.
  - `Develop` menu has Reset All Edits, Reset Crop, Reset Zoom.
  - Left-panel Collections are clickable and show counts.
  - Filmstrip and keyboard navigation use the active folder/collection filter.
- [ ] Run:
  - `cargo fmt --check`
  - `cargo check --workspace --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Commit:
  - `git add crates/chalkraw-ui/src/app.rs crates/chalkraw-ui/src/panels.rs docs/superpowers/plans/2026-05-30-library-develop-usability.md`
  - `git commit -m "Wire library and develop UI menus"`

## Task 1: Make Library Workflow Reliable

**Purpose:** A user should be able to import a folder, find photos, flag them, and
navigate without guessing what is happening.

- [ ] Add a visible import result summary:
  - Introduce an `ImportReport` / `ImportSummary` model before changing UI copy.
  - Track scanned count, decoded count, duplicate count, inserted count, failed count.
  - Track the first few failures as `(path, reason)` for display.
- [ ] Replace the current import completion path:
  - `process_import_paths` should return candidates plus report data, not only
    successful candidates.
  - `finish_import_if_ready` should display the report instead of only
    `Imported {n} new photos`.
  - Duplicates found during final insertion should be reflected in the summary,
    because concurrent import/watch-folder scans can discover already-known hashes.
- [ ] Add catalog APIs required by selected-photo actions:
  - `remove_photo(photo_id)`.
  - `remove_edit(photo_id)` or `remove_photo_with_edit(photo_id)`.
  - `update_photo_path(photo_id, new_path, new_hash, width, height, format, thumbnail)`.
  - tests proving remove does not delete the original source file.
- [ ] Add catalog actions for the selected/current photo:
  - Remove from catalog, without deleting the original file.
  - Reveal missing/original path state in the UI.
  - Relink missing file by choosing a replacement path.
- [ ] Make folder and collection filters compose clearly:
  - show active filter text near the filmstrip
  - add one-click clear filters action
  - keep navigation scoped to visible photos
- [ ] Add tests around library state:
  - import duplicate filtering
  - flag filtering
  - navigation within filtered photos
  - removing a photo keeps original file untouched

## Task 2: Finish Develop Usability

**Purpose:** Phase 2 should feel complete from the app, not only from shader tests.

- [ ] Remove or replace remaining placeholder UI:
  - `Histogram (Phase 2)` should become either a working histogram or be hidden.
  - crop text that says drag-rectangle UI is coming should be replaced by a real
    interaction or downgraded to a normal slider-only mode.
- [ ] Add per-section reset buttons:
  - Basic
  - Color
  - Tone Curve
  - Detail
  - Effects
  - Lens/Geometry
- [ ] Add before/after preview toggle:
  - hold or press a key to temporarily render identity edits
  - keep current edit state unchanged
- [ ] Audit each active slider:
  - if it affects the shader/export, keep it active
  - if it is stored but has no effect, disable it or label it as not yet active
  - remove stale comments/tooltips for sliders that now affect the shader; Grain
    Roughness is currently wired in WGSL, but the UI comments still describe it as
    no-effect/stored-only
- [ ] Add tests or golden coverage for high-risk controls:
  - crop enable/disable
  - lens correction
  - point curve LUT
  - detail/noise-reduction blur inputs

## Task 3: Persist and Restore Full Session State

**Purpose:** Editing should survive restart in a way users can trust.

- [ ] Verify edit autosave for every Develop section, not only Exposure.
- [ ] Persist current flag and confirm the filmstrip reflects it after restart.
- [ ] Choose the storage location before writing code:
  - Prefer a new redb app/session table if the state is catalog-specific.
  - Prefer a local config file only for machine/window preferences that should not
    travel with the catalog.
  - Document the decision in code comments or this plan before implementation.
- [ ] Persist enough app state for a practical session:
  - last selected photo
  - active folder/collection filter
  - optional watch folder
- [ ] Add restart-roundtrip tests using a temporary catalog.

## Task 4: RAW and Export Validation

**Purpose:** Avoid shipping a release where JPEG works but RAW/export silently diverge.

- [ ] Add a small RAW validation path:
  - at least one documented manual sample if a fixture cannot be committed
  - decode result dimensions and format checks
  - clear UI error when RAW decode is unsupported
- [ ] Compare UI render and export output for representative edits:
  - exposure/basic tone
  - HSL/color grading
  - clarity/sharpening/noise reduction
  - crop/lens correction
  - Use a fixed fixture and compare CPU-readback pixels with a documented tolerance
    per channel/backend. Start with a small set of 64x64 or 128x128 fixtures so the
    tests remain fast.
  - Record whether the comparison is exact, tolerance-based, or manual-only for
    each edit group.
- [ ] Improve export progress/error reporting:
  - show failed item names
  - keep completed output paths available after export
  - handle empty export selection explicitly

## Task 5: Release Readiness

**Purpose:** Build confidence before the next `v*` tag.

- [ ] Run full local validation:
  - `cargo fmt --check`
  - `cargo check --workspace --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `cargo build --workspace --release`
- [ ] Manual app checklist:
  - import folder
  - flag picks/rejects
  - filter by folder and collection
  - edit a photo across multiple develop sections
  - restart and confirm edits persist
  - batch export all photos and picks-only
  - open exported files
- [ ] Bump version only when the checklist passes.
- [ ] Tag using the release workflow convention:
  - `v0.25.3` or the next appropriate semver patch
- [ ] Push commit and tag, then verify:
  - CI on `main` succeeds
  - Release workflow on `v*` tag succeeds
  - GitHub Release contains Linux, macOS, and Windows assets

---

## Done Criteria

- [ ] No top-level `Library` or `Develop` menu is a placeholder.
- [ ] Library import/filter/navigation/flagging are usable without keyboard-only
      knowledge.
- [ ] Develop controls either affect preview/export or are clearly disabled.
- [ ] Edits and flags persist after restart.
- [ ] Batch export works for all photos and picks-only.
- [ ] `cargo fmt`, `cargo check`, `cargo clippy`, and `cargo test` pass.
- [ ] A versioned `v*` tag triggers a successful GitHub Release build.
