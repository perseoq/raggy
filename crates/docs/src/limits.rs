//! Valores comunes y topes de seguridad de la extracción de documentos (HU-04).

/// Bytes de un mebibyte, usado para traducir los límites de la interfaz.
pub const BYTES_PER_MIB: u64 = 1024 * 1024;

/// Límites configurables por defecto y sus rangos máximos permitidos.
pub const DEFAULT_MAX_FILE_MIB: u64 = 100;
pub const MAX_MAX_FILE_MIB: u64 = 16_384;
pub const DEFAULT_MAX_ARCHIVE_MIB: u64 = 512;
pub const MAX_MAX_ARCHIVE_MIB: u64 = 32_768;
pub const DEFAULT_MAX_PAGES: usize = 500;
pub const MAX_MAX_PAGES: usize = 100_000;
pub const DEFAULT_MAX_PDF_PAGE_TEXT_MIB: u64 = 8;
pub const MAX_MAX_PDF_PAGE_TEXT_MIB: u64 = 64;
pub const DEFAULT_MAX_PDF_TEXT_MIB: u64 = 64;
pub const MAX_MAX_PDF_TEXT_MIB: u64 = 512;
pub const DEFAULT_PROCESS_TIMEOUT_SECS: u64 = 120;
pub const MAX_PROCESS_TIMEOUT_SECS: u64 = 3_600;
pub const DEFAULT_MAX_OCR_IMAGE_PIXELS: u64 = 20_000_000;
pub const MIN_MAX_OCR_IMAGE_PIXELS: u64 = 262_144;
pub const MAX_MAX_OCR_IMAGE_PIXELS: u64 = 67_108_864;
pub const DEFAULT_MIN_TEXT_CHARS: usize = 24;
pub const MAX_MIN_TEXT_CHARS: usize = 5_000;

/// Salvaguardas internas de parsers que no se amplían mediante la configuración.
pub const MAX_ARCHIVE_ENTRIES: usize = 100_000;
/// Filas por sección de una hoja de cálculo o CSV.
///
/// Es **una**: así cada fragmento es exactamente una fila y la cita apunta a la
/// fila concreta (`hoja 1, fila 2669`) en lugar de a un rango de varias filas.
pub const ROWS_PER_SECTION: usize = 1;
pub const MAX_TOOL_PROBE_TIMEOUT_SECS: u64 = 15;
pub const MIN_OCR_RENDER_DIMENSION: u32 = 512;
pub const MAX_OCR_RENDER_DIMENSION: u32 = 8_192;
