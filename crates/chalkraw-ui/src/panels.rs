use chalkraw_core::{interpolate_curve, EditState};
use egui::Ui;

#[derive(Debug, Clone, Copy, Default)]
pub struct EditChange {
    pub uniforms: bool,
    pub tone_curve: bool,
    pub blur_inputs: bool,
}

impl EditChange {
    pub fn all() -> Self {
        Self {
            uniforms: true,
            tone_curve: true,
            blur_inputs: true,
        }
    }

    pub fn any(self) -> bool {
        self.uniforms || self.tone_curve || self.blur_inputs
    }

    pub fn merge(&mut self, other: Self) {
        self.uniforms |= other.uniforms;
        self.tone_curve |= other.tone_curve;
        self.blur_inputs |= other.blur_inputs;
    }
}

// ── Scroll-aware slider helpers ───────────────────────────────────────────────

/// Wrap an egui Slider with mouse-wheel-while-hovered support. Returns true if
/// the value changed (either by drag, keyboard, or scroll wheel).
fn slider_scroll(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) -> bool {
    let slider = egui::Slider::new(value, range.clone()).fixed_decimals(decimals);
    let response = ui.add(slider);
    let mut changed = response.changed();
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let span = range.end() - range.start();
            let direction = if scroll > 0.0 { 1.0_f32 } else { -1.0 };
            // One scroll notch ≈ 1% of the range, scaled by the actual magnitude.
            let step = (scroll.abs() / 50.0).max(0.01) * span * 0.01;
            let new_val = (*value + direction * step).clamp(*range.start(), *range.end());
            if (new_val - *value).abs() > f32::EPSILON {
                *value = new_val;
                changed = true;
            }
            // Consume the scroll event so the right-panel ScrollArea doesn't also
            // process it. Without this, the panel scrolls simultaneously with the
            // slider when hovering a slider widget.
            ui.input_mut(|i| {
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                i.raw_scroll_delta = egui::Vec2::ZERO;
            });
        }
    }
    changed
}

/// Variant with a suffix (e.g. "°", " K", " px").
fn slider_scroll_suffix(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
    suffix: &str,
) -> bool {
    let slider = egui::Slider::new(value, range.clone())
        .fixed_decimals(decimals)
        .suffix(suffix);
    let response = ui.add(slider);
    let mut changed = response.changed();
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let span = range.end() - range.start();
            let direction = if scroll > 0.0 { 1.0_f32 } else { -1.0 };
            let step = (scroll.abs() / 50.0).max(0.01) * span * 0.01;
            let new_val = (*value + direction * step).clamp(*range.start(), *range.end());
            if (new_val - *value).abs() > f32::EPSILON {
                *value = new_val;
                changed = true;
            }
            // Consume the scroll event so the right-panel ScrollArea doesn't also
            // process it.
            ui.input_mut(|i| {
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                i.raw_scroll_delta = egui::Vec2::ZERO;
            });
        }
    }
    changed
}

// ── Colour Wheel Widget ───────────────────────────────────────────────────────

