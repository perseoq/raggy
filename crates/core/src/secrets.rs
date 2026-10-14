//! Configuración y claves de API (HU-23, HU-29).
//!
//! Las claves viven en `settings.json`, dentro de la carpeta base de la
//! aplicación (`~/.raggydesktop/settings.json`), junto al resto de la
//! configuración. El archivo se escribe con permisos restringidos al propietario
//! cuando el sistema lo permite, y las variables de entorno siguen sirviendo como
//! alternativa. Las claves nunca se incrustan en el código ni en los registros.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use raggy_llm::ProviderId;

/// Error del almacén de secretos.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No se pudo acceder al archivo de configuración.
    #[error("no se pudo acceder a settings.json: {0}")]
    Store(String),
}

/// Clave de API que no se filtra en los registros (HU-29).
///
/// El `Debug` y el `Display` redactan el valor, de modo que un `tracing::warn!`
/// accidental nunca expone la credencial.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Envuelve una clave.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Devuelve el valor real; usar solo al construir la petición al proveedor.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// ¿La clave está vacía?
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(***)")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Borra la clave de la memoria al liberarla (HU-29).
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

/// Alias de resultado para el almacén de secretos.
pub type Result<T> = std::result::Result<T, SecretError>;

/// Contrato del almacén de secretos.
pub trait SecretStore: Send + Sync {
    /// Recupera la clave del proveedor, si existe.
    fn get(&self, provider: ProviderId) -> Result<Option<String>>;

    /// Guarda (o reemplaza) la clave del proveedor.
    fn set(&self, provider: ProviderId, secret: &str) -> Result<()>;

    /// Elimina la clave del proveedor.
    fn delete(&self, provider: ProviderId) -> Result<()>;
}

/// Almacén basado en el archivo `settings.json` (campo `api_keys`).
///
/// Lee y escribe solo ese campo, conservando el resto de la configuración.
pub struct SettingsStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SettingsStore {
    /// Crea el almacén sobre la ruta de `settings.json`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn read_keys(&self) -> Result<HashMap<String, String>> {
        let _guard = self.lock.lock().map_err(|_| locked())?;
        Ok(read_keys(&self.path))
    }

    fn write_keys(&self, keys: &HashMap<String, String>) -> Result<()> {
        let _guard = self.lock.lock().map_err(|_| locked())?;
        write_keys(&self.path, keys)
    }
}

impl SecretStore for SettingsStore {
    fn get(&self, provider: ProviderId) -> Result<Option<String>> {
        Ok(self.read_keys()?.get(provider.key()).cloned())
    }

    fn set(&self, provider: ProviderId, secret: &str) -> Result<()> {
        let mut keys = self.read_keys()?;
        keys.insert(provider.key().to_string(), secret.to_string());
        self.write_keys(&keys)
    }

    fn delete(&self, provider: ProviderId) -> Result<()> {
        let mut keys = self.read_keys()?;
        keys.remove(provider.key());
        self.write_keys(&keys)
    }
}

/// Almacén en memoria, para pruebas.
#[derive(Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<&'static str, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, provider: ProviderId) -> Result<Option<String>> {
        let secrets = self.secrets.lock().map_err(|_| locked())?;
        Ok(secrets.get(provider.key()).cloned())
    }

    fn set(&self, provider: ProviderId, secret: &str) -> Result<()> {
        let mut secrets = self.secrets.lock().map_err(|_| locked())?;
        secrets.insert(provider.key(), secret.to_string());
        Ok(())
    }

    fn delete(&self, provider: ProviderId) -> Result<()> {
        let mut secrets = self.secrets.lock().map_err(|_| locked())?;
        secrets.remove(provider.key());
        Ok(())
    }
}

/// Lee el documento completo, o un objeto vacío si no existe o no es válido.
fn read_document(path: &Path) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()))
}

fn read_keys(path: &Path) -> HashMap<String, String> {
    read_document(path)
        .get("api_keys")
        .and_then(|keys| keys.as_object())
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn write_keys(path: &Path, keys: &HashMap<String, String>) -> Result<()> {
    let mut document = read_document(path);
    let object: serde_json::Map<String, serde_json::Value> = keys
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
        .collect();

    if let serde_json::Value::Object(map) = &mut document {
        map.insert("api_keys".to_string(), serde_json::Value::Object(object));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| SecretError::Store(error.to_string()))?;
    }

    let serialized = serde_json::to_string_pretty(&document)
        .map_err(|error| SecretError::Store(error.to_string()))?;
    std::fs::write(path, serialized).map_err(|error| SecretError::Store(error.to_string()))?;
    crate::config::restrict_permissions(path);
    Ok(())
}

