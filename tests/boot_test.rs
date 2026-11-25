use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use velox::config::Config;
use velox::emulator::Emulator;

#[test]
fn test_boot_mock_title() -> Result<()> {
    let test_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("boot_test");
    let _ = fs::remove_dir_all(&test_root);
    fs::create_dir_all(&test_root).context("Failed to create test root directory")?;

    let mock_exe = test_root.join("mock_xbox_title.exe");

    // Minimal valid DOS + PE header that passes loader validation
    let mut header = vec![0u8; 512];
    header[0] = b'M';
    header[1] = b'Z';
    header[0x3c] = 0x80; // e_lfanew
    let pe = 0x80;
    header[pe] = b'P';
    header[pe + 1] = b'E';
    header[pe + 4] = 0x64; // AMD64 machine type
    header[pe + 0x28] = 0x20; // example entry point RVA

    fs::write(&mock_exe, &header).context("Failed to write mock executable")?;

    let mut cfg = Config::default();
    cfg.game_dir = test_root.clone();
    cfg.cache_dir = test_root.join("velox_cache");
    cfg.sandbox = false; // avoid page-guard interactions in CI
    cfg.backend = "null".to_string();
    cfg.plugins = vec![];

    let mut emulator = Emulator::new(cfg).context("Emulator::new failed")?;

    // This exercises the full chain:
    // * loader::load_and_patch (PE header check, VirtualAlloc, patch_compatibility)
    // * sandbox::enter (skipped because sandbox=false)
    // * http_api::router + Axum server spawn on 8080
    // * translator::translate + Cache on first frame
    // * backends::NullBackend draw/present calls
    // * plugin::load_plugins + LoggerPlugin::on_frame
    // * GameState metric updates
    let run_result = emulator.run(&mock_exe);

    // Cleanup before asserting so we never leave files behind
    let _ = fs::remove_dir_all(&test_root);

    run_result.context("Emulator run failed")?;

    // Explicit verification that the shader cache was created by the first-frame path
    let shader_dir = test_root.join("velox_cache").join("shaders");
    assert!(
        shader_dir.exists(),
        "Shader cache directory was not created by emulator"
    );

    Ok(())
}

#[test]
fn test_boot_with_sandbox_enabled() -> Result<()> {
    let test_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("boot_sandbox_test");
    let _ = fs::remove_dir_all(&test_root);
    fs::create_dir_all(&test_root)?;

    let mock_exe = test_root.join("sandboxed.exe");
    fs::write(&mock_exe, b"MZPE\x00\x00\x64")?;

    let mut cfg = Config::default();
    cfg.game_dir = test_root.clone();
    cfg.cache_dir = test_root.join("cache");
    cfg.sandbox = true;
    cfg.backend = "null".to_string();

    let mut emulator = Emulator::new(cfg)?;

    let result = emulator.run(&mock_exe);

    let _ = fs::remove_dir_all(&test_root);

    result.context("Sandbox-enabled boot failed")?;

    Ok(())
}

#[test]
fn test_translator_cache_during_boot() -> Result<()> {
    use velox::translator::{self, Cache};

    let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("translator_boot_test");
    let _ = fs::remove_dir_all(&cache_root);
    fs::create_dir_all(&cache_root)?;

    let mut cache = Cache::new(&cache_root)?;
    let bytecode = vec![0xAAu8; 32];

    let dxil = translator::translate(&bytecode, &mut cache)
        .context("Translation inside integration test failed")?;

    assert!(dxil.starts_with(b"DXIL"), "Translated blob must start with DXIL magic");
    assert!(dxil.ends_with(b"VELOX-TRANSLATED"), "Must contain translation marker");

    // Verify on-disk artifact
    let entries = fs::read_dir(&cache_root)?
        .filter_map(|e| e.ok())
        .count();
    assert!(entries > 0, "Cache must have written at least one .dxil file");

    let _ = cache.clear();
    let _ = fs::remove_dir_all(&cache_root);

    Ok(())
}
