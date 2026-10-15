//! Respaldo y restauración de índices y memoria de proyecto (HU-32).
//!
//! El paquete es un archivo ZIP con la base de datos (instantánea consistente),
//! la configuración y un manifiesto con el tamaño y el hash de cada entrada; al
//! importar se valida el manifiesto antes de escribir nada.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zeroize::Zeroize;

use crate::artifacts::{sanitize_relative, save_artifact, ConflictPolicy};
use crate::error::{CoreError, Result};
use crate::{AppConfig, AppPaths};

/// Nombre del manifiesto dentro del paquete.
const MANIFEST: &str = "manifest.toml";

/// Versión actual del formato de respaldo (v1 sigue siendo importable).
const FORMAT_VERSION: u32 = 2;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BACKUP_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_BACKUP_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_BACKUP_ARCHIVE_BYTES: u64 = MAX_BACKUP_TOTAL_BYTES + 64 * 1024 * 1024;
const MAX_BACKUP_ENTRIES: usize = 100_000;

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

struct PayloadFile {
    name: String,
    path: PathBuf,
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
    /// Directorio de datos restaurado (solo se define al importar).
    pub restored_data_dir: Option<PathBuf>,
}

/// Empaqueta la base de datos, configuración saneada, archivos subidos y los
/// archivos físicos de los artefactos registrados (HU-32).
pub fn export_backup(
    storage: &raggy_storage::Storage,
    paths: &AppPaths,
    destination: &Path,
) -> Result<BackupSummary> {
    let temporary = private_tempdir().map_err(|source| CoreError::Io {
        path: std::env::temp_dir(),
        source,
    })?;
    let snapshot = temporary.path().join("raggy.sqlite3");
    storage.backup_to(&snapshot)?;

    let mut payloads = vec![payload_file("raggy.sqlite3", &snapshot)?];
    let mut total_size = payloads[0].size;

    // La configuración se incluye sin las claves de API.
    if paths.settings_path().is_file() {
        let settings_path = temporary.path().join("settings.json");
        fs::write(&settings_path, sanitized_settings(&paths.settings_path())?).map_err(
            |source| CoreError::Io {
                path: settings_path.clone(),
                source,
            },
        )?;
        let payload = payload_file("settings.json", &settings_path)?;
        total_size = check_total_size(total_size, payload.size)?;
        payloads.push(payload);
    }

    collect_tree(
        &paths.uploads_dir(),
        "uploads",
        &mut payloads,
        &mut total_size,
    )?;
    collect_tree(
        &paths.attachments_dir(),
        "attachments",
        &mut payloads,
        &mut total_size,
    )?;
    collect_artifacts(storage, temporary.path(), &mut payloads, &mut total_size)?;
    if payloads.len() > MAX_BACKUP_ENTRIES {
        return Err(CoreError::Artifact(format!(
            "el respaldo supera el límite de {MAX_BACKUP_ENTRIES} archivos"
        )));
    }
    let mut entries = Vec::new();
    for payload in &payloads {
        entries.push(ManifestEntry {
            name: payload.name.clone(),
            size: payload.size,
            hash: payload.hash.clone(),
        });
    }

    let manifest = Manifest {
        version: FORMAT_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        entries,
    };
    let manifest_bytes = toml::to_string_pretty(&manifest)
        .map_err(|error| CoreError::Artifact(error.to_string()))?;

    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(CoreError::Artifact(
            "el manifiesto del respaldo es demasiado grande".to_string(),
        ));
    }

    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| CoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary_archive =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    let mut writer = zip::ZipWriter::new(temporary_archive);
    let options = zip::write::SimpleFileOptions::default();

    for payload in &payloads {
        writer
            .start_file(&payload.name, options)
            .map_err(zip_error)?;
        let (copied, hash) =
            copy_and_hash(&payload.path, &mut writer).map_err(|source| CoreError::Io {
                path: payload.path.clone(),
                source,
            })?;
        if copied != payload.size || hash != payload.hash {
            return Err(CoreError::Artifact(format!(
                "el archivo «{}» cambió mientras se creaba el respaldo",
                payload.name
            )));
        }
    }
    writer.start_file(MANIFEST, options).map_err(zip_error)?;
    writer
        .write_all(manifest_bytes.as_bytes())
        .map_err(|source| CoreError::Io {
            path: destination.to_path_buf(),
            source,
        })?;
    let temporary_archive = writer.finish().map_err(zip_error)?;
    let archive_size = temporary_archive
        .as_file()
        .metadata()
        .map_err(|source| CoreError::Io {
            path: destination.to_path_buf(),
            source,
        })?
        .len();
    if archive_size > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(CoreError::Artifact(
            "el archivo ZIP supera el límite máximo del respaldo".to_string(),
        ));
    }
    temporary_archive
        .persist(destination)
        .map_err(|error| CoreError::Io {
            path: destination.to_path_buf(),
            source: error.error,
        })?;

    Ok(BackupSummary {
        files: payloads.len(),
        bytes: total_size,
        restored_data_dir: None,
    })
}

