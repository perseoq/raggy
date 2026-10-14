# Raggy Desktop

[![CI](https://github.com/perseoq/raggy/actions/workflows/ci.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/ci.yml)
[![Release](https://github.com/perseoq/raggy/actions/workflows/release.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Asistente de IA de escritorio que responde **anclado únicamente a tu
documentación Markdown local**. Indexas una carpeta, creas sesiones aisladas por
carpeta y conversas con distintos modelos de lenguaje, obteniendo respuestas con
**citas verificables** al archivo y a las líneas exactas… y sin que tus
documentos salgan de tu equipo más allá del proveedor que elijas.

Escrito en Rust con interfaz nativa (`egui`/`eframe`). Funciona en Linux, macOS y
Windows.

---

## Características

### Indexación local de Markdown
- Añade carpetas con el diálogo nativo; se recorren recursivamente todos los `.md`.
- **Chunking semántico**: respeta encabezados, listas y bloques de código (un
  bloque nunca se parte) y guarda archivo, ruta, encabezado padre y rango de líneas.
- **Reindexación incremental** por hash y fecha de modificación: solo se
  reprocesan los archivos nuevos o cambiados y se retiran los eliminados.
- **Vigilancia automática** de la carpeta: al cambiar un archivo se reindexa solo.
- La indexación corre en segundo plano: la interfaz nunca se bloquea.

### Búsqueda con recuperación híbrida
- **Embeddings locales** deterministas, sin modelos ni conexión a internet.
- Índice vectorial (similitud coseno) + índice léxico **BM25**, fusionados con un
  umbral de similitud configurable.
- Buscador que muestra los fragmentos más relevantes con su puntuación y su cita
  (`archivo.md:12-20`) y abre el archivo al seleccionarlos.

### Chat con contexto y citas
- **Sesiones aisladas por carpeta**: cada sesión solo consulta su propia
  documentación.
- **Streaming** token a token, con botón para detener la generación.
- **Prompt estricto**: el modelo solo responde con el contexto recuperado, cita
  `[archivo:línea-línea]` y, si no está en la documentación, responde
  «No encuentro esa información en los archivos indexados».
- **Citas clicables** en cada respuesta para abrir el fragmento de origen.
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
- **Respaldo e importación** en un `.zip` con verificación de integridad.

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

---

## Uso

1. **Configura tus claves.** Abre **Claves de API** y pega la clave del proveedor
   que vayas a usar, o define la variable de entorno correspondiente. El botón
   **Probar** comprueba la conexión y lista los modelos disponibles.
2. **Añade una carpeta.** Pulsa **Añadir carpeta** y elige una carpeta con
   documentación Markdown. Verás el progreso y, al terminar, cuántos archivos y
   fragmentos se han indexado.
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
├── indexes/          # reservado para índices en disco
├── artifacts/        # reservado para artefactos gestionados por la app
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
  "data_dir": null,
  "api_keys": {}
}
```

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
- **Al chatear solo se envían** al proveedor que elijas los fragmentos
  recuperados necesarios y tu pregunta. **No hay telemetría.**
- La aplicación muestra este aviso en la primera ejecución.
- Las claves **nunca aparecen en los registros**: van tras un tipo `Secret` que
  redacta su valor en cualquier traza y lo borra de la memoria al liberarlo.
- Las claves se guardan en `settings.json`, que se crea con permisos `0600` en
  Unix. Si prefieres no persistirlas, deja `api_keys` vacío y usa las variables
  de entorno.
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
