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

    // ── Basic ────────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Basic")
        .default_open(true)
        .show(ui, |ui| {
            // White Balance — shown first, matching Lightroom order.
            ui.label("Temp (K)");
            changed |= ui.add(
                egui::Slider::new(&mut edit.white_balance.temp_kelvin, 2000.0..=10000.0)
                    .fixed_decimals(0)
                    .suffix(" K"),
            ).changed();

            ui.label("Tint");
            changed |= ui.add(
                egui::Slider::new(&mut edit.white_balance.tint, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.add_space(4.0);

            ui.label("Exposure");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.exposure, -5.0..=5.0)
                    .fixed_decimals(2),
            ).changed();

            ui.label("Contrast");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.contrast, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Highlights");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.highlights, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Shadows");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.shadows, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Whites");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.whites, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Blacks");
            changed |= ui.add(
                egui::Slider::new(&mut edit.tone.blacks, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();
        });

    // ── Presence (multi-pass — Phase 2E) ─────────────────────────────────────
    egui::CollapsingHeader::new("Presence")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2E)"); });

    // ── Color ─────────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Color")
        .default_open(true)
        .show(ui, |ui| {
            ui.label("Vibrance");
            changed |= ui.add(
                egui::Slider::new(&mut edit.color.vibrance, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Saturation");
            changed |= ui.add(
                egui::Slider::new(&mut edit.color.saturation, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();
        });

    // ── Tone Curve (Phase 2D) ─────────────────────────────────────────────────
    egui::CollapsingHeader::new("Tone Curve")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2D)"); });

    // ── HSL (Phase 2B) ────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("HSL")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2B)"); });

    // ── Color Grading (Phase 2C) ──────────────────────────────────────────────
    egui::CollapsingHeader::new("Color Grading")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2C)"); });

    // ── Detail (Phase 2E) ─────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Detail")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2E)"); });

    // ── Effects ───────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Effects")
        .default_open(false)
        .show(ui, |ui| {
            ui.strong("Vignette");
            ui.label("Amount");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.vignette.amount, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Midpoint");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.vignette.midpoint, 0.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Feather");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.vignette.feather, 0.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Roundness");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.vignette.roundness, -100.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.add_space(6.0);
            ui.strong("Grain");
            ui.label("Amount");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.grain.amount, 0.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Size");
            changed |= ui.add(
                egui::Slider::new(&mut edit.effects.grain.size, 0.0..=100.0)
                    .fixed_decimals(0),
            ).changed();

            ui.label("Roughness");
            // Roughness is wired through the uniform buffer but has no shader
            // effect in Phase 2A. The slider is active so edits are preserved;
            // the tooltip explains the current state.
            let roughness_resp = ui.add(
                egui::Slider::new(&mut edit.effects.grain.roughness, 0.0..=100.0)
                    .fixed_decimals(0),
            );
            let roughness_changed = roughness_resp.changed();
            roughness_resp.on_hover_text("Roughness (multi-octave noise — Phase 2E)");
            changed |= roughness_changed;
        });

    // ── Lens Correction (Phase 2F) ────────────────────────────────────────────
    egui::CollapsingHeader::new("Lens Correction")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2F)"); });

    // ── Geometry / Crop (Phase 2F) ────────────────────────────────────────────
    egui::CollapsingHeader::new("Geometry")
        .default_open(false)
        .show(ui, |ui| { ui.label("(Phase 2F)"); });

    changed
}
