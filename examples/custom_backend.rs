//! # Velox Custom Backend Example
//!
//! This example demonstrates how to implement a custom `GpuBackend` (and
//! supporting traits) for Velox. The `LoggingGpuBackend` logs every draw,
//! resource creation, and upload operation to stdout while maintaining
//! internal counters. It serves as a template for researchers, modders,
//! or developers building alternative renderers (software rasterizer,
//! Vulkan, headless test backend, etc.).
//!
//! The example also shows a minimal custom `InputBackend` and a `Plugin`
//! that reacts to frame events, illustrating how all pluggable components
//! fit together.
//!
//! Run with:
//!     cargo run --example custom_backend

use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicU32, Ordering};
use velox::backends::{GpuBackend, InputBackend, InputState, NullAudioBackend};
use velox::{Backend, DrawCommand};
use velox::emulator::GameState;
use velox::plugin::Plugin;

/// A production-quality logging GPU backend for Velox.
///
/// This backend implements both [`Backend`] and [`GpuBackend`]. All
/// operations are logged with clear, structured output. Counters are
/// maintained using atomics so the backend remains `Send + Sync`.
#[derive(Debug)]
pub struct LoggingGpuBackend {
    draw_count: AtomicU32,
    upload_count: AtomicU32,
    buffer_count: AtomicU32,
}

impl LoggingGpuBackend {
    /// Creates a new logging backend with all counters reset to zero.
    pub fn new() -> Self {
        Self {
            draw_count: AtomicU32::new(0),
            upload_count: AtomicU32::new(0),
            buffer_count: AtomicU32::new(0),
        }
    }

    /// Returns the total number of draw calls processed.
    pub fn draw_count(&self) -> u32 {
        self.draw_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of textures uploaded.
    pub fn upload_count(&self) -> u32 {
        self.upload_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of GPU buffers allocated.
    pub fn buffer_count(&self) -> u32 {
        self.buffer_count.load(Ordering::Relaxed)
    }
}

impl Default for LoggingGpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for LoggingGpuBackend {
    fn draw(&self, cmd: DrawCommand) {
        let count = self.draw_count.fetch_add(1, Ordering::Relaxed) + 1;
        println!(
            "[LOGGING_GPU] draw #{}: topology={}, vertex_count={}, instance_count={}",
            count, cmd.topology, cmd.vertex_count, cmd.instance_count
        );
    }

    fn present(&self) {
        println!("[LOGGING_GPU] present() - frame submitted to output");
    }
}

impl GpuBackend for LoggingGpuBackend {
    fn create_buffer(&self, size: usize) -> Result<u64> {
        if size == 0 {
            return Err(anyhow!(
                "Cannot create zero-sized GPU buffer (LoggingGpuBackend validation)"
            ));
        }
        let id = self.buffer_count.fetch_add(1, Ordering::Relaxed) + 1;
        let va = 0xDEAD_BEEF_0000_0000u64 + (id as u64) * 0x10000;
        println!(
            "[LOGGING_GPU] created buffer #{}: {} bytes, VA=0x{:016X}",
            id, size, va
        );
        Ok(va)
    }

    fn upload_texture(&self, data: &[u8], width: u32, height: u32) -> Result<u64> {
        if data.is_empty() {
            return Err(anyhow!(
                "Cannot upload empty texture data (LoggingGpuBackend validation)"
            ));
        }
        if width == 0 || height == 0 {
            return Err(anyhow!(
                "Invalid texture dimensions {}x{}: both must be greater than zero",
                width,
                height
            ));
        }
        let id = self.upload_count.fetch_add(1, Ordering::Relaxed) + 1;
        let handle = 0xC0FFEE_0000_0000u64 + id as u64;
        println!(
            "[LOGGING_GPU] uploaded texture #{}: {}x{} ({} bytes), handle=0x{:016X}",
            id, width, height, data.len(), handle
        );
        Ok(handle)
    }
}

/// A deterministic input backend for reproducible testing and demos.
///
/// Returns a cycling input pattern that exercises every field in
/// [`InputState`].
#[derive(Debug, Default)]
pub struct LoggingInputBackend {
    frame: AtomicU32,
}

impl InputBackend for LoggingInputBackend {
    fn poll_state(&self) -> InputState {
        let f = self.frame.fetch_add(1, Ordering::Relaxed);
        InputState {
            a_pressed: (f % 30) < 8,
            b_pressed: (f % 45) < 5,
            x_pressed: (f % 60) < 12,
            y_pressed: (f % 75) < 6,
            left_trigger: if (f % 50) < 25 { 0.75 } else { 0.0 },
            right_trigger: if (f % 80) < 15 { 1.0 } else { 0.0 },
            left_stick: (0.6, -0.4),
            right_stick: (-0.2, 0.8),
            dpad_up: (f % 20) < 5,
        }
    }
}

/// Example mod plugin that reacts to game state. Demonstrates the
/// plugin hook used by `velox::plugin::load_plugins`.
struct ExampleModPlugin {
    name: String,
    activation_count: u32,
}

impl ExampleModPlugin {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            activation_count: 0,
        }
    }
}

