# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

## [No publicado]

## [0.1.0] - 2026-10-14

Primera versión: cubre las 32 historias de usuario de `hu.md`.

### Añadido

- **Indexación de carpetas** (HU-01 a HU-04, HU-09): diálogo nativo para añadir
  carpetas, escaneo recursivo de `.md`, chunking semántico que respeta
  encabezados, listas y bloques de código, reindexación incremental por hash y
  fecha, vigilancia automática del sistema de archivos y borrado con confirmación.
- **Motor RAG** (HU-10, HU-11, HU-27): embeddings locales deterministas
  (sin modelos ni red), índice vectorial por similitud coseno, índice léxico
  BM25 y recuperación híbrida con umbral configurable; buscador en la interfaz
  con citas de archivo y líneas.
- **Chat y sesiones** (HU-05 a HU-08, HU-12 a HU-15): sesiones aisladas por
  carpeta, streaming con cancelación, prompt de sistema estricto con citas,
  temperatura 0.1 por defecto y selección de proveedor y modelo por sesión
  (OpenAI, Anthropic, DeepSeek y OpenRouter).
- **Claves y tolerancia a fallos** (HU-23 a HU-25): claves guardadas en
  `settings.json` (con permisos restringidos y sin aparecer nunca en los
  registros), prueba de conexión por proveedor y proveedor de respaldo con cambio
  bajo consentimiento.
- **Generación y descarga de código** (HU-16 a HU-19): detección de bloques,
  nombre declarado editable, vista previa con resaltado de sintaxis, copia,
  descarga individual con resolución de conflictos y descarga por lote.
- **Memoria de proyecto** (HU-20 a HU-22): registro de artefactos con contenido,
  árbol por ruta, archivado, exportación de la lista y continuidad entre turnos.
- **Configuración, seguridad y datos** (HU-26 a HU-32): tema claro/oscuro,
  parámetros del RAG con avisos, directorio de datos configurable con migración,
  secretos que nunca aparecen en los registros, aviso de privacidad en la
  primera ejecución, exportación de sesiones a Markdown y respaldo/restauración
  en zip con verificación de integridad.

### Cambiado

- La configuración y las claves de API viven en **`~/.raggydesktop/settings.json`**
  (antes TOML en el directorio de datos y claves en el llavero del sistema). El
  archivo se crea con permisos `0600` en Unix y las claves nunca se registran.
  El respaldo omite las claves al empaquetar la configuración.

### Producción

- Registros a consola **y a archivo con rotación diaria** en
  `<directorio de datos>/logs/`, más un gancho de pánicos que los registra.
- Tamaño y posición de la ventana se restauran al reabrir.
- `LICENSE` (MIT), `CHANGELOG`, `rust-toolchain.toml` y Dependabot.
- Integración continua con formato, Clippy, pruebas, compilación en Linux, macOS
  y Windows y auditoría de dependencias (RustSec).
- Flujo de publicación: al etiquetar `v*` se compilan binarios para Linux, macOS
  (arm64) y Windows, se empaquetan con sus checksums SHA-256 y se publican en
  GitHub Releases.

[No publicado]: https://github.com/perseoq/raggy/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/perseoq/raggy/releases/tag/v0.1.0
