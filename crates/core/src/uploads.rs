//! Subida de archivos de código para que los analice el RAG.
//!
//! Los archivos elegidos se copian a una carpeta gestionada dentro del
//! directorio de datos (`uploads/`), que se indexa como una carpeta más. Así se
//! reutilizan el escaneo, la reindexación incremental, las citas y la vigilancia
//! sin tocar los archivos originales del usuario.

use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};

/// Resultado de importar archivos a la carpeta gestionada.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Rutas destino escritas.
    pub imported: Vec<PathBuf>,
    /// Archivos que han reemplazado a otro con el mismo nombre y contenido
    /// distinto (misma ruta destino, datos nuevos).
    pub replaced: Vec<PathBuf>,
    /// Orígenes omitidos porque no eran archivos o no se pudieron copiar.
    pub skipped: Vec<PathBuf>,
}

impl ImportSummary {
    /// ¿No se importó nada?
    pub fn is_empty(&self) -> bool {
        self.imported.is_empty()
    }
}

/// Copia los archivos indicados a la carpeta de subidas.
///
/// Los directorios y los archivos ilegibles se omiten. Si el destino ya existe
/// se reemplaza (para reindexar la versión nueva); los reemplazos se registran en
/// `replaced` para poder avisar al usuario en vez de perder un archivo en
/// silencio.
pub fn import_files(uploads_dir: &Path, files: &[PathBuf]) -> Result<ImportSummary> {
    std::fs::create_dir_all(uploads_dir).map_err(|source| CoreError::Io {
        path: uploads_dir.to_path_buf(),
        source,
    })?;

    let mut summary = ImportSummary::default();

    for file in files {
        if !file.is_file() {
            tracing::warn!(path = %file.display(), "se omite: no es un archivo");
            summary.skipped.push(file.clone());
            continue;
        }

        let Some(name) = file.file_name() else {
            summary.skipped.push(file.clone());
            continue;
        };
        let destination = uploads_dir.join(name);
        let replaced = destination.is_file();

        match std::fs::copy(file, &destination) {
            Ok(_) => {
                if replaced {
                    summary.replaced.push(destination.clone());
                }
                summary.imported.push(destination);
            }
            Err(error) => {
                tracing::warn!(path = %file.display(), "no se pudo copiar: {error}");
                summary.skipped.push(file.clone());
            }
        }
    }

    Ok(summary)
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

        let summary = import_files(&uploads, &[script.clone(), source.clone()]).expect("importa");

        assert_eq!(summary.imported.len(), 2);
        assert!(
            summary.replaced.is_empty(),
            "la primera vez no reemplaza nada"
        );
        assert!(uploads.join("consulta.py").is_file());
        assert!(uploads.join("main.rs").is_file());
        assert_eq!(
            std::fs::read_to_string(uploads.join("consulta.py")).expect("lee"),
            "print(1)\n"
        );

        // Reemplaza el contenido si se vuelve a subir el mismo archivo, avisando.
        std::fs::write(&script, "print(2)\n").expect("reescribe");
        let summary = import_files(&uploads, &[script]).expect("reimporta");
        assert_eq!(summary.replaced.len(), 1, "debe registrar el reemplazo");
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

        let summary = import_files(&uploads, std::slice::from_ref(&directory)).expect("importa");
        assert!(summary.is_empty());
        assert_eq!(summary.skipped, vec![directory]);

        let _ = std::fs::remove_dir_all(&origin);
        let _ = std::fs::remove_dir_all(uploads.parent().expect("padre"));
    }

    #[test]
    fn no_pierde_un_archivo_con_el_mismo_nombre_de_otra_carpeta() {
        // Dos orígenes distintos con el mismo nombre base: el segundo reemplaza
        // al primero, pero el resumen lo informa para poder avisar.
        let origin = temp_dir("colision");
        let uploads = temp_dir("destino3").join("uploads");

        let first = origin.join("a/informe.md");
        std::fs::create_dir_all(first.parent().expect("padre")).expect("crea a");
        std::fs::write(&first, "# A\n\ncontenido A\n").expect("escribe A");
        let second = origin.join("b/informe.md");
        std::fs::create_dir_all(second.parent().expect("padre")).expect("crea b");
        std::fs::write(&second, "# B\n\ncontenido B\n").expect("escribe B");

        import_files(&uploads, &[first]).expect("importa A");
        let summary = import_files(&uploads, &[second]).expect("importa B");

        assert_eq!(summary.replaced.len(), 1);
        assert_eq!(
            std::fs::read_to_string(uploads.join("informe.md")).expect("lee"),
            "# B\n\ncontenido B\n"
        );

        let _ = std::fs::remove_dir_all(&origin);
        let _ = std::fs::remove_dir_all(uploads.parent().expect("padre"));
    }
}
