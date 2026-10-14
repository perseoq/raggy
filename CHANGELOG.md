# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

> **Convención del proyecto:** la versión y la etiqueta siguen el folio del
> commit, `1.0.<n>` y `v1.0.<n>`, donde `n` es el número del commit en `main`.

## [No publicado]

## [1.0.19] - 2026-10-14

### Cambiado

- **Diseño del chat** al estilo ChatGPT/Gemini: **burbujas** (a la derecha las
  del usuario, a la izquierda las del asistente) sin las etiquetas «Tú» y
  «Asistente», y **compositor fijo abajo** con el cuadro de texto de la **misma
  altura que el botón** y un botón «Enviar» destacado. **Intro envía** y
  Mayús+Intro inserta un salto de línea.
- **Tipografía** dos puntos mayor en toda la interfaz, esquinas redondeadas y más
  espaciado.
- El código de las respuestas **ya no se repite** en la burbuja: se muestra la
  prosa y, debajo, la tarjeta descargable.
- **Prompt más preciso**: el modelo responde con la parte del contexto que sí
  responde a la pregunta y reserva la frase de «no encontrado» para cuando no hay
  nada relacionado, sin mezclarla con una respuesta. Las fuentes solo se muestran
  cuando la respuesta está anclada.

## [1.0.18] - 2026-10-14

### Cambiado

- **Temas visuales**: el modo claro usa la paleta **Novel** (la misma que incluye
  el terminal *ptyxis*) y el oscuro **Tokyo Night**; el modo «sistema» alterna
  entre ambas. El resaltado de sintaxis, los avisos y los errores siguen la paleta
  activa.

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

[No publicado]: https://github.com/perseoq/raggy/compare/v1.0.19...HEAD
[1.0.19]: https://github.com/perseoq/raggy/releases/tag/v1.0.19
[1.0.18]: https://github.com/perseoq/raggy/releases/tag/v1.0.18
[1.0.0]: https://github.com/perseoq/raggy/releases/tag/v1.0.0
