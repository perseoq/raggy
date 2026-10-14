//! Punto de entrada de LocalMind RAG Desktop.
//!
//! Fase 0: inicializa el registro de eventos, resuelve rutas, carga la
//! configuración, abre la base de datos con sus migraciones y lanza la ventana.

use anyhow::Context;

use localmind_core::{AppConfig, AppPaths, APP_NAME, APP_VERSION};
use localmind_storage::Storage;

fn main() -> anyhow::Result<()> {
    init_tracing();

    let paths = AppPaths::resolve().context("no se pudo resolver el directorio de datos")?;
    paths
        .ensure_dirs()
        .context("no se pudieron crear los directorios de datos")?;

    let config = AppConfig::load_or_default(&paths.config_path());
    if !paths.config_path().exists() {
        if let Err(err) = config.save(&paths.config_path()) {
            tracing::warn!("no se pudo escribir la configuración por defecto: {err}");
        }
    }

    let storage = Storage::open(&paths.db_path()).context("no se pudo abrir la base de datos")?;
    let schema_version = storage.schema_version().unwrap_or(-1);

    tracing::info!(
        version = APP_VERSION,
        data_dir = %paths.data_dir.display(),
        schema_version,
        providers = localmind_llm::ProviderId::ALL.len(),
        top_k = localmind_rag::RetrieverOptions::default().top_k,
        "cimientos cargados"
    );

    tracing::info!("arrancando interfaz");
    localmind_ui::run(config, paths)
        .map_err(|err| anyhow::anyhow!("fallo al ejecutar la interfaz: {err}"))?;

    // Mantener el manejador de almacenamiento vivo durante toda la sesión.
    drop(storage);
    tracing::info!("{APP_NAME} finalizado");
    Ok(())
}

/// Inicializa `tracing` respetando la variable de entorno `RUST_LOG`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
