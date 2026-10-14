# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

> **Convención actual:** se usa SemVer y las publicaciones de un programa de
> varias fases se agrupan en la versión acordada para su cierre. Las entradas
> `1.0.x` conservan la convención histórica de folio de commit.

## [No publicado]

### Corregido

- **Fase 1 de correcciones 2.2.0 — secretos y aviso de privacidad**: guardar o
  cerrar preferencias ya conserva las claves más recientes del almacén y no
  resucita las eliminadas; `SettingsStore` serializa las modificaciones de
  claves; se zeroizan copias temporales y las claves de los proveedores al
  liberar los clientes. El aviso de privacidad vuelve a mostrarse cuando cambia
  su versión e informa del historial que se envía al proveedor y del
  almacenamiento no cifrado de las claves.
- **Fase 2 de correcciones 2.2.0 — índice fiable**: los documentos guardan una
  huella de las opciones, versión del pipeline y herramientas externas; cambiar
  chunking, embeddings, OCR o instalar Tesseract/Poppler/LibreOffice fuerza una
  reindexación aunque los bytes del archivo no cambien. Si el recorrido del
  sistema de archivos es incompleto, no se borran del índice rutas no vistas.
  SQLite espera hasta 5 segundos ante escrituras concurrentes.
- **Fase 3 de correcciones 2.2.0 — archivos seguros**: el guardado de artefactos
  usa un descriptor de directorio confinado a la carpeta elegida y no sigue
  enlaces simbólicos que escapen de ella. Los temporales de OCR y conversión
  ahora son privados, impredecibles y se limpian automáticamente.
- **Fase 4 de correcciones 2.2.0 — límites de recursos**: opciones de ingesta
  para tamaño máximo de archivo, expansión de paquetes Office, páginas PDF,
  tiempo máximo de Tesseract/Poppler/LibreOffice y píxeles OCR. La expansión ZIP
  se mide al descomprimir y el texto PDF tiene un tope por página y documento.
- **Fase 5 de correcciones 2.2.0 — respaldos completos**: se incluyen archivos
  subidos y los archivos físicos de artefactos registrados. La importación valida
  nombres, integridad y límites de tamaño/entradas; extrae en un directorio
  temporal privado con limpieza automática, reubica subidas/artefactos y apunta
  settings.json al nuevo directorio sin pisar la base activa.
- **Fase 4 de correcciones 2.2.0 — límites de recursos**: límites configurables
  para tamaño de entrada, expansión de paquetes Office, páginas PDF y píxeles
  OCR; timeout configurable de Tesseract/Poppler/LibreOffice. La extracción ZIP
  comprueba los bytes realmente descomprimidos antes de invocar parsers.

## [2.1.0] - 2026-10-14

### Cambiado

- **Citas más precisas por formato**. Cada fragmento cita su propia ubicación en
  el documento de origen:
  - **Excel y CSV**: hoja y rango de celdas con letras de columna
    (`hoja Ventas, A2:D20`, `hoja 1, A1:C100`), con cada fila numerada y una
    cabecera que mapea columna→letra (`A=nombre, B=edad`).
  - **Word**: número de página aproximado a partir de los saltos explícitos y de
    las marcas `lastRenderedPageBreak` del `.docx` (si no hay marcas, se cae a
    líneas como antes).
  - **PDF** y **PowerPoint**: se mantienen página y diapositiva.
  - **Markdown, texto y código**: líneas, como hasta ahora.
- La ubicación se calcula por **fragmento** (no por sección) en el chunker.

## [2.0.0] - 2026-10-14

### Añadido

- **Ingesta de documentos**: además de Markdown, texto y código, ahora se
  indexan CSV, Excel (`.xlsx`/`.xls`/`.xlsm`), Word (`.docx`), PowerPoint
  (`.pptx`) y PDF. Nuevo crate `raggy-docs` con la extracción por formato.
- **OCR de PDF escaneados** con Tesseract (idioma configurable, `spa` por
  defecto), rasterizando las páginas con Poppler (`pdftoppm`); detección de las
  herramientas y aviso con el comando de instalación por sistema operativo y
  distribución.