fn hsv_to_color32(h_deg: f32, s: f32, v: f32) -> egui::Color32 {
    let h = h_deg / 60.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Interactive colour wheel + hue/saturation readout.
///
/// Draws a 130×130 hue-spectrum disc; the draggable dot's angle encodes hue
/// (0..360°) and its distance from centre encodes saturation (0..100).
/// Right-click resets to neutral (H=0, S=0).  Returns `true` when changed.
pub fn color_wheel_widget(ui: &mut egui::Ui, grade: &mut chalkraw_core::GradeTone) -> bool {
    let mut changed = false;
    let desired = egui::vec2(140.0, 160.0); // 140 for wheel + 20 for label
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());

    let wheel_size = 130.0;
    let wheel_rect = egui::Rect::from_center_size(
        rect.center_top() + egui::vec2(0.0, wheel_size * 0.5 + 5.0),
        egui::vec2(wheel_size, wheel_size),
    );
    let centre = wheel_rect.center();
    let radius = wheel_size * 0.5;

    let painter = ui.painter();

    // Draw the colour wheel via discrete arc segments.
    let segments = 64;
    for i in 0..segments {
        let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let hue_mid = (a0 + a1) * 0.5 / std::f32::consts::TAU * 360.0;
        let inner_p0 = centre;
        let outer_p0 = centre + egui::vec2(a0.cos(), a0.sin()) * radius;
        let outer_p1 = centre + egui::vec2(a1.cos(), a1.sin()) * radius;
        let outer_colour = hsv_to_color32(hue_mid, 1.0, 1.0);
        // Single triangle from centre to outer edge.
        painter.add(egui::Shape::convex_polygon(
            vec![inner_p0, outer_p0, outer_p1],
            outer_colour,
            egui::Stroke::NONE,
        ));
    }

    // Overlay a small grey circle at the centre to simulate S=0 (grey).
    let grey_overlay = egui::Color32::from_rgba_unmultiplied(128, 128, 128, 180);
    painter.circle_filled(centre, radius * 0.15, grey_overlay);

    // Compute dot position from grade.hue + grade.saturation.
    let hue_rad = (grade.hue / 360.0) * std::f32::consts::TAU;
    let sat_norm = (grade.saturation / 100.0).clamp(0.0, 1.0);
    let dot_pos = centre + egui::vec2(hue_rad.cos(), hue_rad.sin()) * (radius * sat_norm);
    painter.circle_stroke(dot_pos, 5.0, egui::Stroke::new(2.0, egui::Color32::WHITE));
    painter.circle_stroke(dot_pos, 5.0, egui::Stroke::new(1.0, egui::Color32::BLACK));

    // Handle drag.
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let from_centre = pos - centre;
            let dist = from_centre.length();
            let normalised = (dist / radius).clamp(0.0, 1.0);
            grade.saturation = normalised * 100.0;
            let angle = from_centre.y.atan2(from_centre.x);
            let mut hue_deg = angle.to_degrees();
            if hue_deg < 0.0 {
                hue_deg += 360.0;
            }
            grade.hue = hue_deg;
            changed = true;
        }
    }
    // Right-click to reset.
    if response.secondary_clicked() {
        grade.hue = 0.0;
        grade.saturation = 0.0;
        changed = true;
    }

    // Label below the wheel.
    let label_pos = rect.left_bottom() + egui::vec2(5.0, -5.0);
    painter.text(
        label_pos,
        egui::Align2::LEFT_BOTTOM,
        format!("H {:.0}° S {:.0}", grade.hue, grade.saturation),
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );

    changed
}

// ── Point Curve Widget ────────────────────────────────────────────────────────

