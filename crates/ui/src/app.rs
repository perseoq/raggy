//! Aplicación `egui` de LocalMind.

use eframe::egui;

use localmind_core::{AppConfig, AppPaths, APP_VERSION};

/// Estado raíz de la interfaz.
pub struct LocalMindApp {
    config: AppConfig,
    paths: AppPaths,
}

impl LocalMindApp {
    /// Crea la aplicación y aplica el tema guardado.
    pub fn new(ctx: &eframe::CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        crate::theme::apply(ctx, config.theme);
        Self { config, paths }
    }

    /// Barra superior con identidad y ubicación de datos.
    fn top_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("LocalMind");
                ui.separator();
                ui.label(format!("v{APP_VERSION} · Fase 0"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Datos: {}", self.paths.data_dir.display()));
                });
            });
            ui.add_space(4.0);
        });
    }

    /// Panel lateral de carpetas y sesiones (sin datos hasta fases 1 y 3).
    fn sidebar(&self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .default_size(260.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.strong("Carpetas");
                ui.add_space(4.0);
                ui.label("— sin carpetas indexadas (Fase 1) —");
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                ui.strong("Sesiones");
                ui.add_space(4.0);
                ui.label("— sin sesiones (Fase 3) —");
            });
    }

    /// Panel central con el estado del proyecto.
    fn central(&self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(96.0);
                ui.heading("LocalMind RAG Desktop");
                ui.add_space(8.0);
                ui.label("Entorno preparado (Fase 0).");
                ui.add_space(16.0);
                ui.label(
                    "La indexación de carpetas y el chat anclado al RAG \
                     llegarán en las próximas fases.",
                );
            });
        });
    }
}

impl eframe::App for LocalMindApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.top_bar(ui);
        self.sidebar(ui);
        self.central(ui);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Err(err) = self.config.save(&self.paths.config_path()) {
            tracing::warn!("no se pudo guardar la configuración al cerrar: {err}");
        }
    }
}
