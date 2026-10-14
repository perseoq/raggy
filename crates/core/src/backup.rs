//! Respaldo y restauración de índices y memoria de proyecto (HU-32).
//!
//! El paquete es un archivo ZIP con la base de datos (instantánea consistente),
//! la configuración y un manifiesto con el tamaño y el hash de cada entrada; al
//! importar se valida el manifiesto antes de escribir nada.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::AppPaths;

/// Nombre del manifiesto dentro del paquete.
const MANIFEST: &str = "manifest.toml";

/// Versión del formato de respaldo.
const FORMAT_VERSION: u32 = 1;

/// Contador para que los nombres de instantánea no colisionen entre hilos.
static SNAPSHOT_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Manifiesto del paquete.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    created_at: String,
    entries: Vec<ManifestEntry>,
}

/// Descripción de una entrada del paquete.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestEntry {
    name: String,
    size: u64,
    hash: String,
}

/// Resumen de una exportación o importación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSummary {
    /// Número de archivos incluidos.
    pub files: usize,
    /// Tamaño total de los datos, en bytes.
    pub bytes: u64,
}

/// Empaqueta la base de datos y la configuración en `destination` (HU-32).
pub fn export_backup(
    storage: &raggy_storage::Storage,
    paths: &AppPaths,
    destination: &Path,
) -> Result<BackupSummary> {
    let snapshot = std::env::temp_dir().join(format!(
        "raggy-snapshot-{}-{}-{}.sqlite3",
        std::process::id(),
        chrono::Utc::now().timestamp_millis(),
        SNAPSHOT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    storage.backup_to(&snapshot)?;

    let mut payloads: Vec<(String, Vec<u8>)> = vec![(
        "raggy.sqlite3".to_string(),
        fs::read(&snapshot).map_err(|source| CoreError::Io {
            path: snapshot.clone(),
            source,
        })?,
    )];

    // La configuración se incluye sin las claves de API.
    if paths.settings_path().is_file() {
        payloads.push((
            "settings.json".to_string(),
            sanitized_settings(&paths.settings_path()),
        ));
    }

    let mut entries = Vec::new();
    for (name, data) in &payloads {
        entries.push(ManifestEntry {
            name: name.clone(),
            size: data.len() as u64,
            hash: hash_bytes(data),
        });
    }

    let manifest = Manifest {
        version: FORMAT_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        entries,
    };
    let manifest_bytes = toml::to_string_pretty(&manifest)
        .map_err(|error| CoreError::Artifact(error.to_string()))?;

    let file = fs::File::create(destination).map_err(|source| CoreError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    for (name, bytes) in &payloads {
        writer.start_file(name, options).map_err(zip_error)?;
        writer.write_all(bytes).map_err(|source| CoreError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
    }
    writer.start_file(MANIFEST, options).map_err(zip_error)?;
    writer
        .write_all(manifest_bytes.as_bytes())
        .map_err(|source| CoreError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
    writer.finish().map_err(zip_error)?;

    let _ = fs::remove_file(&snapshot);

    let bytes = payloads.iter().map(|(_, data)| data.len() as u64).sum();
    Ok(BackupSummary {
        files: payloads.len(),
        bytes,
    })
}

/// Restaura un paquete en el directorio de datos indicado (HU-32).
///
/// Valida el manifiesto completo antes de escribir.
pub fn import_backup(archive: &Path, target: &AppPaths) -> Result<BackupSummary> {
    let file = fs::File::open(archive).map_err(|source| CoreError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_error)?;

    let manifest_bytes = read_entry(&mut zip, MANIFEST)?;
    let manifest: Manifest = toml::from_str(&String::from_utf8_lossy(&manifest_bytes))
        .map_err(|error| CoreError::Artifact(format!("manifiesto ilegible: {error}")))?;

    if manifest.version > FORMAT_VERSION {
        return Err(CoreError::Artifact(format!(
            "el respaldo usa una versión más reciente ({}) del formato soportado ({FORMAT_VERSION})",
            manifest.version
        )));
    }

    // 1. Validación de integridad antes de escribir nada.
    let mut validated = Vec::new();
    for entry in &manifest.entries {
        if !is_safe_name(&entry.name) {
            return Err(CoreError::Artifact(format!(
                "nombre de archivo no permitido en el respaldo: {}",
                entry.name
            )));
        }
        let data = read_entry(&mut zip, &entry.name)?;
        if data.len() as u64 != entry.size || hash_bytes(&data) != entry.hash {
            return Err(CoreError::Artifact(format!(
                "el archivo «{}» del respaldo no supera la verificación de integridad",
                entry.name
            )));
        }
        validated.push((entry.name.clone(), data));
    }

    // 2. Restauración.
    target.ensure_dirs()?;
    let mut bytes = 0u64;
    for (name, data) in &validated {
        let destination = target.data_dir.join(name);
        fs::write(&destination, data).map_err(|source| CoreError::Io {
            path: destination,
            source,
        })?;
        bytes += data.len() as u64;
    }

    Ok(BackupSummary {
        files: validated.len(),
        bytes,
    })
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut file = zip
        .by_name(name)
        .map_err(|error| CoreError::Artifact(format!("falta «{name}» en el respaldo: {error}")))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from(name),
            source,
        })?;
    Ok(buffer)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && !name.starts_with('.')
}

/// Devuelve `settings.json` sin las claves de API, para no filtrarlas (HU-29).
fn sanitized_settings(path: &Path) -> Vec<u8> {
    let mut value: serde_json::Value = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

    if let serde_json::Value::Object(map) = &mut value {
        map.remove("api_keys");
    }

    serde_json::to_vec_pretty(&value).unwrap_or_default()
}

fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn zip_error(error: zip::result::ZipError) -> CoreError {
    CoreError::Artifact(format!("error al procesar el paquete: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use raggy_storage::Storage;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-backup-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    #[test]
    fn exporta_e_importa_con_integridad() {
        let source = temp_dir("origen");
        let source_paths = AppPaths::with_home(&source);
        source_paths.ensure_dirs().expect("crea árbol");

        std::fs::write(
            source_paths.settings_path(),
            r#"{"theme":"dark","api_keys":{"deepseek":"super-secreto"}}"#,
        )
        .expect("escribe settings.json");

        let storage = Storage::open(&source_paths.db_path()).expect("abre la base de datos");
        storage
            .connection()
            .execute(
                "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
                 VALUES ('f1', '/tmp/docs', 'docs', 'indexed', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("inserta carpeta");

        let archive = source.join("respaldo.zip");
        let summary = export_backup(&storage, &source_paths, &archive).expect("exporta");
        assert!(summary.files >= 1);
        assert!(archive.is_file());
        drop(storage);

        // El respaldo conserva la configuración pero no las claves (HU-29).
        let file = fs::File::open(&archive).expect("abre el paquete");
        let mut zip = zip::ZipArchive::new(file).expect("zip válido");
        let mut settings = String::new();
        zip.by_name("settings.json")
            .expect("settings.json incluido")
            .read_to_string(&mut settings)
            .expect("lee settings");
        assert!(settings.contains("dark"), "{settings}");
        assert!(
            !settings.contains("super-secreto"),
            "el respaldo no debe incluir la clave: {settings}"
        );

        let target = temp_dir("destino");
        let target_paths = AppPaths::with_home(&target);
        let restored = import_backup(&archive, &target_paths).expect("importa");
        assert_eq!(restored.files, summary.files);

        let restored_storage = Storage::open(&target_paths.db_path()).expect("abre restaurada");
        let folders = restored_storage.list_folders().expect("lista");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "docs");

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn rechaza_un_paquete_corrupto() {
        let source = temp_dir("corrupto");
        let source_paths = AppPaths::with_home(&source);
        source_paths.ensure_dirs().expect("crea árbol");
        let storage = Storage::open(&source_paths.db_path()).expect("abre");
        let archive = source.join("respaldo.zip");
        export_backup(&storage, &source_paths, &archive).expect("exporta");
        drop(storage);

        // Alteramos el contenido del zip para romper el hash.
        let bytes = fs::read(&archive).expect("lee");
        let mut corrupted = bytes.clone();
        let middle = corrupted.len() / 2;
        corrupted[middle] ^= 0xFF;
        let broken = source.join("roto.zip");
        fs::write(&broken, corrupted).expect("escribe");

        let target = temp_dir("corrupto-destino");
        let target_paths = AppPaths::with_home(&target);
        let result = import_backup(&broken, &target_paths);
        assert!(result.is_err(), "un paquete corrupto no debe importarse");

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&target);
    }
}
