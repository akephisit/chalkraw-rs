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
        .show(ui, |ui| {
            egui::CollapsingHeader::new("Parametric")
                .id_salt("tc_parametric")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label("Highlights");
                    if ui.add(egui::Slider::new(&mut edit.parametric_curve.highlights, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Lights");
                    if ui.add(egui::Slider::new(&mut edit.parametric_curve.lights, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Darks");
                    if ui.add(egui::Slider::new(&mut edit.parametric_curve.darks, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Shadows");
                    if ui.add(egui::Slider::new(&mut edit.parametric_curve.shadows, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                });
            ui.add_space(4.0);
            ui.label("Point curve editor — coming in a later polish phase");
        });

    // ── HSL (Phase 2B) ────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("HSL")
        .default_open(false)
        .show(ui, |ui| {
            let names: [(&str, egui::Color32); 8] = [
                ("Red",     egui::Color32::from_rgb(220, 60, 60)),
                ("Orange",  egui::Color32::from_rgb(230, 140, 50)),
                ("Yellow",  egui::Color32::from_rgb(230, 220, 50)),
                ("Green",   egui::Color32::from_rgb(80, 200, 80)),
                ("Aqua",    egui::Color32::from_rgb(80, 200, 220)),
                ("Blue",    egui::Color32::from_rgb(80, 120, 230)),
                ("Purple",  egui::Color32::from_rgb(160, 80, 230)),
                ("Magenta", egui::Color32::from_rgb(220, 80, 200)),
            ];
            for (i, (name, swatch)) in names.iter().enumerate() {
                let header = egui::RichText::new(*name).color(*swatch);
                egui::CollapsingHeader::new(header)
                    .id_salt(format!("hsl_{i}"))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label("Hue");
                        if ui.add(
                            egui::Slider::new(&mut edit.hsl[i].hue, -100.0..=100.0)
                                .fixed_decimals(0),
                        ).changed() {
                            changed = true;
                        }
                        ui.label("Saturation");
                        if ui.add(
                            egui::Slider::new(&mut edit.hsl[i].saturation, -100.0..=100.0)
                                .fixed_decimals(0),
                        ).changed() {
                            changed = true;
                        }
                        ui.label("Luminance");
                        if ui.add(
                            egui::Slider::new(&mut edit.hsl[i].luminance, -100.0..=100.0)
                                .fixed_decimals(0),
                        ).changed() {
                            changed = true;
                        }
                    });
            }
        });

    // ── Color Grading (Phase 2C) ──────────────────────────────────────────────
    egui::CollapsingHeader::new("Color Grading")
        .default_open(false)
        .show(ui, |ui| {
            // Shadows
            egui::CollapsingHeader::new("Shadows")
                .id_salt("cg_0")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Hue");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.shadows.hue, 0.0..=360.0).fixed_decimals(0).suffix("°")).changed() { changed = true; }
                    ui.label("Saturation");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.shadows.saturation, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Luminance");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.shadows.luminance, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                });
            // Midtones
            egui::CollapsingHeader::new("Midtones")
                .id_salt("cg_1")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Hue");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.midtones.hue, 0.0..=360.0).fixed_decimals(0).suffix("°")).changed() { changed = true; }
                    ui.label("Saturation");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.midtones.saturation, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Luminance");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.midtones.luminance, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                });
            // Highlights
            egui::CollapsingHeader::new("Highlights")
                .id_salt("cg_2")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Hue");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.highlights.hue, 0.0..=360.0).fixed_decimals(0).suffix("°")).changed() { changed = true; }
                    ui.label("Saturation");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.highlights.saturation, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Luminance");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.highlights.luminance, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                });
            // Global
            egui::CollapsingHeader::new("Global")
                .id_salt("cg_3")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Hue");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.global.hue, 0.0..=360.0).fixed_decimals(0).suffix("°")).changed() { changed = true; }
                    ui.label("Saturation");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.global.saturation, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                    ui.label("Luminance");
                    if ui.add(egui::Slider::new(&mut edit.color_grading.global.luminance, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
                });
            ui.separator();
            ui.label("Blending");
            if ui.add(egui::Slider::new(&mut edit.color_grading.blending, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
            ui.label("Balance");
            if ui.add(egui::Slider::new(&mut edit.color_grading.balance, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
        });

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
        .show(ui, |ui| {
            ui.label("Distortion");
            if ui.add(egui::Slider::new(&mut edit.lens_correction.distortion, -100.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
            ui.label("Vignetting (correction)");
            if ui.add(egui::Slider::new(&mut edit.lens_correction.vignetting, 0.0..=100.0).fixed_decimals(0)).changed() { changed = true; }
        });

    // ── Geometry / Crop (Phase 2F) ────────────────────────────────────────────
    egui::CollapsingHeader::new("Geometry")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = edit.crop.is_some();
            let was_enabled = enabled;
            if ui.checkbox(&mut enabled, "Crop enabled").changed() { changed = true; }
            if enabled && !was_enabled {
                // Initialise default crop to full image so the slider state is sane.
                edit.crop = Some(chalkraw_core::Crop {
                    x_pct: 0.0, y_pct: 0.0, w_pct: 1.0, h_pct: 1.0, rotation_deg: 0.0,
                });
            } else if !enabled && was_enabled {
                edit.crop = None;
            }
            if let Some(crop) = edit.crop.as_mut() {
                ui.label("X");
                if ui.add(egui::Slider::new(&mut crop.x_pct, 0.0..=1.0).fixed_decimals(2)).changed() { changed = true; }
                ui.label("Y");
                if ui.add(egui::Slider::new(&mut crop.y_pct, 0.0..=1.0).fixed_decimals(2)).changed() { changed = true; }
                ui.label("Width");
                if ui.add(egui::Slider::new(&mut crop.w_pct, 0.01..=1.0).fixed_decimals(2)).changed() { changed = true; }
                ui.label("Height");
                if ui.add(egui::Slider::new(&mut crop.h_pct, 0.01..=1.0).fixed_decimals(2)).changed() { changed = true; }
                ui.label("Rotation");
                if ui.add(egui::Slider::new(&mut crop.rotation_deg, -45.0..=45.0).fixed_decimals(1).suffix("°")).changed() { changed = true; }
            }
            ui.add_space(4.0);
            ui.label("Drag-rectangle crop UI — coming with Phase 3 import flow");
        });

    changed
}