fn payload_file(name: &str, path: &Path) -> Result<PayloadFile> {
    if !is_safe_archive_name(name) {
        return Err(CoreError::Artifact(format!(
            "nombre no permitido para el respaldo: {name}"
        )));
    }
    let metadata = fs::metadata(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let size = metadata.len();
    if size > MAX_BACKUP_FILE_BYTES {
        return Err(CoreError::Artifact(format!(
            "«{name}» supera el límite de {} MiB por archivo",
            MAX_BACKUP_FILE_BYTES / (1024 * 1024)
        )));
    }
    let hash = hash_file(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(PayloadFile {
        name: name.to_string(),
        path: path.to_path_buf(),
        size,
        hash,
    })
}

fn private_tempdir() -> std::io::Result<tempfile::TempDir> {
    private_tempdir_in(&std::env::temp_dir())
}

fn private_tempdir_in(parent: &Path) -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("raggy-backup-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(parent)
}

fn collect_tree(
    root: &Path,
    prefix: &str,
    payloads: &mut Vec<PayloadFile>,
    total_size: &mut u64,
) -> Result<()> {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(CoreError::Io {
                path: root.to_path_buf(),
                source,
            })
        }
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(CoreError::Artifact(format!(
            "la carpeta gestionada «{prefix}» no es un directorio seguro"
        )));
    }
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| CoreError::Artifact(error.to_string()))?;
        let relative = relative_path
            .components()
            .map(|component| match component {
                Component::Normal(name) => name.to_str().ok_or_else(|| {
                    CoreError::Artifact("el nombre de un archivo no es UTF-8".to_string())
                }),
                _ => Err(CoreError::Artifact(format!(
                    "ruta no normalizada dentro de {prefix}"
                ))),
            })
            .collect::<Result<Vec<_>>>()?
            .join("/");
        let name = format!("{prefix}/{relative}");
        if payloads.iter().any(|payload| payload.name == name) {
            return Err(CoreError::Artifact(format!(
                "hay más de un archivo con la ruta de respaldo «{name}»"
            )));
        }
        let source_size = fs::metadata(entry.path())
            .map_err(|source| CoreError::Io {
                path: entry.path().to_path_buf(),
                source,
            })?
            .len();
        let next_total = check_total_size(*total_size, source_size)?;
        let payload = payload_file(&name, entry.path())?;
        *total_size = next_total;
        payloads.push(payload);
        if payloads.len() > MAX_BACKUP_ENTRIES {
            return Err(CoreError::Artifact(format!(
                "el respaldo supera el límite de {MAX_BACKUP_ENTRIES} archivos"
            )));
        }
    }
    Ok(())
}

fn collect_artifacts(
    storage: &raggy_storage::Storage,
    temporary_dir: &Path,
    payloads: &mut Vec<PayloadFile>,
    total_size: &mut u64,
) -> Result<()> {
    for folder in storage.list_folders()? {
        for artifact in storage.list_artifacts_by_folder(&folder.id)? {
            let relative = artifact_relative_path(&artifact)?;
            let name = format!("artifacts/{relative}");
            if payloads.iter().any(|payload| payload.name == name) {
                return Err(CoreError::Artifact(format!(
                    "hay más de un artefacto con la ruta de respaldo «{name}»"
                )));
            }

            let payload_path = temporary_dir.join("payloads").join(&relative);
            if let Some(parent) = payload_path.parent() {
                fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            let max_copy = MAX_BACKUP_TOTAL_BYTES
                .saturating_sub(*total_size)
                .min(MAX_BACKUP_FILE_BYTES);
            match fs::symlink_metadata(&artifact.path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    copy_limited(&artifact.path, &payload_path, max_copy)?;
                }
                Ok(_) => write_artifact_fallback(&payload_path, &artifact.content, max_copy)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    write_artifact_fallback(&payload_path, &artifact.content, max_copy)?;
                }
                Err(source) => {
                    return Err(CoreError::Io {
                        path: PathBuf::from(&artifact.path),
                        source,
                    });
                }
            }

            let payload = payload_file(&name, &payload_path)?;
            *total_size = check_total_size(*total_size, payload.size)?;
            payloads.push(payload);
            if payloads.len() > MAX_BACKUP_ENTRIES {
                return Err(CoreError::Artifact(format!(
                    "el respaldo supera el límite de {MAX_BACKUP_ENTRIES} archivos"
                )));
            }
        }
    }
    Ok(())
}

