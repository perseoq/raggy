# Stack Tecnológico: Raggy Desktop

A continuación, las tecnologías necesarias para construir este software, todas dentro del ecosistema Rust.

---

## 1. Framework de GUI (Interfaz de Escritorio)

**Opción principal: egui**
- Biblioteca GUI moderna escrita en Rust puro, con renderizado GPU (WGPU/glow).
- Multiplataforma nativa: Linux, macOS, Windows.
- Ideal para paneles laterales, listas de carpetas/sesiones, chat con streaming.
- Ecosistema maduro: `eframe` como capa de aplicación, `egui_extras` para tablas y árboles.

**Temas y resaltado**
- Tema claro **Novel**, oscuro **Tokyo Night** o el del sistema, con cambio
  inmediato.
- Los bloques de código en modo oscuro se resaltan con la paleta **Monokai Pro**
  de
  Sublime Text (fondo `#272822`); en modo claro se usa la paleta Novel. El
  resaltado es propio (`tokenize_code` + `LayoutJob`), sin dependencias externas.

**Alternativa: Iced**
- Arquitectura tipo Elm (mensajes, estado, vistas), muy adecuada para apps con estado complejo como esta.
- Renderizado basado en wgpu, buena integración con fuentes e imágenes.
- Útil si se quiere una UI más declarativa y con theming más controlado.

**Recomendación:** `egui` para desarrollo rápido y prototipado ágil; `Iced` si se prioriza una arquitectura más formal y escalable.

---

## 2. Cliente Multi-Proveedor de LLM

**Implementación actual: `reqwest` + adaptadores propios**
- Cada proveedor implementa el trait `LlmProvider`; OpenAI, Anthropic, DeepSeek y
  OpenRouter comparten el contrato de mensajes, streaming, parámetros y errores.
- `reqwest` gestiona HTTP y `tokio` ejecuta las peticiones fuera de la interfaz.
- Los modelos se seleccionan por sesión; no se cambia de proveedor sin permiso.

---

## 3. RAG y Búsqueda Vectorial

**Implementación actual:**
- `HashingEmbedding` genera embeddings locales deterministas; no descarga modelos
  ni envía documentos a un servicio de embeddings.
- SQLite (`rusqlite`) persiste chunks y vectores. El índice vectorial coseno y
  BM25 se construyen en memoria y se conservan en caché por carpeta y por sesión;
  se renuevan cuando cambia la revisión del índice o la de los adjuntos.
- El recuperador fusiona BM25 normalizado y similitud vectorial. Los parámetros
  de BM25, el peso vectorial y la amplitud del pool se pueden ajustar mediante
  `HybridRetrieverOptions`. La tokenización descarta palabras vacías en español e
  inglés y normaliza acentos; el índice léxico incluye encabezado y nombre de
  archivo.
- El corte de contexto es relativo al mejor resultado **de cada origen** (carpeta
  y adjuntos por separado) y se reserva una plaza al mejor de cada uno, de modo
  que una fuente no eclipsa a la otra y ambas pueden compararse.
- **Referencia cruzada** (`lookup_terms`): los identificadores que mencionan los
  adjuntos (folios, claves, RFC) se buscan también en la carpeta y sus
  coincidencias se anteponen al contexto, para que la pregunta no tenga que
  repetir el código.
- Markdown y código usan un chunker estructural; CSV, Excel, Office y PDF pasan
  por `raggy-docs` antes de crear fragmentos con su ubicación de origen.
- El recuperador de un chat puede fusionar los fragmentos de la carpeta con los
  de los archivos adjuntos a la sesión (HU-34); cada resultado conserva su
  origen (`carpeta` o `adjunto: <nombre>`) para poder compararlos.

---

## 4. Persistencia Local (Sesiones / Memoria / Proyectos)

**Implementación actual: SQLite vía `rusqlite`**
- Almacena:
  - Carpetas indexadas, revisiones y hashes de procesamiento.
  - Sesiones por carpeta.
  - Mensajes e historial.
  - Artefactos generados (nombre, ruta, fecha, sesión).
  - Memoria del proyecto.
- Las migraciones versionadas gestionan cambios de esquema (versión 9: adjuntos por
  sesión); las consultas se ejecutan con una conexión SQLite por hilo de trabajo.
- Los documentos y fragmentos pertenecen exactamente a una carpeta indexada o a
  una sesión (archivos adjuntos), nunca a ambas; se eliminan en cascada con su
  propietario.
- Los archivos adjuntos se guardan en `attachments/<sesión>/` y se incluyen en el
  respaldo y la restauración, como los archivos subidos y los artefactos físicos.

---

## 5. Almacenamiento de claves de API

**Implementación actual:** las claves se guardan sin cifrar en
`~/.raggydesktop/settings.json`, con permisos `0600` en Unix. `Secret` redacta el
valor en `Debug`/`Display` y zeroiza las copias bajo su control; las variables de
entorno también pueden aportar claves. El llavero nativo queda como posible
mejora futura; la aplicación no afirma usar cifrado ni un llavero del sistema.

---

## 6. Componentes Complementarios

| Necesidad | Crate recomendado |
|---|---|
| Runtime asíncrono | `tokio` |
| Cliente HTTP | `reqwest` (con soporte streaming) |
| Serialización JSON | `serde` + `serde_json` |
| Parseo Markdown | `pulldown-cmark` |
| Watcher de archivos (reindexado incremental) | `notify` |
| Diálogos nativos de carpeta/archivo | `rfd` |
| Directorios del sistema | `directories` |
| Logging | `tracing` + `tracing-subscriber` |
| Manejo de errores | `anyhow` + `thiserror` |
| Regex para detectar bloques de código con nombre | `regex` |
| Descarga de archivos generados | `std::fs` + `rfd` para elegir destino |
| Extracción de documentos (CSV, Office) | `csv`, `calamine`, `quick-xml` |
| Extracción de PDF | `lopdf` |
| OCR (opcional, externo) | `tesseract` (+ paquete `spa`) y `pdftoppm` (Poppler) |
| Conversión de `.doc`/`.ppt` (opcional, externo) | LibreOffice headless |

---

## 7. Arquitectura de Crate Sugerida

Separar el proyecto en workspaces de Cargo:

1. **`core`** — lógica de negocio: sesiones, ingesta, artefactos, abstracción de proveedores.
2. **`docs`** — extracción de texto de documentos (texto, CSV, Office, PDF) y OCR.
3. **`llm`** — adaptadores por proveedor (OpenAI, Anthropic, DeepSeek, OpenRouter).
4. **`rag`** — chunking, embeddings, índice vectorial, retrieval.
5. **`storage`** — SQLite, migraciones y repositorio.
6. **`ui`** — interfaz gráfica (egui).
7. **`app`** — binario principal que integra todo.

---

## 8. Resumen del Stack Mínimo Viable

| Capa | Tecnología |
|---|---|
| GUI | egui (eframe) |
| LLM multi-proveedor | `reqwest` + trait `LlmProvider` y adaptadores propios |
| RAG | Índice vectorial en memoria + BM25 sobre chunks de SQLite |
| Embeddings | `HashingEmbedding` local y determinista |
| Persistencia | SQLite (`rusqlite`) |
| Secretos | `settings.json` en texto claro, permisos restringidos y `Secret` |
| Async | tokio |
| Markdown | `pulldown-cmark` |
| Watcher | notify |
| Diálogos | rfd |

Con este conjunto se cubren todos los requisitos del proyecto: GUI moderna, multi-proveedor, RAG por carpeta, sesiones persistentes, generación y descarga de código, y memoria de proyecto.
