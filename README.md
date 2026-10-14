# Raggy Desktop (raggy)

[![CI](https://github.com/perseoq/raggy/actions/workflows/ci.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/ci.yml)
[![Release](https://github.com/perseoq/raggy/actions/workflows/release.yml/badge.svg)](https://github.com/perseoq/raggy/actions/workflows/release.yml)

Asistente de IA de escritorio, escrito en Rust, que responde **anclado únicamente
a documentación Markdown local**. Indexas una carpeta, creas sesiones aisladas
por carpeta y chateas con distintos proveedores de LLM (OpenAI, Anthropic,
DeepSeek, OpenRouter), generando y descargando código trazable.

> Estado: **Fase 6 completada** — las 32 historias de usuario de `hu.md` están
> implementadas. El proyecto cubre indexación, RAG híbrido, chat con sesiones,
> multi-proveedor con claves cifradas, generación y descarga de código con
> memoria de proyecto, configuración, seguridad y respaldo.

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
| 5 | Generación y descarga de código, memoria de proyecto | ✅ |
| 6 | Configuración, seguridad, exportación y respaldo | ✅ |

### Fase 0 — cimientos

- Workspace de Cargo con los 6 crates previstos en `tech.md`.
- Configuración persistente (`settings.json`) con validación y valores por defecto
  acordes a las HU (temperatura 0.1, `top-k` 5, umbral 0.3).
- Resolución de la carpeta base multiplataforma (`~/.raggydesktop`, ajustable con
  `RAGGY_HOME`).
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

> La configuración y las claves de API se guardan en
> **`~/.raggydesktop/settings.json`** (JSON, con permisos restringidos al
> propietario cuando el sistema lo permite). Como alternativa, si no hay clave en
> el archivo se leen las variables de entorno `OPENAI_API_KEY`,
> `ANTHROPIC_API_KEY`, `DEEPSEEK_API_KEY` y `OPENROUTER_API_KEY`.

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

> Nota de seguridad: las claves se guardan en texto plano dentro de
> `settings.json` (el archivo se crea con permisos `0600` en Unix y las claves
> nunca se registran en los logs). Si prefieres no persistirlas, deja
> `api_keys` vacío y usa las variables de entorno.

### Fase 5 — generación, descarga de código y memoria (HU-16 a HU-22)

- **Detección de bloques de código** (HU-16): se reconocen los bloques cercados
  con ``` o ~~~ y se deduce el nombre declarado en la línea de apertura
  (`title="x.py"`, `file=x.py`, `python:x.py`) o en el texto inmediatamente
  anterior; si no hay nombre, se propone uno genérico **editable**.
- **Vista previa** (HU-18) con resaltado de sintaxis propio (palabras clave,
  cadenas, comentarios y números), copia al portapapeles y nombre propuesto.
- **Descarga con el nombre correcto** (HU-17): se elige la carpeta destino, se
  respeta la extensión y, si el archivo ya existe, se avisa y se ofrece
  **sobrescribir** o **renombrar**. Las rutas declaradas se **sanean**: nunca se
  escribe fuera de la carpeta elegida.
- **Descarga por lote** (HU-19): un botón guarda todos los bloques de una
  respuesta respetando sus rutas relativas, con opción de sobrescribir y un
  resumen de archivos guardados.
- **Memoria de proyecto** (HU-20, HU-22): cada descarga se registra con nombre,
  ruta, fecha, sesión y modelo; la vista **Artefactos** muestra el árbol de
  archivos por ruta, permite abrir la ubicación, archivar o eliminar el registro
  (sin borrar el archivo) y **exportar la lista**.
- **Continuidad entre turnos** (HU-21): el prompt incluye el manifiesto de los
  artefactos de la sesión y, cuando el usuario menciona uno, también su
  contenido, para que el modelo lo modifique sin duplicarlo.

### Fase 6 — configuración, seguridad, exportación y respaldo (HU-26 a HU-32)

- **Tema claro/oscuro/sistema** (HU-26) desde el panel de configuración, con
  cambio inmediato y persistencia.
- **Parámetros del RAG** (HU-27) con avisos para valores extremos.
- **Directorio de datos configurable** (HU-28): se elige una carpeta y se puede
  **migrar** la base de datos, la configuración, los índices y los artefactos, o
  empezar de cero; el cambio se aplica al reiniciar.
- **Seguridad** (HU-29): las claves viven tras un tipo `Secret` cuyo `Debug` y
  `Display` redactan el valor, de modo que nunca aparecen en los registros.
- **Privacidad** (HU-30): aviso en la primera ejecución explicando que el índice
  es local y que al chatear solo se envían al proveedor elegido los fragmentos y
  la pregunta; sin telemetría.
- **Exportar sesión completa** (HU-31) a Markdown con metadatos, historial,
  citas y artefactos.
- **Respaldo e importación** (HU-32): paquete `.zip` con la base de datos
  (instantánea consistente) y la configuración, más un manifiesto con el hash de
  cada archivo que se **valida antes de restaurar**.

## Instalación

Descarga el binario de tu plataforma desde la página de
[releases](https://github.com/perseoq/raggy/releases). Cada paquete incluye un
archivo `raggy-<versión>-<plataforma>.tar.gz` (o `.zip` en Windows) con su
checksum SHA-256.

Al ejecutarlo por primera vez se crea `~/.raggydesktop/` con la configuración y
la base de datos. Las claves de API se pueden introducir desde el propio programa
(ventana **Claves de API**) o con las variables de entorno.

## Uso

1. **Añadir carpeta**: indexa una carpeta con documentación Markdown.
2. **Explorar**: busca en el índice y revisa los fragmentos recuperados con sus
   citas.
3. **Nueva sesión**: abre un chat aislado sobre esa carpeta.
4. **Descargar código**: los bloques de las respuestas se previsualizan,
   copian y descargan con su nombre declarado; quedan registrados en
   **Artefactos**.
5. **Configuración**: tema, parámetros del RAG, directorio de datos y respaldos.

## Estructura del proyecto

```
raggy/
├── Cargo.toml                  # workspace y dependencias compartidas
├── crates/
│   ├── core/                   # configuración, rutas, indexación, chat, respaldo
│   │   └── tests/              # pruebas de integración de los pipelines
│   ├── llm/                    # contrato multi-proveedor, SSE y prompt estricto
│   ├── rag/                    # chunking semántico, embeddings y recuperación
│   ├── storage/                # SQLite, migraciones y repositorio
│   │   └── migrations/         # SQL versionado del esquema
│   ├── ui/                     # interfaz egui/eframe
│   └── app/                    # binario `raggy`
└── .github/workflows/          # integración continua y publicación
```

## Requisitos

- Rust estable (probado con 1.99; mínimo declarado 1.85).
- En Linux, las dependencias habituales de `winit`/`glutin` para el entorno
  gráfico (X11) y un portal de escritorio (`xdg-desktop-portal`) para el diálogo
  de selección de carpeta.

## Compilar y ejecutar

```bash
cargo build --release      # compila todo el workspace
cargo run -p raggy-app     # abre la ventana (crea la carpeta base)
cargo test --workspace     # ejecuta las pruebas
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

La carpeta base es `~/.raggydesktop`, donde viven `settings.json`, la base de
datos, los índices, los artefactos y los registros (`logs/`). Para aislarlos
durante el desarrollo:

```bash
RAGGY_HOME=./raggy-home cargo run -p raggy-app
```

El nivel de registro se controla con `RUST_LOG` (por ejemplo,
`RUST_LOG=debug cargo run -p raggy-app`). Los registros se escriben también en
`<directorio de datos>/logs/` con rotación diaria.

## Licencia

MIT — consulta [`LICENSE`](LICENSE).

Los cambios relevantes se documentan en [`CHANGELOG.md`](CHANGELOG.md).