fn copy_limited(source_path: &str, destination: &Path, limit: u64) -> Result<()> {
    let input = fs::File::open(source_path).map_err(|source| CoreError::Io {
        path: PathBuf::from(source_path),
        source,
    })?;
    let mut input = input.take(limit.saturating_add(1));
    let mut output = fs::File::create(destination).map_err(|source| CoreError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    let copied = std::io::copy(&mut input, &mut output).map_err(|source| CoreError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    if copied > limit {
        return Err(CoreError::Artifact(format!(
            "el archivo de artefacto supera el límite de {} MiB",
            limit / (1024 * 1024)
        )));
    }
    Ok(())
}

fn write_artifact_fallback(path: &Path, content: &str, limit: u64) -> Result<()> {
    if content.len() as u64 > limit {
        return Err(CoreError::Artifact(
            "el contenido del artefacto supera el límite disponible del respaldo".to_string(),
        ));
    }
    fs::write(path, content).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn artifact_relative_path(artifact: &raggy_storage::Artifact) -> Result<String> {
    let relative = sanitize_relative(&artifact.relative_path)
        .or_else(|| sanitize_relative(&artifact.name))
        .ok_or_else(|| {
            CoreError::Artifact(format!(
                "ruta de artefacto no válida en el respaldo: {}",
                artifact.relative_path
            ))
        })?;
    sanitize_relative(&format!(
        "{}/{}/{}",
        artifact.session_id, artifact.id, relative
    ))
    .ok_or_else(|| CoreError::Artifact("identificador de artefacto no válido".to_string()))
}

fn check_total_size(current: u64, added: u64) -> Result<u64> {
    let total = current.checked_add(added).ok_or_else(|| {
        CoreError::Artifact("el tamaño total del respaldo desborda el límite".to_string())
    })?;
    if total > MAX_BACKUP_TOTAL_BYTES {
        return Err(CoreError::Artifact(format!(
            "el respaldo supera el límite total de {} GiB",
            MAX_BACKUP_TOTAL_BYTES / (1024 * 1024 * 1024)
        )));
    }
    Ok(total)
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > MAX_BACKUP_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "el archivo supera el límite individual del respaldo",
            ));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn copy_and_hash<W: Write>(path: &Path, writer: &mut W) -> std::io::Result<(u64, String)> {
    use std::io::Read;

    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > MAX_BACKUP_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "el archivo excede el límite del respaldo",
            ));
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    Ok((total, hasher.finalize().to_hex().to_string()))
}

