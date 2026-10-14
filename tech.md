# Stack Tecnológico: Raggy Desktop

A continuación, las tecnologías necesarias para construir este software, todas dentro del ecosistema Rust.

---

## 1. Framework de GUI (Interfaz de Escritorio)

**Opción principal: egui**
- Biblioteca GUI moderna escrita en Rust puro, con renderizado GPU (WGPU/glow).
- Multiplataforma nativa: Linux, macOS, Windows.
- Ideal para paneles laterales, listas de carpetas/sesiones, chat con streaming.
- Ecosistema maduro: `eframe` como capa de aplicación, `egui_extras` para tablas y árboles.

**Alternativa: Iced**
- Arquitectura tipo Elm (mensajes, estado, vistas), muy adecuada para apps con estado complejo como esta.
- Renderizado basado en wgpu, buena integración con fuentes e imágenes.
- Útil si se quiere una UI más declarativa y con theming más controlado.

**Recomendación:** `egui` para desarrollo rápido y prototipado ágil; `Iced` si se prioriza una arquitectura más formal y escalable.

---

## 2. Cliente Multi-Proveedor de LLM

**Recomendado: `async-openai` + adaptadores propios**
- `async-openai` cubre OpenAI y sirve como base para proveedores compatibles (DeepSeek, OpenRouter).
- Para Anthropic se puede usar `anthropic` crate o un cliente HTTP propio con `reqwest`.
- Capa de abstracción propia: un trait común `LlmProvider` con implementaciones por proveedor, normalizando:
  - Mensajes (system, user, assistant).
  - Streaming token a token.
  - Parámetros (temperatura, max tokens).
  - Manejo de errores y rate limits.

**Alternativa unificada: `rig`**
- Framework Rust para aplicaciones con LLM, con soporte de múltiples proveedores y RAG integrado.
- Útil si se quiere acelerar la integración de embeddings, retrievers y agentes.

**Recomendación:** capa propia de abstracción sobre `reqwest` + `async-openai` para máximo control, o `rig` si se quiere ir más rápido.

---

## 3. RAG y Búsqueda Vectorial

**Embeddings (generación):**
- **Local:** `candle` o `ort` (ONNX Runtime) cargando modelos como `bge-small`, `jina-embeddings` o `nomic-embed`.
- **Remoto:** llamadas a la API de embeddings de OpenAI o proveedor elegido.

**Base de datos vectorial (embebida):**
| Opción | Características | Uso |
|---|---|---|
| **qdrant-client (modo local)** | HNSW, filtros por metadata, persistencia | Robusto y escalable |
| **lance / lancedb** | Formato columnar, búsqueda vectorial + full-text | Bueno para proyectos locales |
| **vecdb** | Embebido, HNSW + BM25 híbrido, WAL | Ligero, estilo SQLite |

**Recomendación:** `lancedb` o `qdrant` en modo local para el índice vectorial; combinarlo con BM25 (por ejemplo vía `tantivy`) para búsqueda híbrida.

**Chunking de Markdown:**
- `pulldown-cmark` para parsear `.md`.
- Estrategia propia de chunking respetando encabezados, listas y bloques de código.

---

## 4. Persistencia Local (Sesiones / Memoria / Proyectos)

**Recomendado: SQLite vía `rusqlite` o `sqlx`**
- Almacena:
  - Carpetas indexadas y su hash.
  - Sesiones por carpeta.
  - Mensajes e historial.
  - Artefactos generados (nombre, ruta, fecha, sesión).
  - Memoria del proyecto.
- `sqlx` con SQLite permite migraciones y consultas asíncronas.

**Alternativa: `sled` o `redb`**
- KV embebido en Rust puro, útil si se prefiere evitar SQL.

**Recomendación:** SQLite por claridad relacional y facilidad de consultas.

---

## 5. Almacenamiento Seguro de API Keys

**Recomendado: `keyring`**
- Usa el almacén de credenciales nativo del sistema:
  - Windows Credential Manager.
  - macOS Keychain.
  - Linux Secret Service (GNOME Keyring / KWallet).
- Ideal para guardar API Keys sin reinventar cifrado.

**Alternativa: `secrecy` + `aes-gcm`**
- Cifrado propio con `aes-gcm` y claves protegidas por el usuario.
- `secrecy` evita fugas accidentales en logs (tipos `SecretString`).

**Recomendación:** `keyring` como base, con `secrecy` para manejar secretos en memoria.

> **Decisión de implementación (v0.1.0):** las claves se guardan en
> `~/.raggydesktop/settings.json` junto al resto de la configuración, con
> permisos restringidos al propietario (`0600` en Unix). Se mantiene un tipo
> `Secret` interno que redacta el valor en `Debug`/`Display` y lo borra de
> memoria al liberarlo, y las variables de entorno siguen sirviendo como
> alternativa. El llavero del sistema queda como mejora futura.

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
| Directorios del sistema | `dirs` |
| Logging | `tracing` + `tracing-subscriber` |
| Manejo de errores | `anyhow` + `thiserror` |
| Regex para detectar bloques de código con nombre | `regex` |
| Descarga de archivos generados | `std::fs` + `rfd` para elegir destino |

---

## 7. Arquitectura de Crate Sugerida

Separar el proyecto en workspaces de Cargo:

1. **`core`** — lógica de negocio: sesiones, RAG, artefactos, abstracción de proveedores.
2. **`llm`** — adaptadores por proveedor (OpenAI, Anthropic, DeepSeek, OpenRouter).
3. **`rag`** — chunking, embeddings, índice vectorial, retrieval.
4. **`storage`** — SQLite, keyring, gestión de archivos.
5. **`ui`** — interfaz gráfica (egui o Iced).
6. **`app`** — binario principal que integra todo.

---

## 8. Resumen del Stack Mínimo Viable

| Capa | Tecnología |
|---|---|
| GUI | egui (eframe) |
| LLM multi-proveedor | reqwest + async-openai + capa propia |
| RAG | lancedb o qdrant local + tantivy (BM25) |
| Embeddings | candle u ort (local) o API remota |
| Persistencia | SQLite (sqlx o rusqlite) |
| Secretos | keyring + secrecy |
| Async | tokio |
| Markdown | pulldown-cmark |
| Watcher | notify |
| Diálogos | rfd |

Con este conjunto se cubren todos los requisitos del proyecto: GUI moderna, multi-proveedor, RAG por carpeta, sesiones persistentes, generación y descarga de código, y memoria de proyecto.
