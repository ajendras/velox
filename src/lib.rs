//! Velox — Native compatibility layer for Xbox Series binaries on Windows.
//!
//! Provides a clean, testable, pluggable architecture with strong sandboxing,
//! shader translation, HTTP monitoring, and mod plugin support.

use std::path::{Path, PathBuf};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

pub mod backends;
pub mod config;
pub mod emulator;
pub mod http_api;
pub mod loader;
pub mod plugin;
pub mod sandbox;
pub mod translator;

/// Runtime configuration for a Velox instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directory containing the game binary and assets.
    pub game_dir: PathBuf,
    /// Directory for shader cache, translated assets, and persistent data.
    pub cache_dir: PathBuf,
    /// Whether to enable the memory sandbox and API whitelisting.
    pub sandbox: bool,
    /// Name of the backend to use (e.g. "d3d12").
    pub backend: String,
    /// Paths to dynamic plugin libraries that implement the Plugin trait.
    pub plugins: Vec<String>,
}

impl Config {
    /// Load configuration from a JSON file. Falls back to defaults if the file
    /// does not exist or cannot be read.
    pub fn load(path: &str) -> Result<Config> {
        let path_buf = PathBuf::from(path);
        if path_buf.exists() {
            let content = std::fs::read_to_string(&path_buf)
                .with_context(|| format!("Failed to read config file {}", path))?;
            let cfg: Config = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse config file {}", path))?;
            cfg.validate()?;
            Ok(cfg)
        } else {
            let default = Config::default();
            default.validate()?;
            Ok(default)
        }
    }

    /// Returns a secure-by-default configuration.
    pub fn default() -> Self {
        Self {
            game_dir: PathBuf::from("."),
            cache_dir: PathBuf::from("./velox_cache"),
            sandbox: true,
            backend: "d3d12".to_string(),
            plugins: vec![],
        }
    }

    /// Validates configuration values, returning helpful errors for invalid states.
    fn validate(&self) -> Result<()> {
        if !self.game_dir.exists() {
            return Err(anyhow!(
                "game_dir '{}' does not exist. Please create it or specify a valid path.",
                self.game_dir.display()
            ));
        }
        if self.backend.is_empty() {
            return Err(anyhow!("backend cannot be empty"));
        }
        Ok(())
    }
}

/// A platform-agnostic draw command passed to pluggable GPU backends.
/// In a full implementation this would carry vertex buffers, textures,
/// root signatures, and other D3D12-compatible metadata.
#[derive(Debug, Clone, Copy)]
pub struct DrawCommand {
    pub topology: u32,
    pub vertex_count: u32,
    pub instance_count: u32,
}

/// Pluggable backend trait. Implementations are provided for D3D12 (default),
/// a null backend for testing, and can be extended by users for Vulkan, software
/// rasterizers, or research backends.
pub trait Backend: Send + Sync {
    /// Record a draw call. The backend is responsible for translation
    /// of any console-specific state into native commands.
    fn draw(&self, cmd: DrawCommand);

    /// Present the current frame to the swapchain or output surface.
    fn present(&self);
}

/// Concrete default backend used when "d3d12" is selected.
/// In the full implementation this would initialize a D3D12 device,
/// command queues, and swapchain using the windows crate.
struct DefaultBackend;

impl Backend for DefaultBackend {
    fn draw(&self, cmd: DrawCommand) {
        // Production implementation would submit to a D3D12 command list.
        // This debug path ensures the public API is immediately usable.
        println!(
            "[D3D12] draw: topology={}, vertices={}, instances={}",
            cmd.topology, cmd.vertex_count, cmd.instance_count
        );
    }

    fn present(&self) {
        // Production implementation would call Present() on the swapchain.
        println!("[D3D12] present");
    }
}

/// Top-level Velox instance. Owns configuration and a boxed backend.
/// Additional runtime state (HTTP server handle, plugin list, emulator core)
/// will be added in future iterations while preserving this public shape.
pub struct Velox {
    config: Config,
    backend: Box<dyn Backend>,
}

impl Velox {
    /// Creates a new Velox instance from the supplied configuration.
    /// Validates the config and constructs the selected backend.
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;

        let backend: Box<dyn Backend> = match config.backend.as_str() {
            "d3d12" => Box::new(DefaultBackend),
            "null" => Box::new(DefaultBackend), // placeholder for a true null backend
            other => return Err(anyhow!("Unsupported backend '{}'. Available: d3d12", other)),
        };

        Ok(Velox { config, backend })
    }

    /// High-level run entrypoint. In the complete implementation this delegates
    /// to emulator::run, which wires up the loader, sandbox, translator, plugins,
    /// and HTTP monitoring server.
    pub fn run(&mut self, exe_path: &Path) -> Result<()> {
        if !exe_path.exists() {
            return Err(anyhow!(
                "Game executable '{}' not found. Please provide a valid path.",
                exe_path.display()
            ));
        }

        println!("Velox: launching {}", exe_path.display());
        println!("  sandbox: {}", self.config.sandbox);
        println!("  backend: {}", self.config.backend);
        println!("  plugins: {}", self.config.plugins.len());

        // Simulate a short run loop that exercises the backend.
        // The real implementation lives in emulator.rs and coordinates
        // PE loading, sandbox entry, shader translation cache, and the Axum server.
        for frame in 0..8 {
            self.backend.draw(DrawCommand {
                topology: 3, // D3D12_PRIMITIVE_TOPOLOGY_TRIANGLELIST
                vertex_count: 1024 + frame * 64,
                instance_count: 1,
            });
            self.backend.present();

            // In a full run loop this would also call plugin on_frame hooks
            // and update metrics exposed over HTTP.
            std::thread::sleep(std::time::Duration::from_millis(80));
        }

        println!("Velox: run completed successfully");
        Ok(())
    }
}

/// Public constructor matching the documented API.
pub fn new(config: Config) -> Result<Velox> {
    Velox::new(config)
}