/// Restaura un paquete en el directorio de datos indicado (HU-32).
///
/// Valida el manifiesto completo antes de escribir.
pub fn import_backup(archive: &Path, target: &AppPaths) -> Result<BackupSummary> {
    let archive_size = fs::metadata(archive)
        .map_err(|source| CoreError::Io {
            path: archive.to_path_buf(),
            source,
        })?
        .len();
    if archive_size > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(CoreError::Artifact(format!(
            "el archivo de respaldo supera el límite de {} GiB",
            MAX_BACKUP_ARCHIVE_BYTES / (1024 * 1024 * 1024)
        )));
    }
    let file = fs::File::open(archive).map_err(|source| CoreError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(zip_error)?;
    if zip.len() > MAX_BACKUP_ENTRIES + 1 {
        return Err(CoreError::Artifact(format!(
            "el ZIP supera el límite de {MAX_BACKUP_ENTRIES} entradas"
        )));
    }

    let manifest_bytes = read_entry_limited(&mut zip, MANIFEST, MAX_MANIFEST_BYTES)?;
    let manifest: Manifest = toml::from_str(&String::from_utf8_lossy(&manifest_bytes))
        .map_err(|error| CoreError::Artifact(format!("manifiesto ilegible: {error}")))?;

    if manifest.version == 0 || manifest.version > FORMAT_VERSION {
        return Err(CoreError::Artifact(format!(
            "el respaldo usa una versión no compatible ({}) del formato soportado ({FORMAT_VERSION})",
            manifest.version
        )));
    }

    if manifest.entries.len() > MAX_BACKUP_ENTRIES {
        return Err(CoreError::Artifact(format!(
            "el manifiesto supera el límite de {MAX_BACKUP_ENTRIES} archivos"
        )));
    }
    let mut names = HashSet::new();
    names.insert(MANIFEST.to_string());
    let mut total_size = 0u64;
    for entry in &manifest.entries {
        if !is_safe_archive_name(&entry.name) {
            return Err(CoreError::Artifact(format!(
                "nombre de archivo no permitido en el respaldo: {}",
                entry.name
            )));
        }
        if !names.insert(entry.name.clone()) {
            return Err(CoreError::Artifact(format!(
                "el manifiesto repite el archivo «{}»",
                entry.name
            )));
        }
        if entry.size > MAX_BACKUP_FILE_BYTES {
            return Err(CoreError::Artifact(format!(
                "el archivo «{}» supera el límite por archivo",
                entry.name
            )));
        }
        total_size = total_size.checked_add(entry.size).ok_or_else(|| {
            CoreError::Artifact("el tamaño total del respaldo desborda el límite".to_string())
        })?;
        if total_size > MAX_BACKUP_TOTAL_BYTES {
            return Err(CoreError::Artifact(format!(
                "el respaldo supera el límite total de {} GiB",
                MAX_BACKUP_TOTAL_BYTES / (1024 * 1024 * 1024)
            )));
        }
    }
    if !manifest
        .entries
        .iter()
        .any(|entry| entry.name == "raggy.sqlite3")
    {
        return Err(CoreError::Artifact(
            "el respaldo no contiene raggy.sqlite3".to_string(),
        ));
    }

    let zip_names = zip_entry_names(&mut zip)?;
    if zip_names != names {
        return Err(CoreError::Artifact(
            "las entradas del ZIP no coinciden con el manifiesto".to_string(),
        ));
    }

    fs::create_dir_all(&target.home_dir).map_err(|source| CoreError::Io {
        path: target.home_dir.clone(),
        source,
    })?;
    // La carpeta base siempre debe ser escribible para actualizar settings.json,
    // incluso cuando data_dir apunte a otro volumen o directorio personalizado.
    let staging = private_tempdir_in(&target.home_dir).map_err(|source| CoreError::Io {
        path: target.home_dir.clone(),
        source,
    })?;

    // Extrae y verifica todo dentro de un directorio temporal privado; los
    // archivos activos no se sustituyen si el paquete es inválido.
    for entry in &manifest.entries {
        let destination = staging.path().join(&entry.name);
        if let Some(directory) = destination.parent() {
            fs::create_dir_all(directory).map_err(|source| CoreError::Io {
                path: directory.to_path_buf(),
                source,
            })?;
        }
        extract_entry_to_file(&mut zip, entry, &destination)?;
    }

    let staged_db = staging.path().join("raggy.sqlite3");
    let storage = raggy_storage::Storage::open(&staged_db)?;
    restore_managed_paths(&storage, staging.path())?;
    drop(storage);

    let mut config = if staging.path().join("settings.json").is_file() {
        AppConfig::load(&staging.path().join("settings.json"))?
    } else {
        AppConfig::load_or_default(&target.settings_path())
    };
    // Las credenciales del paquete no se importan: se conservan solo las que
    // ya estén en el settings.json local del destino (AppConfig::save las
    // fusiona desde el disco).
    for secret in config.api_keys.values_mut() {
        secret.zeroize();
    }
    config.api_keys.clear();
    let staged_settings = staging.path().join("settings.json");
    if staged_settings.is_file() {
        fs::remove_file(&staged_settings).map_err(|source| CoreError::Io {
            path: staged_settings,
            source,
        })?;
    }
    config.data_dir = Some(staging.path().to_path_buf());
    let restored_data_dir = staging.keep();
    config.data_dir = Some(restored_data_dir.clone());
    if let Err(error) = config.save(&target.settings_path()) {
        let _ = fs::remove_dir_all(&restored_data_dir);
        return Err(error);
    }

    Ok(BackupSummary {
        files: manifest.entries.len(),
        bytes: total_size,
        restored_data_dir: Some(restored_data_dir),
    })
}

