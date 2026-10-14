# Raggy Desktop (raggy)

Asistente de IA de escritorio, escrito en Rust, que responde **anclado únicamente
a documentación Markdown local**. Indexas una carpeta, creas sesiones aisladas
por carpeta y chateas con distintos proveedores de LLM (OpenAI, Anthropic,
DeepSeek, OpenRouter), generando y descargando código trazable.

> Estado: **Fase 4 completada** — las API Keys se guardan cifradas en el llavero
> del sistema, se pueden probar las conexiones y cada sesión admite un proveedor
> de respaldo. Quedan la generación/descarga de código y la memoria de proyecto
> (Fase 5).

## Documentación

| Documento | Contenido |
|---|---|
| [`plan.md`](plan.md) | Visión, alcance, arquitectura conceptual, flujos y riesgos. |
| [`tech.md`](tech.md) | Stack tecnológico y arquitectura de crates. |
| [`hu.md`](hu.md) | 32 historias de usuario con criterios de aceptación. |

## Estado por fases

| Fase | Alcance | Estado |
|---|---|---|
| 0 | Cimientos: workspace, configuración, SQLite, esqueleto de UI y CI | ✅ |
| 1 | Carpetas e indexación: añadir, escanear, chunking, incremental, eliminar | ✅ |
| 2 | Motor RAG: embeddings, índice vectorial y recuperación | ✅ |
| 3 | Chat y sesiones por carpeta | ✅ |
| 4 | Multi-proveedor: claves, prueba de conexión y fallback | ✅ |
| 5 | Generación y descarga de código, memoria de proyecto | pendiente |
| 6 | Configuración avanzada, seguridad, exportación y pulido | pendiente |

### Fase 0 — cimientos

- Workspace de Cargo con los 6 crates previstos en `tech.md`.
- Configuración persistente (`config.toml`) con validación y valores por defecto
  acordes a las HU (temperatura 0.1, `top-k` 5, umbral 0.3).
- Resolución de directorios de datos multiplataforma (`RAGGY_DATA_DIR`).
- Persistencia SQLite con migraciones versionadas (`PRAGMA user_version`).
- Interfaz `egui`/`eframe`, registro con `tracing` y CI en GitHub Actions.

### Fase 1 — indexación de carpetas (HU-01 a HU-04, HU-09)

- Botón **«Añadir carpeta»** con diálogo nativo (backend `xdg-portal`, sin GTK).
- Escaneo recursivo de archivos `.md` (HU-02), informando de archivos ilegibles
  sin detener el proceso.
- **Chunking semántico** con `pulldown-cmark`: se respetan encabezados, listas y
  bloques de código (nunca se parte un bloque de código), y cada fragmento
  guarda archivo, ruta, encabezado padre y rango de líneas (HU-09).
- **Re-indexación incremental** por hash de contenido y fecha de modificación:
  solo se reprocesan archivos nuevos o cambiados y se retiran los eliminados
  (HU-03), más un botón **«Reindexar todo»**.
- **Vigilancia automática** del sistema de archivos con `notify`, con antirrebote.
- **Eliminación de carpetas** con diálogo de confirmación; los datos del índice
  se borran en cascada y los archivos en disco no se tocan (HU-04).
- Explorador en la interfaz: lista de carpetas con estado, archivos indexados y
  vista previa de los fragmentos de cada archivo.
- La indexación se ejecuta en un hilo dedicado, por lo que la interfaz nunca se
  bloquea y muestra progreso.

### Fase 2 — motor RAG (HU-10, HU-27 y datos de cita de HU-11)

- **Embeddings locales** sin modelo ni red, con *feature hashing* determinista
  (unigramas + bigramas, TF sublineal y normalización L2), en línea con la
  privacidad de HU-30. El trait `EmbeddingProvider` queda listo para conectar un
  modelo neuronal en el futuro.
- **Índice vectorial** en memoria con búsqueda por similitud coseno y **índice
  léxico BM25**; un recuperador **híbrido** fusiona ambos y aplica el umbral
  mínimo de similitud (HU-10).
- Los vectores se **persisten** en SQLite (columna `embedding` por fragmento) y
  se generan durante la indexación.
- **Buscador** en la interfaz: recupera los fragmentos más relevantes de la
  carpeta seleccionada y los muestra ordenados con puntuación, archivo, rango de
  líneas y encabezado; al elegir un resultado se abre su archivo. Si nada supera
  el umbral, se avisa explícitamente (HU-10).
