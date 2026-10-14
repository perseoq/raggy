//! Punto de entrada de Raggy Desktop.
//!
//! Inicializa los registros (consola y archivo con rotación diaria), instala un
//! gancho de pánico, resuelve rutas, carga la configuración, abre la base de
//! datos con sus migraciones y lanza la ventana.

use std::path::Path;

use anyhow::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use raggy_core::{AppConfig, APP_NAME, APP_VERSION};
use raggy_storage::Storage;

fn main() -> anyhow::Result<()> {
    let paths =
        raggy_core::paths::resolve().context("no se pudo resolver el directorio de datos")?;
    paths
        .ensure_dirs()
        .context("no se pudieron crear los directorios de datos")?;

    // El guardián del registro en archivo debe vivir hasta el final del proceso.
    let _log_guard = init_tracing(&paths.logs_dir());
    install_panic_hook();

    let config = AppConfig::load_or_default(&paths.settings_path());
    if !paths.settings_path().exists() {
        if let Err(err) = config.save(&paths.settings_path()) {
            tracing::warn!("no se pudo escribir la configuración por defecto: {err}");
        }
    }

    let storage = Storage::open(&paths.db_path()).context("no se pudo abrir la base de datos")?;
    let schema_version = storage.schema_version().unwrap_or(-1);

    tracing::info!(
        version = APP_VERSION,
        home_dir = %paths.home_dir.display(),
        data_dir = %paths.data_dir.display(),
        settings = %paths.settings_path().display(),
        schema_version,
        providers = raggy_llm::ProviderId::ALL.len(),
        top_k = config.rag.top_k,
        "arrancando"
    );

    localmind_ui_run(config, paths)?;

    // Mantener el manejador de almacenamiento vivo durante toda la sesión.
    drop(storage);
    tracing::info!("{APP_NAME} finalizado");
    Ok(())
}

fn localmind_ui_run(config: AppConfig, paths: raggy_core::AppPaths) -> anyhow::Result<()> {
    raggy_ui::run(config, paths)
        .map_err(|err| anyhow::anyhow!("fallo al ejecutar la interfaz: {err}"))
}

/// Inicializa `tracing` con salida a consola y a un archivo con rotación diaria.
///
/// Devuelve el guardián del escritor asíncrono, que debe mantenerse vivo.
fn init_tracing(logs_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(std::io::stderr);

    match tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("raggy")
        .filename_suffix("log")
        .build(logs_dir)
    {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(writer);

            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(file_layer)
                .init();
            Some(guard)
        }
        Err(error) => {
            eprintln!("no se pudieron escribir registros en disco: {error}");
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .init();
            None
        }
    }
}

/// Registra los pánicos en el archivo de registros antes de abortar.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("pánico no controlado: {info}");
        default_hook(info);
    }));
}