/// Interactive 200×200 point curve editor.
///
/// Plots the current `curve` (piecewise-linear interpolation between control
/// points) on a dark background with a grid overlay.  Supports:
/// - Drag a control point to move it.
/// - Click on empty space to add a control point.
/// - Right-click a middle control point to delete it.
///
/// The first and last points are pinned to x=0 and x=1; only their y is
/// draggable.  Middle points cannot cross their neighbours.
///
/// Returns `true` when the curve was modified.
pub fn point_curve_widget(ui: &mut egui::Ui, curve: &mut chalkraw_core::Curve) -> bool {
    let mut changed = false;
    let desired_size = egui::vec2(200.0, 200.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    // Background and grid.
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(30));
    let grid_color = egui::Color32::from_gray(70);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.left() + rect.width() * t;
        let y = rect.top() + rect.height() * t;
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(0.5, grid_color),
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(0.5, grid_color),
        );
    }
    // Diagonal reference (identity line).
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(0.5, egui::Color32::from_gray(50)),
    );

    // Helpers: map curve (x, y) ∈ [0, 1] ↔ screen coordinates.
    let to_screen = |p: chalkraw_core::CurvePoint| -> egui::Pos2 {
        egui::pos2(
            rect.left() + p.x * rect.width(),
            rect.bottom() - p.y * rect.height(),
        )
    };
    let to_curve = |pos: egui::Pos2| -> chalkraw_core::CurvePoint {
        chalkraw_core::CurvePoint {
            x: ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            y: ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
        }
    };

    // Draw the interpolated curve (64 segments).
    let mut prev_screen = to_screen(chalkraw_core::CurvePoint {
        x: 0.0,
        y: interpolate_curve(&curve.0, 0.0),
    });
    for i in 1..=64 {
        let x = i as f32 / 64.0;
        let y = interpolate_curve(&curve.0, x);
        let cur = to_screen(chalkraw_core::CurvePoint { x, y });
        painter.line_segment(
            [prev_screen, cur],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 220, 100)),
        );
        prev_screen = cur;
    }

    // Draw control-point circles and handle drag.
    let hover_radius = 8.0;
    // For drag: find the point closest to where the drag started.  We remember
    // the active index across frames using the egui memory (keyed by widget id).
    let widget_id = response.id;
    let mut dragged_idx: Option<usize> = ui.memory(|m| m.data.get_temp::<usize>(widget_id));

    // On drag release, clear the stored index.
    if response.drag_stopped() {
        ui.memory_mut(|m| m.data.remove::<usize>(widget_id));
        dragged_idx = None;
    }

    for p in curve.0.iter() {
        let screen_p = to_screen(*p);
        painter.circle_filled(screen_p, 5.0, egui::Color32::from_rgb(255, 200, 60));
        painter.circle_stroke(
            screen_p,
            5.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 150, 30)),
        );
    }

    if response.drag_started() {
        // On the first frame of a drag, pick the closest point.
        if let Some(drag_pos) = response.interact_pointer_pos() {
            let mut best: Option<(usize, f32)> = None;
            for (idx, p) in curve.0.iter().enumerate() {
                let d = (to_screen(*p) - drag_pos).length();
                if d < hover_radius * 2.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((idx, d));
                }
            }
            if let Some((idx, _)) = best {
                ui.memory_mut(|m| m.data.insert_temp(widget_id, idx));
                dragged_idx = Some(idx);
            }
        }
    }

    if response.dragged() {
        if let Some(idx) = dragged_idx {
            if let Some(pos) = response.interact_pointer_pos() {
                let mut new_p = to_curve(pos);
                // Pin x of first and last points.
                if idx == 0 {
                    new_p.x = 0.0;
                } else if idx == curve.0.len() - 1 {
                    new_p.x = 1.0;
                } else {
                    // Middle points: prevent crossing neighbours.
                    new_p.x = new_p
                        .x
                        .clamp(curve.0[idx - 1].x + 0.01, curve.0[idx + 1].x - 0.01);
                }
                curve.0[idx] = new_p;
                changed = true;
            }
        }
    }

    // Click on empty space → add a control point.
    if response.clicked() {
        if let Some(click_pos) = response.interact_pointer_pos() {
            let cp = to_curve(click_pos);
            // Only add if not too close to an existing point.
            let too_close = curve.0.iter().any(|p| (p.x - cp.x).abs() < 0.02);
            if !too_close {
                curve.0.push(cp);
                curve
                    .0
                    .sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                changed = true;
            }
        }
    }

    // Right-click a middle point → delete it (keep at least 2 points).
    if response.secondary_clicked() {
        if let Some(click_pos) = response.interact_pointer_pos() {
            let mut to_remove: Option<usize> = None;
            for (idx, p) in curve.0.iter().enumerate() {
                // Never delete the first or last point.
                if idx == 0 || idx == curve.0.len() - 1 {
                    continue;
                }
                if (to_screen(*p) - click_pos).length() < hover_radius {
                    to_remove = Some(idx);
                    break;
                }
            }
            if let Some(idx) = to_remove {
                curve.0.remove(idx);
                changed = true;
            }
        }
    }

    changed
}

// ── Panels ────────────────────────────────────────────────────────────────────

