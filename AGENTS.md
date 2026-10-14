# Instrucciones del proyecto — Raggy Desktop

Asistente de IA de escritorio escrito en Rust: workspace de Cargo con los crates
`core`, `llm`, `rag`, `storage`, `ui` y `app`. La interfaz usa `egui`/`eframe`.

## Idioma

- Todo va en **español**: código, nombres de dominio, comentarios, documentación,
  mensajes de error y mensajes de commit.
- Cada módulo cita en su cabecera la historia de usuario (HU) que implementa,
  p. ej. `HU-12`. Mantén ese enlace con `hu.md`, `plan.md` y `tech.md`.

## Antes de cada commit (debe quedar todo en verde)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo build --release
```

## Flujo obligatorio al terminar cada tarea

**Nada de esto se pregunta ni se deja para después.** Al terminar cualquier tarea
—por pequeña, técnica o documental que sea— hay que **compilar, hacer push y
crear el release en GitHub** de forma automática. Cada tarea se publica como una
versión. Sigue estos pasos en orden:

1. **Documenta la lista de cambios** en `CHANGELOG.md`, bajo `## [No publicado]`,
   con los encabezados que correspondan (`### Añadido`, `### Cambiado`,
   `### Corregido`, `### Eliminado`). Este es el registro que se conserva por
   versión; descríbelo de forma concreta y en español.
2. **Sube la versión** del workspace en `Cargo.toml`
   (`[workspace.package] version`) según el tipo de cambio:
   - **Cambio menor** (corrección, ajuste, mejora pequeña): incrementa el último
     número. Ejemplo: `1.0.28` → `1.0.29` → `1.0.30`.
   - **Cambio mayor** (arquitectura, funcionalidad importante o ruptura de
     compatibilidad): incrementa el primer número y reinicia el resto a cero.
     Ejemplo: `1.0.29` → `2.0.0`, y más adelante `2.0.x` → `3.0.0`.

   En caso de duda sobre si un cambio es mayor, trátalo como menor y consulta.
   Los crates heredan la versión con `version.workspace = true`; `Cargo.lock` se
   regenera al compilar y también se versiona.
3. Mueve la entrada a `## [<versión>] - <AAAA-MM-DD>` (la versión resultante y
   la fecha actual) y deja `## [No publicado]` vacía en la parte superior.
4. Ejecuta la comprobación completa (formato, Clippy, pruebas y `build --release`).
5. **Commit** en español: asunto conciso terminado en `; versión <versión>`, y en
   el cuerpo el detalle de los cambios.
6. `git push origin main`.
7. **Tag y release**: crea un tag anotado y súbelo. Empujar el tag dispara el
   workflow `Release`, que compila para Linux/macOS/Windows y publica el GitHub
   Release con los binarios y sus checksums:

   ```bash
   git tag -a v<versión> -m "Raggy Desktop <versión>"
   git push origin v<versión>
   ```

8. **Verifica** que CI y Release terminan bien:

   ```bash
   gh run list --limit 5
   gh run watch <id-del-run> --exit-status
   ```

   Si la auditoría de dependencias (RustSec) falla, actualiza la dependencia
   afectada (`cargo update -p <crate>`) dentro del mismo flujo.

## Convenciones de código

- Mantén el estilo, los nombres y la estructura existentes; no introduzcas texto
  en inglés en la interfaz ni en los mensajes.
- Respeta el diseño por capas y los contratos por trait ya existentes
  (`EmbeddingProvider`, `Chunker`, `Retriever`, `LlmProvider`, `SecretStore`).
- Nunca escribas claves de API en el código ni en la documentación: van en
  `settings.json` (permisos `0600` en Unix) o en variables de entorno. El tipo
  `Secret` redacta su valor en `Debug`/`Display`.
- Comenta el *porqué*, no el *qué*. Mantén las citas a las HU.
- Los cambios solo de documentación o proceso también siguen este flujo
  (versión + tag) como cambios menores.
