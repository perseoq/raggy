# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

> **Convención del proyecto:** la versión y la etiqueta siguen el folio del
> commit, `1.0.<n>` y `v1.0.<n>`, donde `n` es el número del commit en `main`.

## [No publicado]

## [1.0.23] - 2026-10-14

### Cambiado

- **Barra superior reorganizada**: el nombre y la versión quedan a la izquierda
  (con el directorio de datos en la ayuda emergente), las acciones principales
  llevan **icono y texto** («Añadir carpeta», «Subir archivos», «Reindexar todo»)
  y las secundarias pasan a **iconos con ayuda emergente** a la derecha
  (Configuración, Claves de API, Artefactos). Desaparece la ruta larga del
  directorio de datos de la barra.
- La ventana de **Configuración** es más ancha y **desplazable**, para que quepa
  en pantallas pequeñas.
- Cinco iconos SVG nuevos: `folder-plus`, `upload`, `key`, `settings` y
  `archive`.

## [1.0.22] - 2026-10-14

### Cambiado

- **Panel lateral rediseñado**: carpetas y sesiones se muestran como tarjetas
  redondeadas con **iconos vectoriales** (carpeta, chat, reindexar, renombrar y
  eliminar en lugar de botones de texto). La fila activa se resalta con el color
  de selección y el estado de cada carpeta lleva un **punto de color** (verde
  cuando está indexada, azul mientras indexa, rojo si falla). Los títulos de
  sección (CARPETAS / SESIONES) y el botón «+» de nueva sesión ordenan el panel.

## [1.0.21] - 2026-10-14

### Añadido

- **Subida de archivos de código**: el botón **Subir archivos** permite elegir
  archivos sueltos (`.py`, `.rs`, `.cpp`, `.h`, `.go`, `.ts`…) que se copian a
  una carpeta gestionada («Archivos subidos») y se indexan como una más, lista
  para consultarla por RAG.

### Cambiado

- La indexación acepta **código y texto**, no solo Markdown: el escaneo de
  carpetas incluye las extensiones de código reconocidas (`is_indexable`).
- El chat **ya no muestra la fila de «Fuentes»**; el modelo sigue citando
  `[archivo:línea-línea]` dentro de la propia respuesta.

## [1.0.20] - 2026-10-14

### Añadido

- **Markdown en el chat**: las respuestas del asistente se renderizan con
  encabezados, negritas, cursivas, código en línea, listas numeradas y con
  viñetas, citas y reglas.
- **Bloques de código como tarjetas**: se muestran con resaltado de sintaxis y,
  en la esquina superior derecha, **iconos vectoriales SVG** para copiar,
  ampliar la vista y descargar. El nombre del archivo sigue siendo editable en la
  cabecera de la tarjeta.

### Cambiado

- El desplegable «Sistema» del tema alterna entre Novel y Tokyo Night según el
  sistema operativo.
- Nuevas dependencias: `egui_extras` (carga de SVG) y `pulldown-cmark` en la
  interfaz.

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

[No publicado]: https://github.com/perseoq/raggy/compare/v1.0.23...HEAD
[1.0.23]: https://github.com/perseoq/raggy/releases/tag/v1.0.23
[1.0.22]: https://github.com/perseoq/raggy/releases/tag/v1.0.22
[1.0.21]: https://github.com/perseoq/raggy/releases/tag/v1.0.21
[1.0.20]: https://github.com/perseoq/raggy/releases/tag/v1.0.20
[1.0.19]: https://github.com/perseoq/raggy/releases/tag/v1.0.19
[1.0.18]: https://github.com/perseoq/raggy/releases/tag/v1.0.18
[1.0.0]: https://github.com/perseoq/raggy/releases/tag/v1.0.0