pub fn left_panel(ui: &mut Ui, state: &mut crate::app::AppState) -> bool {
    let mut changed = false;

    ui.heading("Catalog");
    ui.separator();
    ui.label("Folders");
    ui.indent("folders", |ui| {
        let mut folder_summary: std::collections::BTreeMap<std::path::PathBuf, usize> =
            std::collections::BTreeMap::new();
        for p in &state.photos_cache {
            if let Some(parent) = p.original_path.parent() {
                *folder_summary.entry(parent.to_path_buf()).or_insert(0) += 1;
            }
        }
        if folder_summary.is_empty() {
            ui.label("(no photos imported yet)");
        } else {
            let all_count: usize = folder_summary.values().sum();
            if ui
                .selectable_label(state.folder_filter.is_none(), format!("All ({all_count})"))
                .clicked()
            {
                state.folder_filter = None;
            }
            let mut new_filter = state.folder_filter.clone();
            for (folder, count) in &folder_summary {
                let label = folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.display().to_string());
                let is_active = state.folder_filter.as_ref() == Some(folder);
                if ui
                    .selectable_label(is_active, format!("{label} ({count})"))
                    .clicked()
                {
                    new_filter = Some(folder.clone());
                }
            }
            state.folder_filter = new_filter;
        }
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
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.new_preset_name)
                    .desired_width(120.0)
                    .hint_text("preset name"),
            );
            if ui.button("Save").clicked() {
                let name = std::mem::take(&mut state.new_preset_name);
                let name = name.trim().to_string();
                if !name.is_empty() {
                    if let Err(e) = state.save_preset(name) {
                        log::warn!("save preset failed: {e}");
                    }
                }
            }
        });
        let presets = state.catalog.list_presets().unwrap_or_default();
        if presets.is_empty() {
            ui.label("(none yet)");
        } else {
            // Collect ids and names first to avoid borrow conflicts when calling
            // state methods (which take &mut self) while iterating over presets
            // (which borrows state.catalog).
            let presets_view: Vec<(chalkraw_core::PresetId, String)> =
                presets.into_iter().map(|p| (p.id, p.name)).collect();
            let mut to_apply: Option<chalkraw_core::PresetId> = None;
            let mut to_delete: Option<chalkraw_core::PresetId> = None;
            for (id, name) in &presets_view {
                ui.horizontal(|ui| {
                    if ui.button(name).clicked() {
                        to_apply = Some(*id);
                    }
                    if ui.small_button("✕").clicked() {
                        to_delete = Some(*id);
                    }
                });
            }
            if let Some(id) = to_apply {
                if let Err(e) = state.apply_preset(id) {
                    log::warn!("apply preset failed: {e}");
                } else {
                    changed = true;
                }
            }
            if let Some(id) = to_delete {
                if let Err(e) = state.delete_preset(id) {
                    log::warn!("delete preset failed: {e}");
                }
            }
        }
    });

    changed
}

