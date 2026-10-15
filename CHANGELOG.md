# Changelog

Todos los cambios relevantes de este proyecto se documentan aquí.
El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/)
y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/).

> **Convención actual:** se usa SemVer y las publicaciones de un programa de
> varias fases se agrupan en la versión acordada para su cierre. Las entradas
> `1.0.x` conservan la convención histórica de folio de commit.

## [No publicado]

## [2.2.5] - 2026-10-15

### Añadido

- **Adjuntar archivos al chat (HU-34)**: botón «Adjuntar» junto a «Artefactos»
  para añadir uno o varios archivos a la conversación actual. Su contenido se
  extrae, se trocea y se indexa bajo la sesión, de modo que la respuesta combina
  el índice de la carpeta con el de los adjuntos y permite compararlos.
- Los adjuntos se muestran como etiquetas sobre la conversación, con su tamaño y
  un botón para quitarlos.
- El contexto indica el origen de cada fragmento (carpeta del chat o archivo
  adjunto) sin alterar la etiqueta de la cita.
- Límites de seguridad: 50 MB por adjunto y 200 MB por sesión; los archivos no
  indexables, vacíos o ilegibles se omiten indicando el motivo.
- Los adjuntos viven en `attachments/<sesión>/` y se incluyen en el respaldo y la
  restauración (HU-32).
- Migración de esquema 9: los fragmentos y documentos pueden pertenecer a una
  carpeta o a una sesión (nunca a ambas), con propietario único y borrado en
  cascada.

### Cambiado

- El chat recupera contexto con `search_chat` (carpeta + adjuntos de la sesión) y
  renueva su caché al adjuntar o quitar archivos, sin mezclar sesiones (HU-06).
- `Document` distingue `folder_id` y `session_id`, y el repositorio escribe
  documentos y fragmentos mediante un `IndexOwner` (carpeta o sesión).
- Eliminar una sesión retira también sus adjuntos, sus índices y los archivos
  almacenados.

## [2.2.4] - 2026-10-15

### Corregido

- **Integridad del índice**: los fragmentos de un archivo y su registro de
  documento se escriben en una **sola transacción**, de modo que una interrupción
  no deja un documento apuntando a un índice antiguo (ni al revés).
- **Subidas**: si un archivo reemplaza a otro con el mismo nombre y contenido
  distinto, la interfaz lo avisa en vez de hacerlo en silencio.
- **Filtro de subida**: la lista de extensiones del diálogo se deriva ahora de las
  mismas fuentes que el motor de indexación (Markdown, documentos y código), así
  que ya no se quedan fuera tipos como `.less`, `.astro`, `.htm`, `.fish` o `.env`.
- La tabla de archivos indexados marca en color los que quedaron con **0
  fragmentos** (escaneados, dañados o sin herramienta), con una pista al pasar el
  ratón.

### Cambiado

- `import_files` devuelve un `ImportSummary` (importados, reemplazados y
  omitidos); `INDEXABLE_EXTENSIONS` pasa a ser `indexable_extensions()`.

## [2.2.3] - 2026-10-15

### Cambiado

- **Rendimiento de ingesta**: la detección de Tesseract/Poppler/LibreOffice se
  memoriza durante 30 segundos, de modo que no se lanza `tesseract --list-langs`
  una vez por carpeta ni por cada PDF escaneado. El botón «Comprobar
  herramientas» fuerza una detección nueva.
- La fecha de revisión de una carpeta solo avanza al **terminar** la indexación;
  así la caché de búsqueda no se reconstruye a mitad de proceso ni dos veces por
  reindexado.
- El historial que se envía al proveedor se recorta a los 20 mensajes y 12 000
  caracteres más recientes, en vez de reenviar la conversación completa.
- La interfaz calcula la prosa, los bloques de código y las citas verificadas de
  cada mensaje una sola vez al cargarlo, no en cada fotograma.

## [2.2.2] - 2026-10-15

### Añadido

- Análisis de texto para la recuperación: se descartan las palabras vacías
  (español e inglés) y se normalizan los acentos, de modo que `gestion` encuentra
  «gestión». El índice léxico incluye además el encabezado y el nombre del
  archivo, y el embedding del fragmento parte de su título.

### Cambiado

- Al cambiar la tokenización, la huella del pipeline se incrementa
  (`INDEX_PIPELINE_VERSION` = 2): las carpetas ya indexadas se reprocesan solas en
  la siguiente indexación.