- **Panel de configuración** del RAG (HU-27): tamaño de fragmento, solapamiento,
  top-k, umbral de similitud y dimensionalidad del embedding, con validación y
  persistencia.

### Fase 3 — chat y sesiones (HU-05 a HU-08, HU-12 a HU-15)

- **Sesiones por carpeta** (HU-05): crear, renombrar y eliminar, con historial
  persistente y reanudable (HU-07, HU-08); al eliminar una carpeta o sesión, sus
  mensajes se borran en cascada.
- **Aislamiento de contexto** (HU-06): cada sesión recupera únicamente de su
  carpeta; el prompt nunca mezcla documentación de otras carpetas.
- **Chat con streaming** (HU-12) en un hilo dedicado: la respuesta aparece token
  a token, con botón **Detener** y mensajes de error claros que permiten
  reintentar.
- **Prompt de sistema estricto** (HU-14): el modelo solo responde con el contexto
  recuperado, debe citar `[archivo:línea-línea]` y, si no lo encuentra, responder
  «No encuentro esa información en los archivos indexados».
- **Temperatura 0.1** por defecto y visible (HU-13).
- **Selección de proveedor y modelo por sesión** (HU-15): OpenAI, Anthropic,
  DeepSeek y OpenRouter; cada sesión recuerda su elección. Si falta la API Key se
  indica cuál definir.
- **Citas verificables** (HU-11): cada respuesta guarda y muestra sus fuentes;
  las citas son enlaces que abren el archivo y su fragmento.

> Las API Keys se guardan **cifradas en el llavero del sistema** (Windows
> Credential Manager, macOS Keychain o Secret Service en Linux). Como
> alternativa, si no hay llavero disponible se leen de las variables de entorno
> `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `DEEPSEEK_API_KEY` y
> `OPENROUTER_API_KEY`.

### Fase 4 — claves, prueba de conexión y fallback (HU-23 a HU-25)

- **Panel de claves de API** (HU-23): un campo por proveedor, guardado en el
  **almacén de credenciales del sistema** (nunca en la configuración ni en la
  base de datos) y **enmascarado** mostrando solo los últimos cuatro caracteres.
- **Probar conexión** (HU-24) por proveedor, en segundo plano: hace una llamada
  mínima, muestra cuántos modelos devuelve (con un ejemplo) y traduce los errores
  comunes a mensajes claros (clave inválida, sin crédito, límite de peticiones,
  proveedor caído).
- **Proveedor de respaldo por sesión** (HU-25): si el proveedor activo falla por
  disponibilidad, la interfaz lo avisa y ofrece un botón para cambiar y
  reintentar. Nunca se cambia de proveedor sin consentimiento.
- El chat ahora advierte cuando al proveedor seleccionado le falta la clave y
  ofrece configurarla en el acto.

> Nota sobre el almacén: en Linux el llavero debe estar desbloqueado (GNOME
> Keyring o KWallet). Si no está disponible, la aplicación lo indica y sigue
> funcionando con las variables de entorno.

## Estructura del proyecto

```
raggy/
├── Cargo.toml                  # workspace y dependencias compartidas
├── crates/
│   ├── core/                   # configuración, rutas, errores, indexación y watcher
│   │   └── tests/              # prueba de integración del pipeline completo
│   ├── llm/                    # contrato multi-proveedor de LLM
│   ├── rag/                    # chunking semántico, embeddings y recuperación
│   ├── storage/                # SQLite, migraciones y repositorio
│   │   └── migrations/         # SQL versionado del esquema
│   ├── ui/                     # interfaz egui/eframe
│   └── app/                    # binario `raggy`
└── .github/workflows/ci.yml    # integración continua
```

## Requisitos

- Rust estable (probado con 1.99; mínimo declarado 1.85).
- En Linux, las dependencias habituales de `winit`/`glutin` para el entorno
  gráfico (X11) y un portal de escritorio (`xdg-desktop-portal`) para el diálogo
  de selección de carpeta.

## Compilar y ejecutar

```bash
cargo build --release      # compila todo el workspace
cargo run -p raggy-app # abre la ventana (crea el directorio de datos)
cargo test --workspace     # ejecuta las pruebas
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Los datos se guardan en el directorio estándar del sistema
(`~/.local/share/raggy` en Linux). Para aislarlos durante el desarrollo:

```bash
RAGGY_DATA_DIR=./raggy-data cargo run -p raggy-app
```

El nivel de registro se controla con `RUST_LOG` (por ejemplo,
`RUST_LOG=debug cargo run -p raggy-app`).

## Licencia

MIT.