impl Plugin for ExampleModPlugin {
    fn on_frame(&mut self, state: &GameState) {
        self.activation_count += 1;
        if state.frame_count % 25 == 0 {
            println!(
                "[MOD:{}] frame={} fps={:.1} mem={}MB gpu={:.1}% (activations={})",
                self.name,
                state.frame_count,
                state.fps,
                state.memory_mb,
                state.gpu_usage,
                self.activation_count
            );
        }
    }
}

fn main() -> Result<()> {
    println!("Velox Custom Backend Example");
    println!("============================\n");

    let backend = LoggingGpuBackend::new();

    println!("1. Exercising Backend trait:");
    for i in 0..6 {
        backend.draw(DrawCommand {
            topology: 4,
            vertex_count: 512 + i * 256,
            instance_count: if i % 3 == 0 { 2 } else { 1 },
        });
        if i % 2 == 0 {
            backend.present();
        }
    }

    println!("\n2. Exercising GpuBackend resource methods:");
    let _buf_va = backend.create_buffer(256 * 1024)?;
    let _tex_handle = backend.upload_texture(&vec![0xFFu8; 128 * 128 * 4], 128, 128)?;

    // Demonstrate error paths with helpful messages
    println!("\n3. Input validation (error paths):");
    match backend.create_buffer(0) {
        Ok(_) => println!("   Unexpected success"),
        Err(e) => println!("   Correctly rejected: {}", e),
    }
    match backend.upload_texture(&[], 64, 64) {
        Ok(_) => println!("   Unexpected success"),
        Err(e) => println!("   Correctly rejected: {}", e),
    }

    println!("\n4. Statistics from logging backend:");
    println!("   Draws: {}", backend.draw_count());
    println!("   Textures uploaded: {}", backend.upload_count());
    println!("   Buffers allocated: {}", backend.buffer_count());

    println!("\n5. Complementary components:");
    let input_backend = LoggingInputBackend::default();
    let input = input_backend.poll_state();
    println!(
        "   Input sample - A:{}, LT:{:.2}, LS:{:?}, DpadUp:{}",
        input.a_pressed, input.left_trigger, input.left_stick, input.dpad_up
    );

    let audio = NullAudioBackend;
    audio.set_master_volume(0.9);
    audio.play_sound(vec![0u8; 8192], true);

    let mut mod_plugin = ExampleModPlugin::new("debug_overlay");
    let mock_state = GameState {
        fps: 142.6,
        frame_time_ms: 7.01,
        memory_mb: 2148,
        gpu_usage: 71.0,
        sandbox_violations: 0,
        frame_count: 250,
    };
    mod_plugin.on_frame(&mock_state);

    println!("\nExample completed successfully.");
    println!("This LoggingGpuBackend can be boxed and supplied to a custom");
    println!("Emulator or used as the foundation for a new renderer backend.");
    println!("\nFor production use, replace the println! calls with real");
    println!("graphics API submission while preserving the same trait surface.");

    Ok(())
}
