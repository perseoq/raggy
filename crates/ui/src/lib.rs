//! `raggy-ui` — capa de presentación.
//!
//! Interfaz de escritorio completa: panel lateral de carpetas y sesiones, vista
//! de exploración del índice, chat con streaming y citas, tarjetas de descarga
//! de código y ventanas de configuración, claves, artefactos y respaldo.

pub mod app;
mod theme;

use eframe::egui;

use raggy_core::{AppConfig, AppPaths, APP_NAME};

/// Tamaño por defecto de la ventana cuando no hay ninguno guardado.
const DEFAULT_WINDOW_SIZE: [f32; 2] = [1180.0, 760.0];

/// Configuración de la ventana nativa.
fn native_options(config: &AppConfig) -> eframe::NativeOptions {
    let size = config.window_size.unwrap_or(DEFAULT_WINDOW_SIZE);
    let size = [size[0].max(720.0), size[1].max(480.0)];

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size(size)
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    }
}

/// Arranca el bucle de interfaz. Bloquea hasta cerrar la ventana.
pub fn run(config: AppConfig, paths: AppPaths) -> eframe::Result<()> {
    let options = native_options(&config);
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |ctx| Ok(Box::new(app::RaggyApp::new(ctx, config, paths)))),
    )
}
