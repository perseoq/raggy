# LocalMind RAG Desktop (raggy)

Asistente de IA de escritorio, escrito en Rust, que responde **anclado únicamente
a documentación Markdown local**. Indexas una carpeta, creas sesiones aisladas
por carpeta y chateas con distintos proveedores de LLM (OpenAI, Anthropic,
DeepSeek, OpenRouter), generando y descargando código trazable.

> Este repositorio se encuentra en la **Fase 0**: cimientos técnicos. Todavía no
> indexa ni conversa; solo deja el esqueleto listo para construir encima.

## Documentación

| Documento | Contenido |
|---|---|
| [`plan.md`](plan.md) | Visión, alcance, arquitectura conceptual, flujos y riesgos. |
| [`tech.md`](tech.md) | Stack tecnológico y arquitectura de crates. |
| [`hu.md`](hu.md) | 32 historias de usuario con criterios de aceptación. |

## Estado de la Fase 0

Entregado:

- Workspace de Cargo con los 6 crates previstos en `tech.md`.
- Carga y guardado de configuración (`config.toml`) con validación y valores por
  defecto alineados con las HU (temperatura 0.1, `top-k` 5, umbral 0.3).
- Resolución de directorios de datos multiplataforma, con la variable de entorno
  `LOCALMIND_DATA_DIR` como atajo.
- Persistencia SQLite con migraciones versionadas (`PRAGMA user_version`) y
  esquema inicial: `folders`, `chunks`, `sessions`, `messages`, `artifacts`.
- Contratos de proveedor de LLM y de pipeline RAG (chunking, embeddings,
  recuperación), aún sin implementación.
- Interfaz `egui`/`eframe` con ventana, barra superior, panel lateral y panel
  central de presentación.
- Integración de `tracing` para registros.
- CI en GitHub Actions: formato, clippy sin advertencias, pruebas y build release.

Pendiente para las siguientes fases: indexación real (Fase 1), motor RAG
(Fase 2), chat y sesiones (Fase 3), multi-proveedor (Fase 4), generación y
descarga de código (Fase 5) y ajustes finales de configuración, seguridad y
exportación (Fase 6).

## Estructura del proyecto

```
raggy/
├── Cargo.toml                  # workspace y dependencias compartidas
├── crates/
│   ├── core/                   # configuración, rutas y errores comunes
│   ├── llm/                    # contrato multi-proveedor de LLM
│   ├── rag/                    # chunking, embeddings y recuperación
│   ├── storage/                # SQLite y migraciones
│   │   └── migrations/         # SQL versionado del esquema
│   ├── ui/                     # interfaz egui/eframe
│   └── app/                    # binario `raggy`
└── .github/workflows/ci.yml    # integración continua
```

## Requisitos

- Rust estable (probado con 1.99; mínimo declarado 1.85).
- En Linux, las dependencias habituales de `winit`/`glutin` para el entorno
  gráfico (X11). El crate `eframe` se compila solo con el backend `glow` + `x11`.

## Compilar y ejecutar

```bash
cargo build --release      # compila todo el workspace
cargo run -p localmind-app # abre la ventana (crea el directorio de datos)
cargo test --workspace     # ejecuta las pruebas
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Los datos se guardan en el directorio estándar del sistema
(`~/.local/share/localmind` en Linux). Para aislarlos durante el desarrollo:

```bash
LOCALMIND_DATA_DIR=./localmind-data cargo run -p localmind-app
```

El nivel de registro se controla con `RUST_LOG` (por ejemplo,
`RUST_LOG=debug cargo run -p localmind-app`).

## Licencia

MIT.
