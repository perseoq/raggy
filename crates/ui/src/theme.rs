//! Aplicación del tema visual (HU-26).

use eframe::egui;

use raggy_core::Theme;

/// Traduce el tema de la configuración a la preferencia de `egui`.
pub fn preference(theme: Theme) -> egui::ThemePreference {
    match theme {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::System => egui::ThemePreference::System,
    }
}

/// Aplica el tema solicitado al contexto de `egui`.
pub fn apply(ctx: &eframe::CreationContext<'_>, theme: Theme) {
    ctx.egui_ctx.set_theme(preference(theme));
}
