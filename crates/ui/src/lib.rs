//! `localmind-ui` — capa de presentación.
//!
//! Fase 0: ventana con el esqueleto de la aplicación (barra superior, panel
//! lateral y panel central). Los paneles de carpetas, sesiones y chat se
//! poblarán en las fases 1, 3 y 5.

pub mod app;
mod theme;

use eframe::egui;

use localmind_core::{AppConfig, AppPaths, APP_NAME};

/// Configuración de la ventana nativa.
fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    }
}

/// Arranca el bucle de interfaz. Bloquea hasta cerrar la ventana.
pub fn run(config: AppConfig, paths: AppPaths) -> eframe::Result<()> {
    eframe::run_native(
        APP_NAME,
        native_options(),
        Box::new(move |ctx| Ok(Box::new(app::LocalMindApp::new(ctx, config, paths)))),
    )
}
