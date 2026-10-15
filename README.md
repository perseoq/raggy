# Raggy Desktop

[![CI](https://github.com/perseoq/raggy/actions/workflows/ci.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/ci.yml)
[![Release](https://github.com/perseoq/raggy/actions/workflows/release.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Asistente de IA de escritorio que responde **anclado únicamente a tus documentos
locales**: Markdown, texto, código, CSV, Office y PDF. Indexas una carpeta o
subes archivos sueltos, creas sesiones aisladas por contexto y conversas con
distintos modelos de lenguaje, obteniendo respuestas con citas verificables de
archivo y ubicación… y sin que tus documentos salgan del equipo más allá del
proveedor que elijas.

Escrito en Rust con interfaz nativa (`egui`/`eframe`). Funciona en Linux, macOS y
Windows.

---

## Características

### Indexación local de documentos y código
- Añade carpetas con el diálogo nativo; se recorren recursivamente **Markdown**
  (`.md`, `.markdown`, `.mdx`), **texto** (`.txt`), **archivos de código**
  (`py`, `rs`, `cpp`, `h`, `go`, `ts`…) y **documentos**: `.csv`, `.docx`,
  `.xlsx`/`.xls`/`.xlsm`/`.xlsb`, `.pptx`, `.pdf` y los formatos legacy
  `.doc`/`.ppt`.
- **Extracción de texto por formato**: CSV (filas), Excel (hojas), Word
  (párrafos y títulos), PowerPoint (diapositivas) y PDF (páginas), con citas por
  ubicación (`página 3`, `hoja Ventas`, `filas 1-100`, `diapositiva 2`).
- **OCR de PDF escaneados** con **Tesseract** (idioma configurable, por defecto
  español) rasterizando las páginas con **Poppler**. Si faltan las herramientas,
  la app avisa con el comando de instalación de tu sistema. Los `.doc`/`.ppt`
  legacy se convierten con **LibreOffice** si está instalado.
- **Límites de recursos configurables** en Configuración: tamaño máximo de
  archivo, expansión de Office, páginas y texto PDF, píxeles OCR y timeout de las
  herramientas externas. Los archivos que superan los límites se reportan sin
  bloquear indefinidamente la indexación.
- Botón **Subir archivos** para incorporar archivos sueltos: se copian
  a una carpeta gestionada («Archivos subidos») que se indexa como una más, sin
  tocar los originales. Si un archivo ya existe con el mismo nombre, se reemplaza
  y la interfaz lo avisa en lugar de hacerlo en silencio.
- Botón **Adjuntar** en el chat para añadir uno o varios archivos a la
  conversación actual: su contenido se indexa bajo esa sesión y se busca junto
  con la carpeta, de modo que puedes contrastar o comparar ambas fuentes. Los
  adjuntos se listan sobre la conversación y se pueden quitar uno a uno; nunca se
  filtran a otros chats. Límites: 50 MB por archivo y 200 MB por sesión.
- **Chunking semántico**: respeta encabezados, listas y bloques de código (un
  bloque nunca se parte) y guarda archivo, ruta, encabezado padre y ubicación
  nativa del formato.
- **Reindexación incremental** por hash del archivo y huella de opciones del
  pipeline: los cambios de chunking, embeddings, OCR o herramientas disponibles
  también se aplican al volver a indexar.
- **Límites de recursos configurables**: tamaño máximo de archivo, expansión de
  paquetes Office, páginas PDF, píxeles por imagen OCR y timeout de herramientas.
  Un recorrido incompleto no retira del índice archivos que no pudo examinar.
- **Vigilancia automática** de la carpeta: al cambiar un archivo se reindexa solo.
- La indexación corre en segundo plano: la interfaz nunca se bloquea.

### Búsqueda con recuperación híbrida
- **Embeddings locales** deterministas, sin modelos ni conexión a internet.
- Índice vectorial (similitud coseno) + índice léxico **BM25 normalizado por la
  consulta**, fusionados con un **corte relativo al mejor resultado de cada
  origen** (nunca se descarta el fragmento más relevante y una fuente no eclipsa
  a la otra). El recuperador se mantiene
  en caché por carpeta y se renueva al reindexar.
- **Análisis de texto**: se descartan las palabras vacías y la búsqueda no
  distingue acentos (`gestion` encuentra «gestión»); el índice léxico pesa además
  el encabezado y el nombre del archivo.
- Buscador que muestra los fragmentos más relevantes con su puntuación y su cita
  (`archivo.md (líneas 12-20)`) y abre el fragmento exacto al seleccionarlo.

### Chat con contexto y citas
- **Sesiones aisladas por carpeta**: cada sesión solo consulta su propia
  documentación.
- **Streaming** token a token, con botón para detener la generación.
- **Prompt estricto**: el modelo solo responde con el contexto recuperado, cita
  la fuente y, si no está en la documentación, responde
  «No encuentro esa información en los archivos indexados».
- **Citas precisas por formato**: por **líneas** en Markdown y código, por
  **página** en PDF y Word, por **diapositiva** en PowerPoint, y por **hoja y
  coordenadas de celda** en Excel y CSV (`hoja Ventas, A2:D20`).
- **Citas verificadas y clicables** en cada respuesta: solo se marcan como
  ancladas las referencias que coinciden exactamente con fuentes recuperadas;
  al pulsarlas se abre el fragmento correspondiente en la vista previa.
- Temperatura `0.1` por defecto para minimizar invenciones.

### Código generado, listo para guardar
- Detecta los bloques de código de las respuestas y **deduce el nombre de
  archivo** declarado (`title="x.py"`, `file=x.py`, `python:x.py` o el texto
  anterior); si no lo hay, propone uno editable.
- **Vista previa** con resaltado de sintaxis, copia al portapapeles y descarga
  individual o **por lote**.
- Ante un archivo existente, ofrece **sobrescribir** o **renombrar**; las rutas
  declaradas se sanean para no escribir nunca fuera de la carpeta elegida.
- **Memoria de proyecto**: cada descarga queda registrada en **Artefactos**, con
  árbol por ruta, y el modelo recibe el manifiesto de lo ya generado para no
  duplicarlo.

### Todo lo demás
- **Multi-proveedor por sesión**: OpenAI, Anthropic, DeepSeek y OpenRouter.
- **Prueba de conexión** por proveedor y **proveedor de respaldo** si el activo
  falla (nunca se cambia sin tu consentimiento).
- **Tema** claro (**Novel**), oscuro (**Tokyo Night**) o el del sistema;
  **parámetros del RAG** configurables con avisos para valores extremos.
- **Directorio de datos** configurable con migración de los datos existentes.
- **Exportar una sesión** a Markdown (con historial, citas y artefactos).
- **Respaldo e importación** en un `.zip` con base de datos, configuración sin
  claves, archivos subidos y artefactos físicos registrados; se verifica integridad y
  límites antes de restaurar en un directorio de datos nuevo.
  La restauración cambia `data_dir` y requiere reiniciar; las carpetas fuente
  externas no se copian (sus chunks sí quedan en la base de datos), por lo que
  se necesitan los originales para volver a indexarlas en otro equipo.

---

## Instalación

Descarga el paquete de tu plataforma desde
[Releases](https://github.com/perseoq/raggy/releases):

| Plataforma | Archivo |
|---|---|
| Linux (x86_64) | `raggy-<versión>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS (Apple Silicon) | `raggy-<versión>-aarch64-apple-darwin.tar.gz` |
| Windows (x86_64) | `raggy-<versión>-x86_64-pc-windows-msvc.zip` |

Cada archivo va acompañado de su checksum SHA-256. Descomprímelo y ejecuta el
binario `raggy`.

Requisitos en Linux: las bibliotecas habituales del entorno gráfico (X11) y un
portal de escritorio (`xdg-desktop-portal`) para el diálogo de selección de
carpeta.

Herramientas **opcionales** (solo si vas a usar esas funciones):

- **OCR de PDF escaneados**: `tesseract` con el paquete de español y Poppler
  (`pdftoppm`). La app muestra el comando exacto de tu sistema en
  *Configuración › OCR y conversión*.
- **Formatos legacy `.doc`/`.ppt`**: LibreOffice.

---

## Uso

1. **Configura tus claves.** Abre **Claves de API** y pega la clave del proveedor
   que vayas a usar, o define la variable de entorno correspondiente. El botón
   **Probar** comprueba la conexión y lista los modelos disponibles.
2. **Añade documentación.** Pulsa **Añadir carpeta** para indexar una carpeta con
   Markdown o código, o **Subir archivos** para incorporar archivos de código
   sueltos (quedan en la carpeta gestionada **Archivos subidos**).
3. **Explora.** Escribe una consulta en el buscador para ver los fragmentos
   relevantes con su puntuación y su cita. Los archivos de la carpeta se listan
   en la vista central.
4. **Chatea.** Dentro de esa carpeta, pulsa **Nueva** en la sección *Sesiones*.
   Elige proveedor y modelo (y un respaldo si quieres) y escribe tu pregunta. Las
   respuestas aparecen en streaming y con sus fuentes.
5. **Descarga el código.** Bajo cada bloque de código de una respuesta tienes
   *Vista previa*, *Copiar*, *Descargar* y, si hay varios, *Descargar todo*.
6. **Consulta lo generado.** La ventana **Artefactos** muestra el árbol de
   archivos descargados y permite abrir su ubicación, archivarlos o exportar la
   lista.

En **Configuración** están el tema, los parámetros del RAG, el directorio de
datos, la exportación de la sesión activa y el respaldo/restauración.

---

## Proveedores y modelos

| Proveedor | Modelos sugeridos |
|---|---|
| DeepSeek | `deepseek-chat`, `deepseek-flash`, `deepseek-v4-pro` |
| OpenAI | `gpt-4o-mini`, `gpt-4o`, `gpt-4.1-mini` |
| Anthropic | `claude-3-5-sonnet-latest`, `claude-3-5-haiku-latest`, `claude-3-opus-latest` |
| OpenRouter | `openai/gpt-4o-mini`, `anthropic/claude-3.5-sonnet`, `google/gemini-flash-1.5` |

El modelo por defecto de cada sesión es `deepseek-chat`. El campo de modelo
acepta cualquier identificador válido del proveedor.

---

## Configuración y datos

Todo vive en la **carpeta base** `~/.raggydesktop/`:

```
~/.raggydesktop/
├── settings.json     # configuración y claves de API
├── raggy.sqlite3     # base de datos (índices, sesiones, mensajes, artefactos)
├── uploads/          # archivos subidos a la carpeta gestionada
├── attachments/      # archivos adjuntos a cada chat (una subcarpeta por sesión)
├── artifacts/        # archivos generados registrados (se reconstruyen al restaurar)
├── indexes/          # reservado; los vectores viven en SQLite
└── logs/             # registros con rotación diaria
```

`settings.json` contiene, entre otros:

```json
{
  "theme": "system",
  "rag": { "chunk_size": 800, "chunk_overlap": 100, "top_k": 5,
            "similarity_threshold": 0.3, "embedding_dimensions": 512 },
  "model": { "temperature": 0.1, "max_tokens": 2048,
             "default_provider": "deepseek", "default_model": "deepseek-chat" },
  "ingest": { "ocr_enabled": true, "ocr_languages": ["spa"], "min_text_chars": 24,
               "max_file_mb": 100, "max_archive_mb": 512, "max_pages": 500,
               "max_pdf_page_text_mb": 8, "max_pdf_text_mb": 64,
               "process_timeout_secs": 120, "max_ocr_image_pixels": 20000000 },
  "data_dir": null,
  "api_keys": {}
}
```

`similarity_threshold` es un **corte relativo** (0.0–1.0): se conservan los
fragmentos cuya puntuación alcanza al menos esa fracción de la mejor de la
consulta. El corte se aplica por separado a la carpeta del chat y a sus adjuntos,
y se reserva una plaza al mejor de cada origen para poder compararlos. Un valor
bajo aporta más contexto; uno alto es más estricto.

Las claves se pueden introducir desde la interfaz (se guardan en `api_keys`) o
dejarse fuera y usar las variables de entorno `OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, `DEEPSEEK_API_KEY` y `OPENROUTER_API_KEY`.

Variables de entorno útiles:

| Variable | Efecto |
|---|---|
| `RAGGY_HOME` | Cambia la carpeta base (útil para pruebas o instalaciones portables). |
| `RUST_LOG` | Nivel de registro (por ejemplo `RUST_LOG=debug`). |
| `*_API_KEY` | Clave del proveedor, como alternativa a `settings.json`. |

---

## Privacidad y seguridad

- **El índice es local.** Los documentos se dividen y se vectorizan en tu equipo;
  no se envía el corpus a ningún servicio.
- Al chatear se envían al proveedor que elijas **la pregunta actual, los
  fragmentos recuperados y el historial previo de la conversación**. Si pides
  modificar un artefacto, también puede enviarse su contenido. **No hay
  telemetría.**
- El aviso de privacidad se muestra en la primera ejecución y vuelve a mostrarse
  cuando cambia su contenido.
- Las claves **nunca aparecen en los registros**: van tras un tipo `Secret` que
  redacta su valor en `Debug`/`Display` y zeroiza las copias bajo control de la
  aplicación al liberarlas.
- Las claves se guardan en **texto claro** en `settings.json` (permisos `0600`
  en Unix; en otros sistemas dependen de los permisos heredados del directorio).
  No se cifran ni se guardan en el llavero del sistema. Para no persistirlas,
  usa las variables de entorno y deja `api_keys` vacío.
- El respaldo **omite las claves** al empaquetar la configuración.

---

## Compilar desde el código

```bash
cargo build --release      # compila todo el workspace
cargo run -p raggy-app     # abre la aplicación
cargo test --workspace     # ejecuta las pruebas
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Para no tocar los datos reales durante el desarrollo:

```bash
RAGGY_HOME=./raggy-home RUST_LOG=debug cargo run -p raggy-app
```

### Estructura

```
raggy/
├── crates/
│   ├── core/       # configuración, rutas, indexación, chat, artefactos, respaldo
│   ├── docs/       # extracción de documentos (CSV, Office, PDF) y OCR
│   ├── llm/        # contrato multi-proveedor, streaming SSE y prompt estricto
│   ├── rag/        # chunking semántico, embeddings, BM25 y recuperación híbrida
│   ├── storage/    # SQLite, migraciones versionadas y repositorio
│   ├── ui/         # interfaz egui/eframe
│   └── app/        # binario `raggy`
└── .github/        # integración continua y flujo de publicación
```

La integración continua comprueba formato, Clippy, pruebas y compilación en
Linux, macOS y Windows, además de auditar las dependencias (RustSec). Al etiquetar
`v*` se compilan los binarios de cada plataforma, se empaquetan con sus checksums
y se publican en GitHub Releases.

---

## Documentación

| Documento | Contenido |
|---|---|
| [`plan.md`](plan.md) | Visión, alcance, arquitectura conceptual, flujos y riesgos. |
| [`tech.md`](tech.md) | Stack tecnológico y arquitectura de crates. |
| [`hu.md`](hu.md) | Historias de usuario con criterios de aceptación. |
| [`CHANGELOG.md`](CHANGELOG.md) | Cambios por versión. |

## Licencia

MIT — consulta [`LICENSE`](LICENSE).