pub fn right_panel(ui: &mut Ui, edit: &mut EditState) -> EditChange {
    let mut changed = false;
    let before_tone_curve = edit.tone_curve.rgb.clone();
    let before_sharpening_radius = edit.detail.sharpening.radius;
    let before_noise_reduction = edit.detail.noise_reduction;

    ui.heading("Develop");
    ui.separator();

    egui::CollapsingHeader::new("Histogram")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("(Phase 2)");
        });

    // ── Basic ────────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Basic")
        .default_open(true)
        .show(ui, |ui| {
            // White Balance — shown first, matching Lightroom order.
            ui.label("Temp (K)");
            changed |= slider_scroll_suffix(
                ui,
                &mut edit.white_balance.temp_kelvin,
                2000.0..=10000.0,
                0,
                " K",
            );

            ui.label("Tint");
            changed |= slider_scroll(ui, &mut edit.white_balance.tint, -100.0..=100.0, 0);

            ui.add_space(4.0);

            ui.label("Exposure");
            changed |= slider_scroll(ui, &mut edit.tone.exposure, -5.0..=5.0, 2);

            ui.label("Contrast");
            changed |= slider_scroll(ui, &mut edit.tone.contrast, -100.0..=100.0, 0);

            ui.label("Highlights");
            changed |= slider_scroll(ui, &mut edit.tone.highlights, -100.0..=100.0, 0);

            ui.label("Shadows");
            changed |= slider_scroll(ui, &mut edit.tone.shadows, -100.0..=100.0, 0);

            ui.label("Whites");
            changed |= slider_scroll(ui, &mut edit.tone.whites, -100.0..=100.0, 0);

            ui.label("Blacks");
            changed |= slider_scroll(ui, &mut edit.tone.blacks, -100.0..=100.0, 0);
        });

    // ── Presence (multi-pass — Phase 2E) ─────────────────────────────────────
    egui::CollapsingHeader::new("Presence")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("Texture");
            if slider_scroll(ui, &mut edit.presence.texture, -100.0..=100.0, 0) {
                changed = true;
            }
            ui.label("Clarity");
            if slider_scroll(ui, &mut edit.presence.clarity, -100.0..=100.0, 0) {
                changed = true;
            }
            ui.label("Dehaze");
            if slider_scroll(ui, &mut edit.presence.dehaze, -100.0..=100.0, 0) {
                changed = true;
            }
        });

    // ── Color ─────────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Color")
        .default_open(true)
        .show(ui, |ui| {
            ui.label("Vibrance");
            changed |= slider_scroll(ui, &mut edit.color.vibrance, -100.0..=100.0, 0);

            ui.label("Saturation");
            changed |= slider_scroll(ui, &mut edit.color.saturation, -100.0..=100.0, 0);
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
                    if slider_scroll(ui, &mut edit.parametric_curve.highlights, -100.0..=100.0, 0) {
                        changed = true;
                    }
                    ui.label("Lights");
                    if slider_scroll(ui, &mut edit.parametric_curve.lights, -100.0..=100.0, 0) {
                        changed = true;
                    }
                    ui.label("Darks");
                    if slider_scroll(ui, &mut edit.parametric_curve.darks, -100.0..=100.0, 0) {
                        changed = true;
                    }
                    ui.label("Shadows");
                    if slider_scroll(ui, &mut edit.parametric_curve.shadows, -100.0..=100.0, 0) {
                        changed = true;
                    }
                });
            egui::CollapsingHeader::new("Point Curve")
                .id_salt("tc_point")
                .default_open(false)
                .show(ui, |ui| {
                    if point_curve_widget(ui, &mut edit.tone_curve.rgb) {
                        changed = true;
                    }
                    if ui.button("Reset to Linear").clicked() {
                        edit.tone_curve.rgb = chalkraw_core::Curve::default();
                        changed = true;
                    }
                });
        });

    // ── HSL (Phase 2B) ────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("HSL")
        .default_open(false)
        .show(ui, |ui| {
            let names: [(&str, egui::Color32); 8] = [
                ("Red", egui::Color32::from_rgb(220, 60, 60)),
                ("Orange", egui::Color32::from_rgb(230, 140, 50)),
                ("Yellow", egui::Color32::from_rgb(230, 220, 50)),
                ("Green", egui::Color32::from_rgb(80, 200, 80)),
                ("Aqua", egui::Color32::from_rgb(80, 200, 220)),
                ("Blue", egui::Color32::from_rgb(80, 120, 230)),
                ("Purple", egui::Color32::from_rgb(160, 80, 230)),
                ("Magenta", egui::Color32::from_rgb(220, 80, 200)),
            ];
            for (i, (name, swatch)) in names.iter().enumerate() {
                let header = egui::RichText::new(*name).color(*swatch);
                egui::CollapsingHeader::new(header)
                    .id_salt(format!("hsl_{i}"))
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.label("Hue");
                        if slider_scroll(ui, &mut edit.hsl[i].hue, -100.0..=100.0, 0) {
                            changed = true;
                        }
                        ui.label("Saturation");
                        if slider_scroll(ui, &mut edit.hsl[i].saturation, -100.0..=100.0, 0) {
                            changed = true;
                        }
                        ui.label("Luminance");
                        if slider_scroll(ui, &mut edit.hsl[i].luminance, -100.0..=100.0, 0) {
                            changed = true;
                        }
                    });
            }
        });

    // ── Color Grading (Phase 2C) ──────────────────────────────────────────────
    egui::CollapsingHeader::new("Color Grading")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("Shadows");
            if color_wheel_widget(ui, &mut edit.color_grading.shadows) {
                changed = true;
            }
            ui.label("Shadows Luminance");
            if slider_scroll(
                ui,
                &mut edit.color_grading.shadows.luminance,
                -100.0..=100.0,
                0,
            ) {
                changed = true;
            }
            ui.separator();
            ui.label("Midtones");
            if color_wheel_widget(ui, &mut edit.color_grading.midtones) {
                changed = true;
            }
            ui.label("Midtones Luminance");
            if slider_scroll(
                ui,
                &mut edit.color_grading.midtones.luminance,
                -100.0..=100.0,
                0,
            ) {
                changed = true;
            }
            ui.separator();
            ui.label("Highlights");
            if color_wheel_widget(ui, &mut edit.color_grading.highlights) {
                changed = true;
            }
            ui.label("Highlights Luminance");
            if slider_scroll(
                ui,
                &mut edit.color_grading.highlights.luminance,
                -100.0..=100.0,
                0,
            ) {
                changed = true;
            }
            ui.separator();
            ui.label("Global");
            if color_wheel_widget(ui, &mut edit.color_grading.global) {
                changed = true;
            }
            ui.label("Global Luminance");
            if slider_scroll(
                ui,
                &mut edit.color_grading.global.luminance,
                -100.0..=100.0,
                0,
            ) {
                changed = true;
            }
            ui.separator();
            ui.label("Blending");
            if slider_scroll(ui, &mut edit.color_grading.blending, 0.0..=100.0, 0) {
                changed = true;
            }
            ui.label("Balance");
            if slider_scroll(ui, &mut edit.color_grading.balance, -100.0..=100.0, 0) {
                changed = true;
            }
        });

    // ── Detail (Phase 2E) ─────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Detail")
        .default_open(false)
        .show(ui, |ui| {
            ui.strong("Sharpening");
            ui.label("Amount");
            if slider_scroll(ui, &mut edit.detail.sharpening.amount, 0.0..=150.0, 0) {
                changed = true;
            }
            ui.label("Radius");
            if slider_scroll_suffix(ui, &mut edit.detail.sharpening.radius, 0.5..=3.0, 1, " px") {
                changed = true;
            }
            ui.add_space(4.0);
            ui.label("Detail");
            if slider_scroll(ui, &mut edit.detail.sharpening.detail, 0.0..=100.0, 0) {
                changed = true;
            }
            ui.label("Masking");
            if slider_scroll(ui, &mut edit.detail.sharpening.masking, 0.0..=100.0, 0) {
                changed = true;
            }
            ui.add_space(4.0);
            ui.strong("Noise Reduction");
            ui.label("Noise Reduction Luminance");
            if slider_scroll(
                ui,
                &mut edit.detail.noise_reduction.luminance,
                0.0..=100.0,
                0,
            ) {
                changed = true;
            }
            ui.label("Noise Reduction Color");
            if slider_scroll(ui, &mut edit.detail.noise_reduction.color, 0.0..=100.0, 0) {
                changed = true;
            }
        });

    // ── Effects ───────────────────────────────────────────────────────────────
    egui::CollapsingHeader::new("Effects")
        .default_open(false)
        .show(ui, |ui| {
            ui.strong("Vignette");
            ui.label("Amount");
            changed |= slider_scroll(ui, &mut edit.effects.vignette.amount, -100.0..=100.0, 0);

            ui.label("Midpoint");
            changed |= slider_scroll(ui, &mut edit.effects.vignette.midpoint, 0.0..=100.0, 0);

            ui.label("Feather");
            changed |= slider_scroll(ui, &mut edit.effects.vignette.feather, 0.0..=100.0, 0);

            ui.label("Roundness");
            changed |= slider_scroll(ui, &mut edit.effects.vignette.roundness, -100.0..=100.0, 0);

            ui.add_space(6.0);
            ui.strong("Grain");
            ui.label("Amount");
            changed |= slider_scroll(ui, &mut edit.effects.grain.amount, 0.0..=100.0, 0);

            ui.label("Size");
            changed |= slider_scroll(ui, &mut edit.effects.grain.size, 0.0..=100.0, 0);

            ui.label("Roughness");
            // Roughness is wired through the uniform buffer but has no shader
            // effect in Phase 2A. The slider is active so edits are preserved;
            // the tooltip explains the current state.
            let roughness_resp = ui.add(
                egui::Slider::new(&mut edit.effects.grain.roughness, 0.0..=100.0).fixed_decimals(0),
            );
            let roughness_changed = roughness_resp.changed();
            roughness_resp.on_hover_text("Roughness (multi-octave noise — Phase 2E)");
            if roughness_changed {
                // Also handle scroll for roughness — we need the response first
                // for the tooltip, so the scroll logic runs after.
                changed = true;
            }
            // Scroll support for roughness (manually, since we needed the response for tooltip)
            {
                let hovered = ui.rect_contains_pointer(ui.min_rect());
                if hovered {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        let span = 100.0_f32;
                        let direction = if scroll > 0.0 { 1.0_f32 } else { -1.0 };
                        let step = (scroll.abs() / 50.0).max(0.01) * span * 0.01;
                        let new_val =
                            (edit.effects.grain.roughness + direction * step).clamp(0.0, 100.0);
                        if (new_val - edit.effects.grain.roughness).abs() > f32::EPSILON {
                            edit.effects.grain.roughness = new_val;
                            changed = true;
                        }
                    }
                }
            }
        });

    // ── Lens Correction (Phase 2F) ────────────────────────────────────────────
    egui::CollapsingHeader::new("Lens Correction")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("Distortion");
            if slider_scroll(ui, &mut edit.lens_correction.distortion, -100.0..=100.0, 0) {
                changed = true;
            }
            ui.label("Vignetting (correction)");
            if slider_scroll(ui, &mut edit.lens_correction.vignetting, 0.0..=100.0, 0) {
                changed = true;
            }
        });

    // ── Geometry / Crop (Phase 2F) ────────────────────────────────────────────
    egui::CollapsingHeader::new("Geometry")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = edit.crop.is_some();
            let was_enabled = enabled;
            if ui.checkbox(&mut enabled, "Crop enabled").changed() {
                changed = true;
            }
            if enabled && !was_enabled {
                // Initialise default crop to full image so the slider state is sane.
                edit.crop = Some(chalkraw_core::Crop {
                    x_pct: 0.0,
                    y_pct: 0.0,
                    w_pct: 1.0,
                    h_pct: 1.0,
                    rotation_deg: 0.0,
                });
            } else if !enabled && was_enabled {
                edit.crop = None;
            }
            if let Some(crop) = edit.crop.as_mut() {
                ui.label("X");
                if slider_scroll(ui, &mut crop.x_pct, 0.0..=1.0, 2) {
                    changed = true;
                }
                ui.label("Y");
                if slider_scroll(ui, &mut crop.y_pct, 0.0..=1.0, 2) {
                    changed = true;
                }
                ui.label("Width");
                if slider_scroll(ui, &mut crop.w_pct, 0.01..=1.0, 2) {
                    changed = true;
                }
                ui.label("Height");
                if slider_scroll(ui, &mut crop.h_pct, 0.01..=1.0, 2) {
                    changed = true;
                }
                ui.label("Rotation");
                if slider_scroll_suffix(ui, &mut crop.rotation_deg, -45.0..=45.0, 1, "°") {
                    changed = true;
                }
            }
            ui.add_space(4.0);
            ui.label("Drag-rectangle crop UI — coming with Phase 3 import flow");
        });

    if changed {
        EditChange {
            uniforms: true,
            tone_curve: before_tone_curve != edit.tone_curve.rgb,
            blur_inputs: (before_sharpening_radius - edit.detail.sharpening.radius).abs()
                > f32::EPSILON
                || before_noise_reduction != edit.detail.noise_reduction,
        }
    } else {
        EditChange::default()
    }
}
