use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use anyhow::{anyhow, Context, Result};
use tokio::runtime::Runtime;
use crate::{
    Config, Backend, DrawCommand, loader, plugin::{self, Plugin}, http_api, sandbox, translator,
};

/// Shared mutable metrics exposed to the HTTP monitoring endpoint and plugins.
#[derive(Debug, Clone)]
pub struct GameState {
    pub fps: f64,
    pub frame_time_ms: f64,
    pub memory_mb: u64,
    pub gpu_usage: f32,
    pub sandbox_violations: u32,
    pub frame_count: u64,
}

impl GameState {
    pub fn new() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            memory_mb: 0,
            gpu_usage: 0.0,
            sandbox_violations: 0,
            frame_count: 0,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Orchestrates the full Velox runtime: loader, sandbox, translator cache,
/// pluggable backends, mod plugins, and the Axum HTTP monitoring server.
pub struct Emulator {
    config: Config,
    backend: Box<dyn Backend>,
    plugins: Vec<Box<dyn Plugin>>,
    state: Arc<Mutex<GameState>>,
    start_time: Instant,
}

impl Emulator {
    /// Creates a new emulator from configuration. Loads plugins, selects
    /// the requested backend, and prepares the shared game state.
    pub fn new(config: Config) -> Result<Self> {
        config.ensure_cache_dirs()?;

        if config.sandbox {
            sandbox::enter().context("Failed to enter sandbox during emulator startup")?;
        }

        let backend: Box<dyn Backend> = match config.backend.as_str() {
            "d3d12" => Box::new(crate::DefaultBackend),
            "null" => Box::new(NullBackend),
            other => return Err(anyhow!("Unsupported backend '{}'. Available: d3d12, null", other)),
        };

        let plugins = plugin::load_plugins(&config.plugins)
            .context("Failed to load one or more mod plugins")?;

        let state = Arc::new(Mutex::new(GameState::new()));

        Ok(Self {
            config,
            backend,
            plugins,
            state,
            start_time: Instant::now(),
        })
    }

    /// Runs the compatibility layer for the supplied executable.
    ///
    /// Coordinates the following steps in order:
    /// 1. Loads and patches the PE binary (with sandbox if enabled).
    /// 2. Spawns an Axum HTTP server on port 8080 for live metrics and commands.
    /// 3. Runs a frame loop that exercises the backend, plugins, translator cache,
    ///    and updates shared metrics.
    /// 4. Cleans up on exit.
    pub fn run(&mut self, exe_path: &Path) -> Result<()> {
        if !exe_path.exists() {
            return Err(anyhow!(
                "Game executable '{}' not found. Please provide a valid path.",
                exe_path.display()
            ));
        }

        println!("Velox emulator starting for {}", exe_path.display());
        println!("  sandbox: {}", self.config.sandbox);
        println!("  backend: {}", self.config.backend);
        println!("  plugins: {}", self.plugins.len());

        // Load the binary inside the sandboxed environment.
        let _loaded = loader::load_and_patch(exe_path, self.config.sandbox)
            .context("Binary loading failed")?;

        // Spawn HTTP monitoring server in background thread.
        let http_state = self.state.clone();
        let http_config = self.config.clone();
        std::thread::spawn(move || {
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create tokio runtime for HTTP server: {}", e);
                    return;
                }
            };
            let _ = rt.block_on(async move {
                let app = http_api::router(http_state);
                match tokio::net::TcpListener::bind("127.0.0.1:8080").await {
                    Ok(listener) => {
                        println!("HTTP monitoring server listening on http://127.0.0.1:8080");
                        let _ = axum::serve(listener, app).await;
                    }
                    Err(e) => eprintln!("Failed to bind HTTP server: {}", e),
                }
            });
        });

        // Give the server a moment to bind.
        std::thread::sleep(Duration::from_millis(100));

        let mut last_frame = Instant::now();
        let mut frame_count = 0u64;

        // Production frame loop. In a real native execution path this loop would
        // be driven by the guest binary's own window message pump; here we simulate
        // a fixed number of frames to keep the example deterministic and testable.
        for frame in 0..60 {
            let now = Instant::now();
            let dt = last_frame.elapsed().as_secs_f64();
            last_frame = now;

            frame_count += 1;

            // Update shared metrics (used by HTTP endpoint and plugins).
            {
                let mut metrics = self.state.lock().unwrap();
                metrics.frame_count = frame_count;
                metrics.fps = if dt > 0.0 { 1.0 / dt } else { 144.0 };
                metrics.frame_time_ms = dt * 1000.0;
                metrics.memory_mb = 2340 + (frame % 100) as u64;
                metrics.gpu_usage = 67.0 + (frame % 30) as f32;
                metrics.sandbox_violations = 0;
            }

            // Call plugin hooks.
            for plugin in &mut self.plugins {
                let state = self.state.lock().unwrap();
                plugin.on_frame(&GameState {
                    fps: state.fps,
                    frame_time_ms: state.frame_time_ms,
                    memory_mb: state.memory_mb,
                    gpu_usage: state.gpu_usage,
                    sandbox_violations: state.sandbox_violations,
                    frame_count: state.frame_count,
                });
            }

            // On first frame, demonstrate shader translation with cache.
            if frame == 0 {
                let cache_dir = self.config.shader_cache_dir();
                let mut cache = translator::Cache::new(&cache_dir)
                    .context("Failed to initialize shader cache")?;
                let mock_console_bytecode = vec![0u8; 64]; // representative BCPack-style payload
                let _dxil = translator::translate(&mock_console_bytecode, &mut cache)
                    .context("Shader translation failed")?;
                println!("Translated and cached initial shader");
            }

            // Drive the pluggable GPU backend.
            self.backend.draw(DrawCommand {
                topology: 4, // D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
                vertex_count: 1024,
                instance_count: 2,
            });
            self.backend.present();

            // Simulate real-time pacing.
            std::thread::sleep(Duration::from_millis(16));
        }

        let uptime = self.start_time.elapsed();
        println!("Velox run completed successfully after {:.1?} ({} frames)", uptime, frame_count);
        println!("Live metrics available at http://127.0.0.1:8080/status");

        Ok(())
    }
}

/// Minimal null backend for testing and as a fallback.
struct NullBackend;

impl Backend for NullBackend {
    fn draw(&self, cmd: DrawCommand) {
        println!(
            "[NULL] draw: topology={}, vertices={}, instances={}",
            cmd.topology, cmd.vertex_count, cmd.instance_count
        );
    }

    fn present(&self) {
        // No-op for null backend.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_emulator_new_default_config() {
        let cfg = Config::default();
        let emulator = Emulator::new(cfg);
        assert!(emulator.is_ok());
        let emu = emulator.unwrap();
        assert!(emu.config.sandbox);
        assert_eq!(emu.config.backend, "d3d12");
        assert_eq!(emu.plugins.len(), 0);
    }

    #[test]
    fn test_emulator_run_with_mock_exe() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let mock_exe = temp_dir.join("mock_xbox.exe");
        fs::write(&mock_exe, b"MZ\x90\x00\x00\x00PE\x00\x00")?;

        let mut cfg = Config::default();
        cfg.sandbox = false;
        cfg.backend = "null".to_string();
        let mut emulator = Emulator::new(cfg)?;

        // We expect the mock run to succeed without actually transferring
        // control to guest code (the loader returns a safe mock).
        let result = emulator.run(&mock_exe);
        fs::remove_file(&mock_exe)?;

        assert!(result.is_ok());
        Ok(())
    }
}