- **Conversión de formatos legacy** `.doc`/`.ppt` con LibreOffice headless.
- **Citas por ubicación** (`página N`, `hoja X`, `filas N-M`, `diapositiva N`):
  nueva columna `location` en `chunks` (migración 7) y en las citas.
- **Configuración de ingesta** (`ingest` en `settings.json`): activación del OCR,
  idiomas y umbral de texto; sección **OCR y conversión** en Configuración con el
  botón «Comprobar herramientas» y los avisos de la indexación.

### Cambiado

- La indexación calcula el hash sobre los bytes del archivo (no sobre el texto
  extraído), para poder saltar la extracción y el OCR cuando el documento no ha
  cambiado.

## [1.0.30] - 2026-10-14

### Cambiado

- `AGENTS.md`: el flujo de publicación (**compilar, push y release en GitHub**)
  pasa a ser **obligatorio y automático al terminar cada tarea**, sin preguntar
  ni dejarlo para después.

## [1.0.29] - 2026-10-14

### Añadido

- `AGENTS.md`: regla de versionado por tipo de cambio; los **cambios mayores**
  saltan a `2.0.0`, `3.0.0`… y los **menores** solo suben el último número.

### Cambiado

- **Compositor del chat centrado en vertical**: la caja de texto y el botón
  comparten una altura fija y un relleno simétrico, de modo que quedan centrados
  entre los delimitadores superior e inferior y ocupan todo el ancho del área.

## [1.0.28] - 2026-10-14

### Cambiado

- **Compositor del chat más alto**: la caja de texto pasa a tres líneas y el
  botón iguala su altura, con menos relleno interior, de forma que casi alcanzan
  los delimitadores superior e inferior del área y se ajustan a su ancho.

## [1.0.27] - 2026-10-14

### Corregido

- Actualizado `rustls` a `0.23.46`, que corrige cuatro avisos de seguridad
  (RUSTSEC-2026-0346 a RUSTSEC-2026-0349, publicados el 2026-10-14) y
  restablece la auditoría de dependencias en la CI.

## [1.0.26] - 2026-10-14

### Añadido

- **`AGENTS.md`**: instrucciones persistentes del proyecto. Fijan el flujo de
  publicación de cada cambio: registrar la lista de cambios en el `CHANGELOG`,
  subir la versión `1.0.<n>` (donde `<n>` es el número del commit), ejecutar
  formato/Clippy/pruebas/`build --release`, commit y `push`, y crear el tag
  `v1.0.<n>` que dispara la Release con los binarios por plataforma.

## [1.0.25] - 2026-10-14

### Cambiado

- **Cabecera de la sesión alineada**: el título, los desplegables de
  proveedor, modelo y respaldo, y los botones «Exportar sesión» y «Artefactos»
  comparten ahora una misma altura de fila, de modo que quedan centrados en la
  misma línea, con relleno simétrico respecto al borde. El botón «Configurar»
  (cuando falta la API Key) pasa a tamaño normal para no desentonar.

## [1.0.24] - 2026-10-14

### Cambiado

- El **cuadro de texto y el botón «Enviar»** del chat son un poco más altos (más
  relleno interior en el campo; el botón mantiene la misma altura que el campo).

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

[No publicado]: https://github.com/perseoq/raggy/compare/v2.1.0...HEAD
[2.1.0]: https://github.com/perseoq/raggy/releases/tag/v2.1.0
[2.0.0]: https://github.com/perseoq/raggy/releases/tag/v2.0.0
[1.0.24]: https://github.com/perseoq/raggy/releases/tag/v1.0.24
[1.0.23]: https://github.com/perseoq/raggy/releases/tag/v1.0.23
[1.0.22]: https://github.com/perseoq/raggy/releases/tag/v1.0.22
[1.0.21]: https://github.com/perseoq/raggy/releases/tag/v1.0.21
[1.0.20]: https://github.com/perseoq/raggy/releases/tag/v1.0.20
[1.0.19]: https://github.com/perseoq/raggy/releases/tag/v1.0.19
[1.0.18]: https://github.com/perseoq/raggy/releases/tag/v1.0.18
[1.0.0]: https://github.com/perseoq/raggy/releases/tag/v1.0.0
