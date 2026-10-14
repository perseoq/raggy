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

## Flujo obligatorio para cada cambio

Cada cambio en `main` se publica como una versión. Sigue estos pasos en orden:

1. **Documenta la lista de cambios** en `CHANGELOG.md`, bajo `## [No publicado]`,
   con los encabezados que correspondan (`### Añadido`, `### Cambiado`,
   `### Corregido`, `### Eliminado`). Este es el registro que se conserva por
   versión; descríbelo de forma concreta y en español.
2. **Sube la versión** del workspace en `Cargo.toml`
   (`[workspace.package] version`) a `1.0.<n>`, donde `<n>` es el número del
   commit que estás creando en `main` (`git rev-list --count HEAD` + 1). Los
   crates heredan la versión con `version.workspace = true`; `Cargo.lock` se
   regenera al compilar y también se versiona.
3. Mueve la entrada a `## [1.0.<n>] - <AAAA-MM-DD>` (fecha actual) y deja
   `## [No publicado]` vacía en la parte superior.
4. Ejecuta la comprobación completa (formato, Clippy, pruebas y `build --release`).
5. **Commit** en español: asunto conciso terminado en `; versión 1.0.<n>`, y en
   el cuerpo el detalle de los cambios.
6. `git push origin main`.
7. **Tag y release**: crea un tag anotado y súbelo. Empujar el tag dispara el
   workflow `Release`, que compila para Linux/macOS/Windows y publica el GitHub
   Release con los binarios y sus checksums:

   ```bash
   git tag -a v1.0.<n> -m "Raggy Desktop 1.0.<n>"
   git push origin v1.0.<n>
   ```

8. **Verifica** que CI y Release terminan bien:

   ```bash
   gh run list --limit 5
   gh run watch <id-del-run> --exit-status
   ```

## Convenciones de código

- Mantén el estilo, los nombres y la estructura existentes; no introduzcas texto
  en inglés en la interfaz ni en los mensajes.
- Respeta el diseño por capas y los contratos por trait ya existentes
  (`EmbeddingProvider`, `Chunker`, `Retriever`, `LlmProvider`, `SecretStore`).
- Nunca escribas claves de API en el código ni en la documentación: van en
  `settings.json` (permisos `0600` en Unix) o en variables de entorno. El tipo
  `Secret` redacta su valor en `Debug`/`Display`.
- Comenta el *porqué*, no el *qué*. Mantén las citas a las HU.
- Los cambios solo de documentación también siguen este flujo (versión + tag),
  para conservar la correspondencia `1.0.<n>` ↔ commit `<n>` de `main`.
