// On Windows do NOT show a console window when opening the app
#![cfg_attr(
    all(not(test), not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use cntp_i18n::{I18N_MANAGER, tr_load};
use gpui::set_trace_enabled;
#[cfg(not(any(target_os = "macos", target_os = "wasi")))]
use std::path::Path;
use std::sync::LazyLock;

use crate::media::{
    builtin::{lofty::LoftyProvider, symphonia::SymphoniaProvider},
    lookup_table::add_provider,
};

mod devices;
mod errors;
mod library;
mod logging;
mod media;
mod paths;
mod playback;
mod power;
mod services;
mod settings;
#[cfg(test)]
mod test_support;
mod toasts;
mod ui;
#[cfg(feature = "update")]
mod update;
mod util;
#[cfg(target_os = "windows")]
mod windows;

const VERSION_STRING: &str = env!("HUMMINGBIRD_VERSION_STRING");

// count allocations during testing, needed for testing the allocation behavior of the playback
// pipeline
#[cfg(test)]
#[global_allocator]
static ALLOC_GUARD: test_support::alloc_guard::CountingAllocator =
    test_support::alloc_guard::CountingAllocator;

#[cfg(all(not(test), not(feature = "heap-profileable"), not(target_os = "wasi")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    // WaOS port: no I/O driver, whose mio has no WASI 0.3 backend; the time driver and tokio's
    // blocking pool run on WaOS's cooperative threads.
    #[cfg(target_os = "wasi")]
    builder.enable_time();
    #[cfg(not(target_os = "wasi"))]
    builder.enable_all();
    builder
        .worker_threads(2)
        .max_blocking_threads(12)
        .build()
        .unwrap()
});

#[cfg(not(target_os = "wasi"))]
fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    windows::init()?;
    set_up()?;
    crate::ui::app::run()
}

// WaOS port: the program's entry, which runs GPUI over wasdk until the app quits.
#[cfg(target_os = "wasi")]
#[wasdk::main]
async fn main() {
    if let Err(error) = set_up() {
        eprintln!("hummingbird: {error:#}");
        return;
    }
    if let Err(error) = crate::ui::app::run().await {
        eprintln!("hummingbird: {error:#}");
    }
}

/// What the app needs before its UI: translations, logging and the media providers.
fn set_up() -> anyhow::Result<()> {
    // disable the GPUI mini profiler immediately to avoid unnecessary allocations
    set_trace_enabled(false);

    I18N_MANAGER.load_source(tr_load!());
    crate::logging::init()?;

    // do this even when updating is disabled since it doesn't really hurt anything
    #[cfg(not(any(target_os = "macos", target_os = "wasi")))]
    spawn_update_temp_cleanup();

    tracing::info!("version {VERSION_STRING}");

    add_provider(Box::new(LoftyProvider));
    add_provider(Box::new(SymphoniaProvider));
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "wasi")))]
fn spawn_update_temp_cleanup() {
    crate::RUNTIME.spawn(async {
        if let Err(error) = cleanup_update_temp_dirs().await {
            tracing::warn!("Failed to clean up stale update temp directories: {error:?}");
        }
    });
}

#[cfg(not(any(target_os = "macos", target_os = "wasi")))]
async fn cleanup_update_temp_dirs() -> std::io::Result<()> {
    let temp_dir = std::env::temp_dir();
    let mut entries = tokio::fs::read_dir(&temp_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if !is_update_temp_dir(&path) {
            continue;
        }

        match entry.file_type().await {
            Ok(file_type) if file_type.is_dir() => {
                if let Err(error) = tokio::fs::remove_dir_all(&path).await {
                    tracing::warn!(
                        "Failed to remove stale update temp directory {}: {error:?}",
                        path.display()
                    );
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    "Failed to inspect stale update temp directory candidate {}: {error:?}",
                    path.display()
                );
            }
        }
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "wasi")))]
fn is_update_temp_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name.starts_with("hb-update-"))
}
