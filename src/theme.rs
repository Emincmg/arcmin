//! Global look and feel: spacing, rounding, accent colour and button styles.

use eframe::egui::{self, Color32, CornerRadius, FontId, RichText, Stroke, TextStyle, vec2};

pub const ACCENT: Color32 = Color32::from_rgb(99, 102, 241);
const ACCENT_HOVER: Color32 = Color32::from_rgb(120, 123, 255);
const DANGER: Color32 = Color32::from_rgb(160, 60, 60);

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|style| {
        style.spacing.button_padding = vec2(14.0, 7.0);
        style.spacing.item_spacing = vec2(10.0, 8.0);
        style.spacing.interact_size.y = 32.0;

        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(24.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
            (TextStyle::Small, FontId::proportional(12.0)),
        ]
        .into();

        let radius = CornerRadius::same(8);
        let w = &mut style.visuals.widgets;
        for widget in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            widget.corner_radius = radius;
        }

        w.inactive.weak_bg_fill = Color32::from_gray(50);
        w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(72));
        w.hovered.weak_bg_fill = Color32::from_gray(66);
        w.hovered.bg_stroke = Stroke::new(1.0, ACCENT_HOVER);
        w.active.weak_bg_fill = ACCENT;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_HOVER);

        style.visuals.selection.bg_fill = ACCENT;
        style.visuals.panel_fill = Color32::from_gray(27);
    });
}

/// Filled accent button for the main action in a group.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .stroke(Stroke::NONE)
}

/// Filled red button for destructive actions.
pub fn danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(Color32::WHITE))
        .fill(DANGER)
        .stroke(Stroke::NONE)
}

/// A rounded, padded container that fills the available width.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(Color32::from_gray(34))
        .stroke(Stroke::new(1.0, Color32::from_gray(56)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}
