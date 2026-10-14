//! Temas visuales: la paleta **Novel** (claro, la misma que incluye el terminal
//! *ptyxis*) y **Tokyo Night** (oscuro).
//!
//! El modo «sistema» elige una u otra según la preferencia del sistema operativo.

use eframe::egui::{self, Color32, Stroke, Theme as EguiTheme, Visuals};

use raggy_core::Theme;

// --- Novel (claro): https://github.com/Gogh-Co/Gogh themes/Novel.yml ---
const NOVEL_BG: Color32 = Color32::from_rgb(0xDF, 0xDB, 0xC3);
const NOVEL_BG_ALT: Color32 = Color32::from_rgb(0xD5, 0xD1, 0xB8);
const NOVEL_BG_LIGHT: Color32 = Color32::from_rgb(0xEF, 0xEC, 0xE0);
const NOVEL_LINE: Color32 = Color32::from_rgb(0xC8, 0xC3, 0xA6);
const NOVEL_FG: Color32 = Color32::from_rgb(0x3B, 0x23, 0x22);
const NOVEL_RED: Color32 = Color32::from_rgb(0xCC, 0x00, 0x00);
const NOVEL_GREEN: Color32 = Color32::from_rgb(0x00, 0x96, 0x00);
const NOVEL_ORANGE: Color32 = Color32::from_rgb(0xD0, 0x6B, 0x00);
const NOVEL_BLUE: Color32 = Color32::from_rgb(0x00, 0x00, 0xCC);
const NOVEL_CYAN: Color32 = Color32::from_rgb(0x00, 0x87, 0xCC);
const NOVEL_GRAY: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);

/// Color de la burbuja del usuario y de las filas seleccionadas (paleta Novel).
const NOVEL_USER_BG: Color32 = Color32::from_rgb(0xC6, 0xD3, 0xDA);

// --- Tokyo Night ---
const TN_BG: Color32 = Color32::from_rgb(0x1A, 0x1B, 0x26);
const TN_BG_DARK: Color32 = Color32::from_rgb(0x16, 0x16, 0x1E);
const TN_HIGHLIGHT: Color32 = Color32::from_rgb(0x29, 0x2E, 0x42);
const TN_FG: Color32 = Color32::from_rgb(0xC0, 0xCA, 0xF5);
const TN_FG_DARK: Color32 = Color32::from_rgb(0xA9, 0xB1, 0xD6);
const TN_COMMENT: Color32 = Color32::from_rgb(0x56, 0x5F, 0x89);
const TN_BLUE: Color32 = Color32::from_rgb(0x7A, 0xA2, 0xF7);
const TN_GREEN: Color32 = Color32::from_rgb(0x9E, 0xCE, 0x6A);
const TN_PURPLE: Color32 = Color32::from_rgb(0xBB, 0x9A, 0xF7);
const TN_ORANGE: Color32 = Color32::from_rgb(0xFF, 0x9E, 0x64);
const TN_RED: Color32 = Color32::from_rgb(0xF7, 0x76, 0x8E);
const TN_YELLOW: Color32 = Color32::from_rgb(0xE0, 0xAF, 0x68);

/// Traduce el tema de la configuración a la preferencia de `egui`.
pub fn preference(theme: Theme) -> egui::ThemePreference {
    match theme {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
        Theme::System => egui::ThemePreference::System,
    }
}

/// Tipografías del proyecto: dos puntos por encima de las de `egui`.
pub fn text_styles() -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::{FontFamily, FontId, TextStyle};

    [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(20.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(15.0, FontFamily::Monospace),
        ),
    ]
    .into()
}

/// Aplica la paleta, las tipografías y el espaciado del tema activo.
///
/// Es idempotente: fija valores absolutos, así que puede llamarse cada vez que
/// cambie el tema sin acumular cambios.
pub fn apply_style(ctx: &egui::Context, theme: Theme) {
    ctx.set_visuals(visuals(theme, ctx.theme()));

    let styles = text_styles();
    ctx.all_styles_mut(|style| {
        style.text_styles = styles.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
    });
}

