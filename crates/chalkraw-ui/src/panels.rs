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
