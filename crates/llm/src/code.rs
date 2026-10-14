//! Detección de bloques de código en las respuestas del modelo (HU-16).
//!
//! Extrae los bloques cercados con ``` o ~~~ e intenta deducir, en este orden,
//! el nombre de archivo declarado: en la línea de apertura (`title="x.py"`,
//! `file=x.py`, `python:x.py`) o en el texto inmediatamente anterior.

/// Bloque de código detectado en una respuesta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    /// Lenguaje declarado en la línea de apertura, si lo hay.
    pub language: Option<String>,
    /// Nombre de archivo declarado, si lo hay.
    pub filename: Option<String>,
    /// Contenido del bloque.
    pub content: String,
    /// Línea (1-based) de la primera línea de contenido.
    pub start_line: usize,
}

/// Extensiones que se reconocen como código al deducir nombres de archivo.
const KNOWN_EXTENSIONS: &[&str] = &[
    "rs",
    "py",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "go",
    "java",
    "kt",
    "kts",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "m",
    "mm",
    "scala",
    "lua",
    "pl",
    "r",
    "dart",
    "ex",
    "exs",
    "clj",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "sql",
    "json",
    "yaml",
    "yml",
    "toml",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "md",
    "txt",
    "ini",
    "cfg",
    "conf",
    "env",
    "dockerfile",
    "makefile",
    "tf",
    "hcl",
    "proto",
    "graphql",
    "vue",
    "svelte",
    "astro",
    "zig",
    "nim",
    "jl",
    "groovy",
    "gradle",
    "sbt",
    "asm",
    "v",
    "sol",
];

/// Extrae todos los bloques de código de un texto Markdown.
pub fn extract_code_blocks(text: &str) -> Vec<CodeBlock> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    // `split` añade una línea vacía final cuando el texto termina en salto.
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let Some((marker, marker_len, info)) = opening_fence(lines[index]) else {
            index += 1;
            continue;
        };

        let mut end = index + 1;
        while end < lines.len() {
            if let Some((close_marker, close_len)) = closing_fence(lines[end]) {
                if close_marker == marker && close_len >= marker_len {
                    break;
                }
            }
            end += 1;
        }

        let content = lines[index + 1..end.min(lines.len())].join("\n");
        let (language, filename) = parse_info(&info);
        let filename = filename.or_else(|| filename_before(&lines, index));

        blocks.push(CodeBlock {
            language,
            filename,
            content,
            start_line: index + 2,
        });

        index = if end < lines.len() {
            end + 1
        } else {
            lines.len()
        };
    }

    blocks
}

/// Devuelve el texto sin los bloques de código cercados, conservando la prosa.
///
/// Se usa para no repetir el código en la burbuja cuando ya se muestra como
/// tarjeta descargable.
pub fn strip_code_blocks(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;

    for line in text.split('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next();
        let count = match marker {
            Some(c) if c == '`' || c == '~' => trimmed.chars().take_while(|&x| x == c).count(),
            _ => 0,
        };
        let is_fence = count >= 3;

        match open_fence {
            Some((open, length)) => {
                if is_fence && marker == Some(open) && count >= length {
                    open_fence = None;
                }
            }
            None => {
                if is_fence {
                    open_fence = Some((marker.unwrap_or('`'), count));
                } else {
                    out.push(line);
                }
            }
        }
    }

    let mut cleaned: Vec<&str> = Vec::new();
    for line in out {
        if line.trim().is_empty()
            && (cleaned.is_empty() || cleaned.last().is_some_and(|last| last.trim().is_empty()))
        {
            continue;
        }
        cleaned.push(line);
    }
    while cleaned.last().is_some_and(|line| line.trim().is_empty()) {
        cleaned.pop();
    }

    cleaned.join("\n")
}

/// Propone un nombre genérico y editable para un bloque sin nombre (HU-16).
pub fn suggest_filename(language: Option<&str>, index: usize) -> String {
    let extension = language.and_then(extension_for).unwrap_or("txt");
    format!("snippet_{}.{extension}", index + 1)
}

