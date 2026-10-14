//! Almacenamiento seguro de API Keys (HU-23).
//!
//! Las claves se guardan en el almacén de credenciales del sistema operativo
//! (Windows Credential Manager, macOS Keychain o Secret Service en Linux), nunca
//! en la configuración ni en la base de datos. Las variables de entorno se usan
//! como alternativa cuando no hay llavero disponible.

use std::collections::HashMap;
use std::sync::Mutex;

use raggy_llm::ProviderId;

/// Error del almacén de secretos.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// El almacén del sistema no está accesible.
    #[error("no se pudo acceder al almacén de credenciales del sistema: {0}")]
    Store(String),
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

/// Almacén basado en el llavero del sistema operativo.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    /// Crea el almacén con el nombre de servicio por defecto.
    pub fn new() -> Self {
        Self::with_service("raggy-desktop")
    }

    /// Crea el almacén con un nombre de servicio concreto.
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, provider: ProviderId) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, provider.key()).map_err(store_error)
    }

    /// ¿Está disponible el almacén del sistema?
    pub fn is_available() -> bool {
        keyring::Entry::store_status().is_ok()
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, provider: ProviderId) -> Result<Option<String>> {
        match self.entry(provider)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn set(&self, provider: ProviderId, secret: &str) -> Result<()> {
        self.entry(provider)?
            .set_password(secret)
            .map_err(store_error)
    }

    fn delete(&self, provider: ProviderId) -> Result<()> {
        match self.entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        }
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

fn store_error(error: keyring::Error) -> SecretError {
    SecretError::Store(error.to_string())
}

fn locked() -> SecretError {
    SecretError::Store("el almacén en memoria está bloqueado".to_string())
}

/// Resuelve la clave del proveedor: primero el llavero, después el entorno.
pub fn resolve_key(store: &dyn SecretStore, provider: ProviderId) -> Option<String> {
    store
        .get(provider)
        .ok()
        .flatten()
        .or_else(|| raggy_llm::api_key(provider))
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

    /// Verifica el llavero real del sistema. Se ignora por defecto porque no
    /// todos los entornos tienen un almacén de credenciales disponible.
    #[test]
    #[ignore = "requiere el llavero del sistema"]
    fn el_llavero_del_sistema_guarda_y_recupera() {
        if !KeyringStore::is_available() {
            eprintln!("el llavero del sistema no está disponible; se omite");
            return;
        }

        let store = KeyringStore::with_service("raggy-desktop-test");
        let provider = ProviderId::OpenAi;
        store.set(provider, "clave-de-prueba").expect("guarda");
        assert_eq!(
            store.get(provider).expect("lee"),
            Some("clave-de-prueba".to_string())
        );
        store.delete(provider).expect("borra");
        assert_eq!(store.get(provider).expect("lee"), None);
    }
}
