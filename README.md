# Raggy Desktop (raggy)

Asistente de IA de escritorio, escrito en Rust, que responde **anclado únicamente
a documentación Markdown local**. Indexas una carpeta, creas sesiones aisladas
por carpeta y chateas con distintos proveedores de LLM (OpenAI, Anthropic,
DeepSeek, OpenRouter), generando y descargando código trazable.

> Estado: **Fase 1 completada** — ya se pueden indexar carpetas de Markdown y
> explorar los fragmentos generados. El motor RAG, el chat y la gestión de
> sesiones llegan en las siguientes fases.

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
| 2 | Motor RAG: embeddings, índice vectorial y recuperación | pendiente |
| 3 | Chat y sesiones por carpeta | pendiente |
| 4 | Multi-proveedor y claves | pendiente |
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
