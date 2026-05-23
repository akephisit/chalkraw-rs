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
            egui::MenuBar::new().ui(ui, |ui| {
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
