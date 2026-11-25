use crate::emulator::GameState;
use anyhow::{anyhow, Context, Result};
use libloading::Library;
use std::path::Path;
use std::sync::Arc;

/// Plugin trait. Every mod implements this to receive per-frame callbacks.
/// The trait is object-safe and `Send + Sync` so it can be stored in a `Vec<Box<dyn Plugin>>`.
pub trait Plugin: Send + Sync {
    /// Called once per rendered frame. The implementation may mutate internal
    /// state, log telemetry, apply runtime patches, or drive overlay rendering.
    fn on_frame(&mut self, state: &GameState);
}

/// Default plugin that logs high-level metrics every 30 frames. Serves as both
/// a safety net when no user plugins are configured and a concrete example
/// for developers writing their own DLL-based mods.
#[derive(Default)]
pub struct LoggerPlugin;

impl Plugin for LoggerPlugin {
    fn on_frame(&mut self, state: &GameState) {
        if state.frame_count % 30 == 0 {
            println!(
                "[velox:logger] frame={} fps={:.1} mem={}MB gpu={:.1}% violations={}",
                state.frame_count,
                state.fps,
                state.memory_mb,
                state.gpu_usage,
                state.sandbox_violations
            );
        }
    }
}

/// Thin wrapper that ties a dynamically loaded library to its plugin instance.
/// The `Arc<Library>` guarantees the DLL remains mapped for the entire lifetime
/// of the plugin.
struct DynamicPlugin {
    _lib: Arc<Library>,
    inner: Box<dyn Plugin>,
}

impl Plugin for DynamicPlugin {
    fn on_frame(&mut self, state: &GameState) {
        self.inner.on_frame(state);
    }
}

/// Loads zero or more dynamic plugins from the supplied DLL paths.
///
/// * A built-in `LoggerPlugin` is **always** installed first.
/// * For each path we attempt to load the library with `libloading`.
/// * We look for an exported constructor symbol `velox_create_plugin`.
/// * If the symbol is missing or the DLL cannot be loaded we emit a clear
///   error but continue so a single bad plugin does not prevent the game
///   from launching.
///
/// The function is infallible from the caller's perspective (`Vec` is always
/// returned) to keep the emulator startup robust.
pub fn load_plugins(paths: &[String]) -> Vec<Box<dyn Plugin>> {
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::with_capacity(paths.len() + 1);

    // Built-in logger is always present
    plugins.push(Box::new(LoggerPlugin));

    for path_str in paths {
        match load_dynamic(path_str) {
            Ok(p) => {
                println!("Velox: successfully loaded plugin from {}", path_str);
                plugins.push(p);
            }
            Err(e) => {
                eprintln!("Velox: plugin load failed for {}: {}", path_str, e);
            }
        }
    }

    plugins
}

fn load_dynamic(path_str: &str) -> Result<Box<dyn Plugin>> {
    let path = Path::new(path_str);
    if !path.exists() {
        return Err(anyhow!("Plugin DLL not found: {}", path_str));
    }

    let lib = unsafe { Library::new(path) }
        .with_context(|| format!("libloading failed for {}", path_str))?;

    let lib_arc = Arc::new(lib);

    // Production plugins export this exact symbol. The function must return
    // a boxed trait object allocated inside the DLL.
    let symbol_name = b"velox_create_plugin\0";
    let constructor: libloading::Symbol<unsafe extern "C" fn() -> *mut dyn Plugin> =
        unsafe { lib_arc.get(symbol_name) }
            .map_err(|e| anyhow!("Symbol velox_create_plugin not found: {}", e))?;

    let raw = unsafe { constructor() };
    if raw.is_null() {
        return Err(anyhow!("Plugin constructor returned null pointer"));
    }

    let boxed = unsafe { Box::from_raw(raw) };
    let dynamic = DynamicPlugin {
        _lib: lib_arc,
        inner: boxed,
    };

    Ok(Box::new(dynamic))
}

/// Returns a minimal set of plugins for unit and integration tests.
/// Guarantees deterministic behaviour without touching the filesystem.
pub fn test_plugins() -> Vec<Box<dyn Plugin>> {
    vec![Box::new(LoggerPlugin)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::GameState;

    #[test]
    fn test_logger_plugin_logs_without_crashing() {
        let mut plugin = LoggerPlugin;
        let state = GameState::new();
        // We call it many times; the modulo guard prevents spam in test output
        for i in 0..100 {
            let mut s = state.clone();
            s.frame_count = i;
            plugin.on_frame(&s);
        }
    }

    #[test]
    fn test_load_plugins_always_returns_at_least_logger() {
        let plugins = load_plugins(&[]);
        assert!(!plugins.is_empty());
        // The first plugin is always the logger
        let _first = &plugins[0];
    }

    #[test]
    fn test_load_plugins_with_missing_path_emits_error_but_continues() {
        let plugins = load_plugins(&["/non/existent/plugin.dll".to_string()]);
        assert_eq!(plugins.len(), 1); // only the built-in logger
    }
}