/// Color de acento (botones principales, enlaces destacados).
pub fn accent(dark: bool) -> Color32 {
    if dark {
        TN_BLUE
    } else {
        NOVEL_BLUE
    }
}

/// Color del texto sobre el color de acento.
pub fn on_accent(dark: bool) -> Color32 {
    if dark {
        TN_BG
    } else {
        Color32::WHITE
    }
}

/// Color de un botón secundario (detener, cancelar).
pub fn secondary_button(dark: bool) -> Color32 {
    if dark {
        TN_HIGHLIGHT
    } else {
        NOVEL_BG_ALT
    }
}

/// Color del borde de las burbujas del chat.
pub fn bubble_stroke(dark: bool) -> Color32 {
    if dark {
        TN_HIGHLIGHT
    } else {
        NOVEL_LINE
    }
}

/// Fondo de una fila seleccionada o activa.
pub fn selection_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x2A, 0x33, 0x52)
    } else {
        NOVEL_USER_BG
    }
}

/// Color de éxito (carpeta indexada correctamente).
pub fn success_color(dark: bool) -> Color32 {
    if dark {
        TN_GREEN
    } else {
        NOVEL_GREEN
    }
}

/// Paleta que corresponde a la configuración y al tema resuelto por el sistema.
pub fn visuals(theme: Theme, resolved: EguiTheme) -> Visuals {
    match theme {
        Theme::Light => novel(),
        Theme::Dark => tokyo_night(),
        Theme::System => match resolved {
            EguiTheme::Dark => tokyo_night(),
            EguiTheme::Light => novel(),
        },
    }
}

/// Aplica el tema al arrancar.
pub fn apply(ctx: &eframe::CreationContext<'_>, theme: Theme) {
    ctx.egui_ctx.set_theme(preference(theme));
    apply_style(&ctx.egui_ctx, theme);
}

/// Color de los mensajes de error.
pub fn error_color(dark: bool) -> Color32 {
    if dark {
        TN_RED
    } else {
        NOVEL_RED
    }
}

/// Color de los avisos.
pub fn warning_color(dark: bool) -> Color32 {
    if dark {
        TN_YELLOW
    } else {
        NOVEL_ORANGE
    }
}

/// Color de la burbuja del usuario en el chat.
pub fn user_bubble(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x33, 0x3C, 0x5E)
    } else {
        NOVEL_USER_BG
    }
}

/// Color de la burbuja del asistente en el chat.
pub fn assistant_bubble(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x1F, 0x23, 0x35)
    } else {
        NOVEL_BG_ALT
    }
}

/// Colores del resaltado de sintaxis.
pub struct SyntaxColors {
    /// Palabras reservadas.
    pub keyword: Color32,
    /// Cadenas de texto.
    pub string: Color32,
    /// Comentarios.
    pub comment: Color32,
    /// Números.
    pub number: Color32,
}

/// Colores del resaltado según el modo.
pub fn syntax_colors(dark: bool) -> SyntaxColors {
    if dark {
        SyntaxColors {
            keyword: TN_BLUE,
            string: TN_GREEN,
            comment: TN_COMMENT,
            number: TN_ORANGE,
        }
    } else {
        SyntaxColors {
            keyword: NOVEL_BLUE,
            string: NOVEL_GREEN,
            comment: NOVEL_GRAY,
            number: NOVEL_ORANGE,
        }
    }
}