fn locked() -> SecretError {
    SecretError::Store("el almacén está bloqueado".to_string())
}

/// Resuelve la clave del proveedor: primero `settings.json`, después el entorno.
pub fn resolve_key(store: &dyn SecretStore, provider: ProviderId) -> Option<Secret> {
    store
        .get(provider)
        .ok()
        .flatten()
        .or_else(|| raggy_llm::api_key(provider))
        .filter(|value| !value.trim().is_empty())
        .map(Secret::new)
}

/// Enmascara una clave mostrando solo sus últimos cuatro caracteres (HU-23).
pub fn mask(secret: &str) -> String {
    let total = secret.chars().count();
    if total <= 4 {
        return "•".repeat(total.max(1));
    }
    let visible: String = secret.chars().skip(total - 4).collect();
    let hidden = "•".repeat((total - 4).min(16));
    format!("{hidden}{visible}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path.join("settings.json")
    }

    #[test]
    fn enmascara_dejando_los_ultimos_cuatro() {
        let masked = mask("sk-1234567890abcd");
        assert!(masked.ends_with("abcd"), "{masked}");
        assert!(masked.starts_with("•"), "{masked}");
        assert!(!masked.contains("1234567890"), "{masked}");

        assert_eq!(mask("abcd"), "••••");
        assert_eq!(mask(""), "•");
    }

    #[test]
    fn el_secreto_no_se_filtra_en_logs() {
        let secret = Secret::new("sk-super-secreto");
        assert_eq!(format!("{secret}"), "***");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert!(!format!("{secret:?}").contains("secreto"));
        assert_eq!(secret.expose(), "sk-super-secreto");
    }

    #[test]
    fn el_almacen_en_memoria_guarda_recupera_y_borra() {
        let store = MemoryStore::default();
        let provider = ProviderId::DeepSeek;

        assert_eq!(store.get(provider).expect("lee"), None);
        store.set(provider, "secreto").expect("guarda");
        assert_eq!(
            store.get(provider).expect("lee"),
            Some("secreto".to_string())
        );
        store.delete(provider).expect("borra");
        assert_eq!(store.get(provider).expect("lee"), None);
    }

    #[test]
    fn settings_json_guarda_recupera_y_borra() {
        let path = temp_settings("store");
        let store = SettingsStore::new(&path);
        let provider = ProviderId::DeepSeek;

        assert_eq!(store.get(provider).expect("lee"), None);
        store.set(provider, "token-de-prueba").expect("guarda");
        assert_eq!(
            store.get(provider).expect("lee"),
            Some("token-de-prueba".to_string())
        );

        let raw = std::fs::read_to_string(&path).expect("lee archivo");
        assert!(raw.contains("\"api_keys\""), "{raw}");

        store.delete(provider).expect("borra");
        assert_eq!(store.get(provider).expect("lee"), None);

        let _ = std::fs::remove_dir_all(path.parent().expect("padre"));
    }

    #[test]
    fn resolve_key_usa_settings_json() {
        let path = temp_settings("resolve");
        let store = SettingsStore::new(&path);
        store
            .set(ProviderId::DeepSeek, "clave-del-archivo")
            .expect("guarda");

        let resolved = resolve_key(&store, ProviderId::DeepSeek).expect("resuelve");
        assert_eq!(resolved.expose(), "clave-del-archivo");
    }

    #[test]
    fn settings_json_conserva_el_resto_de_la_configuracion() {
        let path = temp_settings("preserva");
        std::fs::create_dir_all(path.parent().expect("padre")).expect("crea carpeta");
        std::fs::write(
            &path,
            "{\n  \"theme\": \"dark\",\n  \"language\": \"es\"\n}",
        )
        .expect("escribe");

        let store = SettingsStore::new(&path);
        store.set(ProviderId::OpenAi, "otro-token").expect("guarda");

        let raw = std::fs::read_to_string(&path).expect("lee");
        assert!(raw.contains("dark"), "debe conservar el tema: {raw}");
        assert!(raw.contains("otro-token"), "debe incluir la clave: {raw}");
        assert_eq!(
            store.get(ProviderId::OpenAi).expect("lee"),
            Some("otro-token".to_string())
        );

        let _ = std::fs::remove_dir_all(path.parent().expect("padre"));
    }
}