fn read_entry_limited<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let file = zip
        .by_name(name)
        .map_err(|error| CoreError::Artifact(format!("falta «{name}» en el respaldo: {error}")))?;
    let mut buffer = Vec::new();
    if file.size() > limit {
        return Err(CoreError::Artifact(format!(
            "la entrada «{name}» excede el límite de {limit} bytes"
        )));
    }
    file.take(limit.saturating_add(1))
        .read_to_end(&mut buffer)
        .map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from(name),
            source,
        })?;
    if buffer.len() as u64 > limit {
        return Err(CoreError::Artifact(format!(
            "la entrada «{name}» excede el límite de {limit} bytes"
        )));
    }
    Ok(buffer)
}

fn zip_entry_names<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
) -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        let name = {
            let entry = zip.by_index(index).map_err(zip_error)?;
            entry.name().map_err(zip_error)?.into_owned()
        };
        if !names.insert(name.clone()) {
            return Err(CoreError::Artifact(format!(
                "el ZIP contiene una entrada duplicada: {name}"
            )));
        }
    }
    Ok(names)
}

fn extract_entry_to_file<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    entry: &ManifestEntry,
    destination: &Path,
) -> Result<()> {
    let mut file = zip.by_name(&entry.name).map_err(zip_error)?;
    if file.size() != entry.size || entry.size > MAX_BACKUP_FILE_BYTES {
        return Err(CoreError::Artifact(format!(
            "tamaño no válido en la entrada «{}» del respaldo",
            entry.name
        )));
    }

    let mut output = fs::File::create(destination).map_err(|source| CoreError::Io {
        path: destination.to_path_buf(),
        source,
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let mut input = file.by_ref().take(entry.size.saturating_add(1));
    loop {
        let read = input.read(&mut buffer).map_err(|source| CoreError::Io {
            path: std::path::PathBuf::from(&entry.name),
            source,
        })?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > entry.size || total > MAX_BACKUP_FILE_BYTES {
            return Err(CoreError::Artifact(format!(
                "la entrada «{}» excede su tamaño declarado",
                entry.name
            )));
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|source| CoreError::Io {
                path: destination.to_path_buf(),
                source,
            })?;
    }
    output.sync_all().map_err(|source| CoreError::Io {
        path: destination.to_path_buf(),
        source,
    })?;

    if total != entry.size || hasher.finalize().to_hex().as_str() != entry.hash {
        return Err(CoreError::Artifact(format!(
            "la entrada «{}» no supera la verificación de integridad",
            entry.name
        )));
    }
    Ok(())
}

fn is_safe_archive_name(name: &str) -> bool {
    if matches!(name, "raggy.sqlite3" | "settings.json") {
        return true;
    }
    ["uploads/", "artifacts/", "attachments/"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .is_some_and(is_safe_relative)
}

fn is_safe_relative(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains('\0')
        || name
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return false;
    }
    Path::new(name)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn restore_managed_paths(storage: &raggy_storage::Storage, data_dir: &Path) -> Result<()> {
    let uploads_dir = data_dir.join("uploads");
    let artifacts_dir = data_dir.join("artifacts");
    let attachments_dir = data_dir.join("attachments");
    fs::create_dir_all(&uploads_dir).map_err(|source| CoreError::Io {
        path: uploads_dir.clone(),
        source,
    })?;
    fs::create_dir_all(&artifacts_dir).map_err(|source| CoreError::Io {
        path: artifacts_dir.clone(),
        source,
    })?;
    fs::create_dir_all(&attachments_dir).map_err(|source| CoreError::Io {
        path: attachments_dir.clone(),
        source,
    })?;

    for folder in storage.list_folders()? {
        let normalized_path = folder.path.replace('\\', "/");
        if folder.name == "Archivos subidos" && normalized_path.ends_with("/uploads") {
            storage.update_folder_path(&folder.id, &uploads_dir.to_string_lossy())?;
        }

        for artifact in storage.list_artifacts_by_folder(&folder.id)? {
            // El ZIP ya contiene el archivo físico. Solo se reconstruye desde
            // SQLite para los respaldos antiguos que no incluían esa entrada.
            let relative = artifact_relative_path(&artifact)?;
            let restored_path = artifacts_dir.join(&relative);
            match fs::symlink_metadata(&restored_path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(CoreError::Artifact(format!(
                        "la ruta restaurada del artefacto no es un archivo: {}",
                        restored_path.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let outcome = save_artifact(
                        &artifacts_dir,
                        &relative,
                        &artifact.content,
                        ConflictPolicy::Fail,
                    )?;
                    if matches!(outcome, crate::artifacts::SaveOutcome::Exists(_)) {
                        return Err(CoreError::Artifact(format!(
                            "conflicto al reconstruir el artefacto: {}",
                            restored_path.display()
                        )));
                    }
                }
                Err(source) => {
                    return Err(CoreError::Io {
                        path: restored_path,
                        source,
                    });
                }
            }
            storage.update_artifact_path(&artifact.id, &restored_path.to_string_lossy())?;
        }
    }

    Ok(())
}

/// Devuelve `settings.json` sin las claves de API, para no filtrarlas (HU-29).
fn sanitized_settings(path: &Path) -> Result<Vec<u8>> {
    let mut raw = fs::read_to_string(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed = serde_json::from_str::<serde_json::Value>(&raw);
    raw.zeroize();
    let mut value = parsed?;
    if !value.is_object() {
        return Err(CoreError::Artifact(
            "settings.json no contiene un objeto de configuración".to_string(),
        ));
    }

    if let serde_json::Value::Object(map) = &mut value {
        if let Some(serde_json::Value::Object(mut keys)) = map.remove("api_keys") {
            for value in keys.values_mut() {
                if let serde_json::Value::String(secret) = value {
                    secret.zeroize();
                }
            }
        }
    }

    Ok(serde_json::to_vec_pretty(&value)?)
}

fn zip_error(error: zip::result::ZipError) -> CoreError {
    CoreError::Artifact(format!("error al procesar el paquete: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use raggy_storage::{Artifact, Folder, Session, Storage};

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

        let uploads_folder = Folder::new(
            source_paths.uploads_dir().to_string_lossy().to_string(),
            "Archivos subidos",
        );
        storage
            .upsert_folder(&uploads_folder)
            .expect("registra carpeta de subidas");
        fs::write(
            source_paths.uploads_dir().join("ventas.csv"),
            "id,valor\n1,10\n",
        )
        .expect("crea archivo subido");

        let session = Session::new("f1", "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");

        let attachment_dir = source_paths.attachments_dir().join(&session.id);
        fs::create_dir_all(&attachment_dir).expect("crea la carpeta de adjuntos");
        fs::write(
            attachment_dir.join("informe.md"),
            "# Informe\n\ncontenido adjunto\n",
        )
        .expect("crea el archivo adjunto");
        storage
            .connection()
            .execute(
                "INSERT INTO documents
                     (id, session_id, relative_path, content_hash, processing_hash, size_bytes,
                      modified_at, indexed_at, chunk_count)
                 VALUES ('d1', ?1, 'informe.md', 'h', 'p', 26, 0, '2026-01-01T00:00:00Z', 1)",
                [&session.id],
            )
            .expect("registra el adjunto");

        let artifact_path = source.join("salida/saludo.py");
        fs::create_dir_all(artifact_path.parent().expect("carpeta del artefacto"))
            .expect("crea carpeta del artefacto");
        fs::write(&artifact_path, "print('hola')  # edición física")
            .expect("escribe archivo físico editado");
        let artifact = Artifact::new(
            &session.id,
            "saludo.py",
            "src/saludo.py",
            artifact_path.to_string_lossy().to_string(),
            Some("python".to_string()),
            "print('hola')",
            Some("deepseek-chat".to_string()),
        );
        storage.add_artifact(&artifact).expect("guarda artefacto");

        let archive = source.join("respaldo.zip");
        let summary = export_backup(&storage, &source_paths, &archive).expect("exporta");
        assert_eq!(
            summary.files, 5,
            "db, settings, subida, adjunto y artefacto físico"
        );
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
        assert!(
            zip.file_names().any(|name| name
                .expect("nombre de entrada válido")
                .starts_with("artifacts/")),
            "el respaldo debe incluir los archivos físicos de artefactos"
        );
        assert!(
            zip.file_names().any(|name| name
                .expect("nombre de entrada válido")
                .starts_with("attachments/")),
            "el respaldo debe incluir los archivos adjuntos a los chats"
        );

        let target = temp_dir("destino");
        let target_paths = AppPaths::with_data_dir(&target, target.join("datos-personalizados"));
        target_paths.ensure_dirs().expect("crea destino activo");
        fs::write(
            target_paths.settings_path(),
            r#"{"theme":"light","api_keys":{"openai":"clave-destino"}}"#,
        )
        .expect("prepara clave de la instalación destino");
        let active_storage = Storage::open(&target_paths.db_path()).expect("abre db activa");
        active_storage
            .connection()
            .execute(
                "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
                 VALUES ('active', '/tmp/active', 'active', 'indexed', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("inserta datos activos");
        drop(active_storage);

        let restored = import_backup(&archive, &target_paths).expect("importa");
        assert_eq!(restored.files, summary.files);

        let restored_data_dir = restored
            .restored_data_dir
            .as_ref()
            .expect("import devuelve la carpeta restaurada");
        let restored_paths = AppPaths::with_data_dir(&target, restored_data_dir);
        assert!(restored_paths.uploads_dir().join("ventas.csv").is_file());
        assert!(
            restored_paths
                .attachments_dir()
                .join(&session.id)
                .join("informe.md")
                .is_file(),
            "el adjunto del chat se restaura en su carpeta"
        );
        let restored_config = AppConfig::load(&target_paths.settings_path()).expect("lee config");
        assert_eq!(restored_config.theme, crate::Theme::Dark);
        assert_eq!(
            restored_config.data_dir.as_deref(),
            Some(restored_data_dir.as_path())
        );
        assert_eq!(
            restored_config.api_keys.get("openai").map(String::as_str),
            Some("clave-destino"),
            "la importación conserva las claves locales y el respaldo no las trae"
        );

        let old_storage = Storage::open(&target_paths.db_path()).expect("db activa intacta");
        assert_eq!(
            old_storage.list_folders().expect("lista db activa")[0].name,
            "active",
            "la restauración no pisa la db activa"
        );
        drop(old_storage);

        let restored_storage = Storage::open(&restored_paths.db_path()).expect("abre restaurada");
        let folders = restored_storage.list_folders().expect("lista");
        assert_eq!(folders.len(), 2);
        let restored_uploads = folders
            .iter()
            .find(|folder| folder.name == "Archivos subidos")
            .expect("carpeta de subidas restaurada");
        assert_eq!(
            restored_uploads.path,
            restored_paths.uploads_dir().to_string_lossy()
        );
        let artifacts = restored_storage
            .list_artifacts_by_session(&session.id)
            .expect("lista artefactos restaurados");
        assert_eq!(artifacts.len(), 1);
        assert!(std::path::Path::new(&artifacts[0].path).is_file());
        assert_eq!(
            fs::read_to_string(&artifacts[0].path).expect("lee artefacto reconstruido"),
            "print('hola')  # edición física"
        );
        drop(restored_storage);

        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&target);
        let _ = fs::remove_dir_all(restored_data_dir);
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

    #[test]
    fn acepta_solo_nombres_de_entrada_permitidos() {
        assert!(is_safe_archive_name("raggy.sqlite3"));
        assert!(is_safe_archive_name("settings.json"));
        assert!(is_safe_archive_name("uploads/sub/ventas.csv"));
        assert!(is_safe_archive_name("artifacts/sesion/id/src/main.rs"));
        assert!(is_safe_archive_name("attachments/sesion/informe.pdf"));
        assert!(!is_safe_archive_name("../../afuera"));
        assert!(!is_safe_archive_name("attachments/../afuera"));
        assert!(!is_safe_archive_name("uploads/../../afuera"));
        assert!(!is_safe_archive_name("uploads/C:/Windows/file"));
        assert!(!is_safe_archive_name("artifacts/id/../../afuera"));
        assert!(!is_safe_archive_name("arbitrary.txt"));
    }
}
