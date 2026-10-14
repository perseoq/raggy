# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

> **Convención del proyecto:** la versión menor se incrementa en cada cambio
> (1.0 → 1.1 → 1.2 …) y la mayor queda reservada para cambios incompatibles.

## [No publicado]

## [1.0.0] - 2026-10-14

Primera versión estable: cubre las 32 historias de usuario de `hu.md`.

### Añadido

- **Indexación de carpetas**: diálogo nativo, escaneo recursivo de `.md`,
  chunking semántico que respeta encabezados, listas y bloques de código,
  reindexación incremental por hash y fecha, vigilancia automática y borrado con
  confirmación.
- **Motor RAG**: embeddings locales deterministas (sin modelos ni red), índice
  vectorial por similitud coseno, índice léxico BM25 y recuperación híbrida con
  umbral configurable; buscador con citas de archivo y líneas.
- **Chat y sesiones**: sesiones aisladas por carpeta, streaming con cancelación,
  prompt de sistema estricto con citas, temperatura 0.1 por defecto y selección de
  proveedor y modelo por sesión (OpenAI, Anthropic, DeepSeek y OpenRouter).
- **Claves y tolerancia a fallos**: gestión de claves, prueba de conexión por
  proveedor y proveedor de respaldo con cambio bajo consentimiento.
- **Generación y descarga de código**: detección de bloques, nombre declarado
  editable, vista previa con resaltado de sintaxis, copia, descarga individual con
  resolución de conflictos y descarga por lote.
- **Memoria de proyecto**: registro de artefactos con contenido, árbol por ruta,
  archivado, exportación de la lista y continuidad entre turnos.
- **Configuración, seguridad y datos**: tema claro/oscuro/sistema, parámetros del
  RAG con avisos, directorio de datos configurable con migración, secretos que
  nunca aparecen en los registros, aviso de privacidad en la primera ejecución,
  exportación de sesiones a Markdown y respaldo/restauración en zip con
  verificación de integridad.

### Cambiado

- La configuración y las claves de API viven en **`~/.raggydesktop/settings.json`**
  (carpeta base ajustable con `RAGGY_HOME`), con permisos `0600` en Unix. Las
  claves nunca se registran y el respaldo las omite al empaquetar la
  configuración.
- La ventana **Claves de API** usa **pestañas, una por proveedor**, en lugar de
  una tabla apretada.
- La barra superior muestra solo la versión (sin etiqueta de fase).

### Producción

- Registros a consola **y a archivo con rotación diaria** en
  `<directorio de datos>/logs/`, más un gancho de pánicos que los registra.
- Tamaño de la ventana restaurado al reabrir la aplicación.
- `LICENSE` (MIT), este CHANGELOG, `rust-toolchain.toml` y Dependabot.
- Integración continua con formato, Clippy, pruebas, compilación en Linux, macOS
  y Windows y auditoría de dependencias (RustSec).
- Flujo de publicación: al etiquetar `v*` se compilan binarios para Linux, macOS
  (arm64) y Windows, se empaquetan con sus checksums SHA-256 y se publican en
  GitHub Releases.

[No publicado]: https://github.com/perseoq/raggy/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/perseoq/raggy/releases/tag/v1.0.0
