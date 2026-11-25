use std::path::PathBuf;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

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
    /// does not exist or cannot be read. Creates cache directories on load.
    pub fn load(path: &str) -> Result<Config> {
        let path_buf = PathBuf::from(path);
        if path_buf.exists() {
            let content = std::fs::read_to_string(&path_buf)
                .with_context(|| format!("Failed to read config file {}", path))?;
            let cfg: Config = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse config file {}", path))?;
            cfg.validate()?;
            cfg.ensure_cache_dirs()?;
            Ok(cfg)
        } else {
            let default = Config::default();
            default.validate()?;
            default.ensure_cache_dirs()?;
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
        if self.cache_dir.exists() && !self.cache_dir.is_dir() {
            return Err(anyhow!(
                "cache_dir '{}' exists but is not a directory",
                self.cache_dir.display()
            ));
        }
        Ok(())
    }

    /// Ensures the cache directory tree exists, creating `cache_dir` and
    /// the `shaders` subdirectory if necessary. Idempotent and safe to call
    /// multiple times.
    pub fn ensure_cache_dirs(&self) -> Result<()> {
        let shader_dir = self.cache_dir.join("shaders");
        std::fs::create_dir_all(&shader_dir)
            .with_context(|| format!("Failed to create cache directory at {}", self.cache_dir.display()))?;
        Ok(())
    }

    /// Returns the full path to the shader cache directory (does not create it).
    pub fn shader_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("shaders")
    }
}
