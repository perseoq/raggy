//! Aplicación del tema visual (HU-26).

use eframe::egui;

use raggy_core::Theme;

/// Traduce el tema de la configuración a la preferencia de `egui`.
pub fn apply(ctx: &eframe::CreationContext<'_>, theme: Theme) {
    let preference = match theme {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::System => egui::ThemePreference::System,
    };
    ctx.egui_ctx.set_theme(preference);
}