/// Extensión habitual de un lenguaje.
pub fn extension_for(language: &str) -> Option<&'static str> {
    Some(match language.to_ascii_lowercase().as_str() {
        "rust" | "rs" => "rs",
        "python" | "py" => "py",
        "javascript" | "js" | "node" => "js",
        "typescript" | "ts" => "ts",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "go" | "golang" => "go",
        "java" => "java",
        "kotlin" | "kt" => "kt",
        "c" => "c",
        "c++" | "cpp" | "cxx" => "cpp",
        "c#" | "csharp" | "cs" => "cs",
        "ruby" | "rb" => "rb",
        "php" => "php",
        "swift" => "swift",
        "scala" => "scala",
        "lua" => "lua",
        "perl" | "pl" => "pl",
        "r" => "r",
        "dart" => "dart",
        "elixir" | "ex" => "ex",
        "clojure" | "clj" => "clj",
        "bash" | "sh" | "shell" | "zsh" => "sh",
        "powershell" | "ps1" => "ps1",
        "sql" => "sql",
        "json" => "json",
        "yaml" | "yml" => "yml",
        "toml" => "toml",
        "xml" => "xml",
        "html" => "html",
        "css" => "css",
        "scss" => "scss",
        "markdown" | "md" => "md",
        "dockerfile" | "docker" => "dockerfile",
        "makefile" | "make" => "makefile",
        "hcl" | "terraform" | "tf" => "tf",
        "protobuf" | "proto" => "proto",
        "graphql" => "graphql",
        "zig" => "zig",
        "nim" => "nim",
        "julia" | "jl" => "jl",
        "groovy" => "groovy",
        _ => return None,
    })
}

/// Detecta una línea de apertura de cerco de código.
fn opening_fence(line: &str) -> Option<(char, usize, String)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == marker).count();
    if count < 3 {
        return None;
    }
    let info = trimmed[count..].trim().to_string();
    Some((marker, count, info))
}

/// Detecta una línea de cierre de cerco (sin información adicional).
fn closing_fence(line: &str) -> Option<(char, usize)> {
    let (marker, count, info) = opening_fence(line)?;
    if info.is_empty() {
        Some((marker, count))
    } else {
        None
    }
}

/// Interpreta la línea de apertura para obtener lenguaje y nombre.
fn parse_info(info: &str) -> (Option<String>, Option<String>) {
    let mut language = None;
    let mut filename = None;

    let mut parts = info.split_whitespace();

    if let Some(head) = parts.next() {
        if let Some((key, value)) = head.split_once('=') {
            if is_language(key) {
                language = Some(key.to_string());
            }
            filename = clean_filename(value);
        } else if let Some((lang, value)) = head.split_once(':') {
            if is_language(lang) {
                language = Some(lang.to_string());
            }
            filename = clean_filename(value);
        } else if let Some(name) = clean_filename(head) {
            filename = Some(name);
        } else if is_language(head) {
            language = Some(head.to_string());
        }
    }

    for token in parts {
        if filename.is_none() {
            if let Some((key, value)) = token.split_once('=') {
                if matches!(key, "title" | "file" | "filename" | "name") {
                    filename = clean_filename(value);
                }
            } else if let Some(value) = token
                .strip_prefix("file:")
                .or_else(|| token.strip_prefix("title:"))
            {
                filename = clean_filename(value);
            } else {
                filename = clean_filename(token);
            }
        }
        if filename.is_some() {
            break;
        }
    }

    (language, filename)
}

/// Busca un nombre de archivo en el texto inmediatamente anterior al bloque.
fn filename_before(lines: &[&str], fence: usize) -> Option<String> {
    let mut checked = 0;
    let mut index = fence;

    while index > 0 && checked < 3 {
        index -= 1;
        let line = lines[index].trim();
        if line.is_empty() {
            continue;
        }
        checked += 1;
        if let Some(name) = filename_in_text(line) {
            return Some(name);
        }
    }

    None
}

