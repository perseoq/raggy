//! Iconos vectoriales (SVG) usados en las tarjetas de código.

use eframe::egui::{self, Color32, Response, Vec2};

/// Icono de copiar al portapapeles.
pub fn copy() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/copy.svg")
}

/// Icono de descargar el bloque.
pub fn download() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/download.svg")
}

/// Icono de mostrar u ocultar la vista previa.
pub fn eye() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/eye.svg")
}

/// Icono de carpeta.
pub fn folder() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/folder.svg")
}

/// Icono de sesión de chat.
pub fn chat() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/chat.svg")
}

/// Icono de añadir.
pub fn plus() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/plus.svg")
}

/// Icono de reindexar.
pub fn refresh() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/refresh.svg")
}

/// Icono de eliminar.
pub fn trash() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/trash.svg")
}

/// Icono de renombrar.
pub fn pencil() -> egui::ImageSource<'static> {
    egui::include_image!("../assets/icons/pencil.svg")
}

/// Dibuja un icono vectorial sin botón.
pub fn icon(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tint: Color32,
    size: f32,
) -> Response {
    ui.add(
        egui::Image::new(source)
            .fit_to_exact_size(Vec2::splat(size))
            .tint(tint),
    )
}

/// Botón compacto con un icono vectorial y una ayuda emergente.
pub fn icon_button(
    ui: &mut egui::Ui,
    source: egui::ImageSource<'static>,
    tooltip: &str,
    tint: Color32,
) -> Response {
    let image = egui::Image::new(source)
        .fit_to_exact_size(Vec2::splat(16.0))
        .tint(tint);

    ui.add(egui::Button::image(image).frame(false))
        .on_hover_text(tooltip)
}
