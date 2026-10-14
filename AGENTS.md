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
hay que compilar y hacer push. El release también se publica automáticamente,
salvo cuando el usuario haya acordado explícitamente un programa de varias fases
con un único release final. En ese caso se valida y se sube cada fase, pero no se
etiqueta ni se publica hasta completar la última.

1. **Documenta la lista de cambios** en `CHANGELOG.md`, bajo `## [No publicado]`,
   con los encabezados que correspondan (`### Añadido`, `### Cambiado`,
   `### Corregido`, `### Eliminado`). Este es el registro que se conserva por
   versión; descríbelo de forma concreta y en español.
2. **Para una tarea/release individual**, sube la versión del workspace en `Cargo.toml`
   (`[workspace.package] version`) según el tipo de cambio:
   - **Cambio menor** (corrección, ajuste, mejora pequeña): incrementa el último
     número. Ejemplo: `1.0.28` → `1.0.29` → `1.0.30`.
   - **Cambio mayor** (arquitectura, funcionalidad importante o ruptura de
     compatibilidad): incrementa el primer número y reinicia el resto a cero.
     Ejemplo: `1.0.29` → `2.0.0`, y más adelante `2.0.x` → `3.0.0`.

   En caso de duda sobre si un cambio es mayor, trátalo como menor y consulta.
   Los crates heredan la versión con `version.workspace = true`; `Cargo.lock` se
   regenera al compilar y también se versiona.
   En un programa de varias fases acordado con el usuario, conserva la versión
   actual durante las fases intermedias; el incremento se hace al cerrar la última.
3. En una publicación individual, mueve la entrada a `## [<versión>] - <AAAA-MM-DD>`
   y deja `## [No publicado]` vacía. Durante un programa por fases, deja los
   cambios de todas las fases bajo `## [No publicado]` hasta el cierre.
4. Ejecuta la comprobación completa tras cada fase/tarea (formato, Clippy,
   pruebas y `build --release`).
5. **Commit** en español, con el detalle de los cambios. En publicaciones
   individuales, termina el asunto con `; versión <versión>`. En fases
   intermedias, indica `Fase N de M` y no incrementes la versión.
6. `git push origin main` después de cada fase/tarea.
7. **Al cerrar un release** (o la última fase del programa acordado), crea el
   tag anotado y súbelo. Empujar el tag dispara el
   workflow `Release`, que compila para Linux/macOS/Windows y publica el GitHub
   Release con los binarios y sus checksums:

   ```bash
   git tag -a v<versión> -m "Raggy Desktop <versión>"
   git push origin v<versión>
   ```

8. **Verifica** que CI y, cuando corresponda, Release terminan bien:

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
- Nunca escribas claves de API en el código ni en la documentación: se guardan
  en `settings.json` (permisos `0600` en Unix; no están cifradas) o en variables
  de entorno. El tipo `Secret` redacta su valor en `Debug`/`Display` y zeroiza
  las copias bajo su control.
- Comenta el *porqué*, no el *qué*. Mantén las citas a las HU.
- Los cambios solo de documentación o proceso también siguen este flujo
  (versión + tag) como cambios menores.