/// Busca el primer token que parezca un nombre de archivo dentro de una línea.
fn filename_in_text(line: &str) -> Option<String> {
    for raw in line.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
        });
        if let Some(name) = clean_filename(token) {
            return Some(name);
        }
    }
    None
}

/// Normaliza un posible nombre de archivo; devuelve `None` si no lo parece.
fn clean_filename(raw: &str) -> Option<String> {
    let token = raw.trim_matches(|c: char| {
        c == '"' || c == '\'' || c == '`' || c == '*' || c == ':' || c == ',' || c == ')'
    });
    if token.is_empty() || token.len() > 200 || token.contains("..") {
        return None;
    }

    let path = token.strip_prefix("./").unwrap_or(token);
    let name = path.rsplit('/').next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() || extension.len() > 10 {
        return None;
    }
    if !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if !KNOWN_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
        return None;
    }

    Some(path.to_string())
}

/// ¿El token parece un lenguaje de programación?
fn is_language(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 20
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '#')
        && token.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_bloques_con_lenguaje() {
        let text = "Aquí tienes:\n\n```rust\nfn main() {}\n```\n\nY listo.\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language.as_deref(), Some("rust"));
        assert_eq!(blocks[0].content, "fn main() {}");
        assert_eq!(blocks[0].filename, None);
        assert_eq!(blocks[0].start_line, 4);
    }

    #[test]
    fn deduce_el_nombre_de_la_linea_de_apertura() {
        for info in [
            "python title=\"file_uno.py\"",
            "file=file_uno.py",
            "python:file_uno.py",
        ] {
            let text = format!("```{info}\nprint('hola')\n```\n");
            let blocks = extract_code_blocks(&text);
            assert_eq!(
                blocks[0].filename.as_deref(),
                Some("file_uno.py"),
                "falló con: {info}"
            );
        }
    }

    #[test]
    fn deduce_el_nombre_del_texto_previo() {
        let text = "Crea `src/main.rs` con lo siguiente:\n\n```rust\nfn main() {}\n```\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks[0].filename.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn deduce_el_nombre_de_negrita_antes_del_bloque() {
        let text = "**app.js**\n```javascript\nconsole.log(1);\n```\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks[0].filename.as_deref(), Some("app.js"));
    }

    #[test]
    fn ignora_el_texto_previo_que_no_es_un_archivo() {
        let text = "Este es un ejemplo sencillo:\n\n```python\nprint(1)\n```\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks[0].filename, None);
    }

    #[test]
    fn propone_nombres_genericos_segun_el_lenguaje() {
        assert_eq!(suggest_filename(Some("rust"), 0), "snippet_1.rs");
        assert_eq!(suggest_filename(Some("python"), 2), "snippet_3.py");
        assert_eq!(suggest_filename(None, 0), "snippet_1.txt");
    }

    #[test]
    fn quita_los_bloques_de_codigo_y_conserva_la_prosa() {
        let text = "Antes de nada:\n\n```python\nprint(1)\n```\n\nY después.\n";
        assert_eq!(strip_code_blocks(text), "Antes de nada:\n\nY después.");

        assert_eq!(strip_code_blocks("```\nsolo código\n```"), "");
        assert_eq!(strip_code_blocks("prosa sin código"), "prosa sin código");
        assert_eq!(strip_code_blocks("Intro.\n\n```sh\nls\n```"), "Intro.");
    }

    #[test]
    fn respeta_cercos_con_tildes_y_varios_bloques() {
        let text = "~~~go\npackage main\n~~~\n\n```sql\nSELECT 1;\n```\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language.as_deref(), Some("go"));
        assert_eq!(blocks[1].language.as_deref(), Some("sql"));
    }

    #[test]
    fn no_detecta_bloques_sin_cierre_como_vacios() {
        let text = "```rust\nfn main() {}\n";
        let blocks = extract_code_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].content, "fn main() {}");
    }
}
