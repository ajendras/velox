# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-25

### Added

- Binary Loader — Maps and launches original x86-64 executables plus DLLs inside a controlled process
- Shader Translator — Converts console GPU bytecode to DXIL ahead-of-time or on-demand with disk cache
- Pluggable Backends — Traits let users swap GPU (D3D12 default), audio, and input providers at runtime
- Secure Sandbox — Enforces memory isolation, API whitelisting and crash isolation by default
- HTTP Monitoring — Exposes live game state, FPS, memory stats and accepts remote commands via Axum
- Mod Plugins — Dynamic library plugins hook into frame, input and shader events without patching binaries
- Asset Streaming — Parallel archive reader with on-the-fly BCPack/Zlib decode and texture detiling
- Comprehensive Tests — Unit tests for translation correctness plus integration tests that boot mock titles