fn novel() -> Visuals {
    let mut visuals = Visuals::light();

    visuals.panel_fill = NOVEL_BG;
    visuals.window_fill = NOVEL_BG;
    visuals.faint_bg_color = NOVEL_BG_ALT;
    visuals.extreme_bg_color = NOVEL_BG_LIGHT;
    visuals.window_stroke = Stroke::new(1.0, NOVEL_LINE);
    visuals.hyperlink_color = NOVEL_CYAN;
    visuals.warn_fg_color = NOVEL_ORANGE;
    visuals.error_fg_color = NOVEL_RED;
    visuals.selection.bg_fill = NOVEL_LINE;
    visuals.selection.stroke = Stroke::new(1.0, NOVEL_CYAN);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = NOVEL_BG;
    widgets.noninteractive.weak_bg_fill = NOVEL_BG;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, NOVEL_LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, NOVEL_FG);

    widgets.inactive.bg_fill = NOVEL_BG_ALT;
    widgets.inactive.weak_bg_fill = NOVEL_BG_ALT;
    widgets.inactive.bg_stroke = Stroke::new(1.0, NOVEL_LINE);
    widgets.inactive.fg_stroke = Stroke::new(1.0, NOVEL_FG);

    widgets.hovered.bg_fill = NOVEL_BG_LIGHT;
    widgets.hovered.weak_bg_fill = NOVEL_BG_LIGHT;
    widgets.hovered.bg_stroke = Stroke::new(1.0, NOVEL_CYAN);
    widgets.hovered.fg_stroke = Stroke::new(1.0, NOVEL_FG);

    widgets.active.bg_fill = NOVEL_BG_LIGHT;
    widgets.active.weak_bg_fill = NOVEL_BG_LIGHT;
    widgets.active.bg_stroke = Stroke::new(1.0, NOVEL_CYAN);
    widgets.active.fg_stroke = Stroke::new(1.0, NOVEL_FG);

    widgets.open.bg_fill = NOVEL_BG_ALT;
    widgets.open.weak_bg_fill = NOVEL_BG_ALT;
    widgets.open.bg_stroke = Stroke::new(1.0, NOVEL_LINE);
    widgets.open.fg_stroke = Stroke::new(1.0, NOVEL_FG);

    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
    }

    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);

    visuals
}

fn tokyo_night() -> Visuals {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = TN_BG;
    visuals.window_fill = TN_BG;
    visuals.faint_bg_color = TN_BG_DARK;
    visuals.extreme_bg_color = TN_BG_DARK;
    visuals.window_stroke = Stroke::new(1.0, TN_HIGHLIGHT);
    visuals.hyperlink_color = TN_BLUE;
    visuals.warn_fg_color = TN_YELLOW;
    visuals.error_fg_color = TN_RED;
    visuals.selection.bg_fill = TN_HIGHLIGHT;
    visuals.selection.stroke = Stroke::new(1.0, TN_BLUE);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = TN_BG;
    widgets.noninteractive.weak_bg_fill = TN_BG;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, TN_HIGHLIGHT);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TN_FG);

    widgets.inactive.bg_fill = TN_HIGHLIGHT;
    widgets.inactive.weak_bg_fill = TN_BG;
    widgets.inactive.bg_stroke = Stroke::new(1.0, TN_HIGHLIGHT);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TN_FG_DARK);

    widgets.hovered.bg_fill = TN_HIGHLIGHT;
    widgets.hovered.weak_bg_fill = TN_HIGHLIGHT;
    widgets.hovered.bg_stroke = Stroke::new(1.0, TN_BLUE);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TN_FG);

    widgets.active.bg_fill = TN_HIGHLIGHT;
    widgets.active.weak_bg_fill = TN_HIGHLIGHT;
    widgets.active.bg_stroke = Stroke::new(1.0, TN_PURPLE);
    widgets.active.fg_stroke = Stroke::new(1.0, TN_FG);

    widgets.open.bg_fill = TN_HIGHLIGHT;
    widgets.open.weak_bg_fill = TN_BG;
    widgets.open.bg_stroke = Stroke::new(1.0, TN_HIGHLIGHT);
    widgets.open.fg_stroke = Stroke::new(1.0, TN_FG);

    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
    }

    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);

    visuals
}