### Corregido

- Las consultas sin relación dejaban de puntuar alto solo por compartir artículos
  y preposiciones (antes ~0.15–0.26; ahora ~0.0), así que el corte relativo vuelve
  a separar con claridad lo relevante del ruido.

## [2.2.1] - 2026-10-15

### Corregido

- **Recuperación de contexto**: el corte de fragmentos era un umbral absoluto
  (0.3) sobre una puntuación combinada cuya escala real suele estar entre 0.2 y
  0.5, así que descartaba el mejor resultado en preguntas naturales y el modelo
  respondía «no encuentro esa información» aunque el dato estuviera indexado. Ahora
  el corte es **relativo al mejor resultado** (fracción configurable), de modo que
  nunca se descarta el fragmento más relevante; solo se descarta la consulta
  cuando no hay ninguna señal.
- Se añade una prueba de regresión que indexa documentos y verifica que varias
  preguntas naturales recuperan su documento con la configuración por defecto.
- Se registra en el log de depuración cuántos fragmentos se recuperan y su mejor
  puntuación, para diagnosticar respuestas sin contexto.

### Cambiado

- `RetrieverOptions::min_score` pasa a `relative_floor` (fracción 0.0–1.0); el
  campo `similarity_threshold` de `settings.json` conserva su nombre pero ahora se
  interpreta como corte relativo.
- El aviso de contexto vacío del prompt invita a reformular la búsqueda en vez de
  dar por hecho que la información no existe.

## [2.2.0] - 2026-10-14

### Añadido

- Límites configurables para el texto extraído por página y por documento PDF;
  las configuraciones previas reciben los valores por defecto al cargarse.
- `SearchCache` para conservar los recuperadores por carpeta y refrescarlos solo
  cuando cambia la revisión del índice.
- Pruebas de regresión con libros XLSX y PDF reales y ejecutables simulados para
  OCR/Poppler y conversión LibreOffice, sin depender de herramientas instaladas.

### Cambiado

- BM25 normaliza su puntuación según la cantidad de términos de consulta, ignora
  repeticiones y conserva la importancia absoluta de términos comunes; la fusión
  ya no reescala cada lista para forzar el mejor resultado a `1.0`.
- Los parámetros del recuperador híbrido (peso vectorial, BM25 y pool de
  candidatos) y los límites de extracción tienen valores centrales y opciones
  explícitas. La interfaz reutiliza el índice hasta que se completa una
  reindexación.
- El workflow `Release` llama a la CI para el commit etiquetado y no publica
  binarios si fallan las pruebas, auditoría o compilaciones multiplataforma.
- Se actualizaron plan, historias de usuario, stack y comentarios para reflejar
  `settings.json`, los formatos actualmente admitidos y el almacenamiento de
  claves sin cifrar.

### Corregido

- **Fase 1 — secretos y privacidad**: guardar o cerrar preferencias conserva las
  claves más recientes y no resucita las eliminadas; `SettingsStore` serializa
  cambios; se zeroizan copias temporales. El aviso versionado describe los datos
  enviados al proveedor y el almacenamiento no cifrado de las claves.
- **Fase 2 — índice fiable**: una huella de opciones y herramientas fuerza la
  reindexación necesaria; recorridos incompletos no eliminan rutas no vistas y
  SQLite espera hasta 5 segundos ante escrituras concurrentes.
- **Fase 3 — archivos seguros**: artefactos confinados a la carpeta elegida y
  temporales privados, impredecibles y con limpieza automática.
- **Fase 4 — límites de recursos**: tamaños de entrada y expansión Office,
  páginas y texto PDF, píxeles OCR y tiempos de herramientas quedan acotados.
  La expansión ZIP se mide realmente antes de invocar los extractores.
- **Fase 5 — respaldos completos**: se incluyen archivos subidos y artefactos
  físicos registrados; restauración validada en un directorio temporal privado,
  reubicación de rutas y actualización de `settings.json` sin pisar la base activa.
- **Fase 6 — citas verificables y navegables**: búsqueda, panel de fragmentos,
  chat y exportación usan una etiqueta con la ubicación propia del formato. Los
  enlaces abren el fragmento exacto y solo citas literales de fuentes recuperadas
  cuentan como ancladas, incluso al releer mensajes antiguos.

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
