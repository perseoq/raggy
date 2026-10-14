//! Subida de archivos de código para que los analice el RAG.
//!
//! Los archivos elegidos se copian a una carpeta gestionada dentro del
//! directorio de datos (`uploads/`), que se indexa como una carpeta más. Así se
//! reutilizan el escaneo, la reindexación incremental, las citas y la vigilancia
//! sin tocar los archivos originales del usuario.

use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Copia los archivos indicados a la carpeta de subidas.
///
/// Devuelve las rutas destino. Los directorios y los archivos ilegibles se
/// omiten; si el destino ya existe se reemplaza (para reindexar la versión
/// nueva).
pub fn import_files(uploads_dir: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(uploads_dir).map_err(|source| CoreError::Io {
        path: uploads_dir.to_path_buf(),
        source,
    })?;

    let mut imported = Vec::new();

    for file in files {
        if !file.is_file() {
            tracing::warn!(path = %file.display(), "se omite: no es un archivo");
            continue;
        }

        let Some(name) = file.file_name() else {
            continue;
        };
        let destination = uploads_dir.join(name);

        match std::fs::copy(file, &destination) {
            Ok(_) => imported.push(destination),
            Err(error) => {
                tracing::warn!(path = %file.display(), "no se pudo copiar: {error}");
            }
        }
    }

    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-uploads-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    #[test]
    fn copia_los_archivos_a_la_carpeta_de_subidas() {
        let origin = temp_dir("orig");
        let uploads = temp_dir("destino").join("uploads");

        let script = origin.join("consulta.py");
        std::fs::write(&script, "print(1)\n").expect("escribe origen");
        let source = origin.join("main.rs");
        std::fs::write(&source, "fn main() {}\n").expect("escribe origen");

        let imported = import_files(&uploads, &[script.clone(), source.clone()]).expect("importa");

        assert_eq!(imported.len(), 2);
        assert!(uploads.join("consulta.py").is_file());
        assert!(uploads.join("main.rs").is_file());
        assert_eq!(
            std::fs::read_to_string(uploads.join("consulta.py")).expect("lee"),
            "print(1)\n"
        );

        // Reemplaza el contenido si se vuelve a subir el mismo archivo.
        std::fs::write(&script, "print(2)\n").expect("reescribe");
        import_files(&uploads, &[script]).expect("reimporta");
        assert_eq!(
            std::fs::read_to_string(uploads.join("consulta.py")).expect("lee"),
            "print(2)\n"
        );

        let _ = std::fs::remove_dir_all(&origin);
        let _ = std::fs::remove_dir_all(uploads.parent().expect("padre"));
    }

    #[test]
    fn omite_directorios() {
        let origin = temp_dir("dirs");
        let uploads = temp_dir("destino2").join("uploads");
        let directory = origin.join("subcarpeta");
        std::fs::create_dir_all(&directory).expect("crea subcarpeta");

        let imported = import_files(&uploads, &[directory]).expect("importa");
        assert!(imported.is_empty());

        let _ = std::fs::remove_dir_all(&origin);
        let _ = std::fs::remove_dir_all(uploads.parent().expect("padre"));
    }
}
