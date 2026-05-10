# AGENTS

## Project essentials
- Rust CLI that repacks the official ChatGPT Windows MSIX/MSIXBundle into Linux packages (Arch/DEB/RPM).
- Binary entrypoint: `src/main.rs` (`chatgpt-linux-desktop` in `Cargo.toml`).

## Build and run
- Build release binary: `cargo build --release` (output: `target/release/chatgpt-linux-desktop`).
- Run with defaults (auto-fetch MSIX and build Arch): `chatgpt-linux-desktop`.
- Provide local MSIX/MSIXBundle: `chatgpt-linux-desktop --msix /path/to/ChatGPT.msixbundle`.
- Package format flags: `--format arch|deb|rpm|both`.

## Output and cleanup behavior
- Default output root is `dist/` with `dist/cache/` (downloads) and `dist/build-tmp/` (staging).
- Temporary build dir is deleted after success unless `--no-clean` is set.

## Packaging prerequisites
- Arch builds require `libarchive/bsdtar`; DEB builds require `dpkg-dev`; RPM builds require `rpmbuild`.

## Source acquisition
- If `--msix` is omitted, the tool downloads the MSIXBundle via rg-adguard using `--store-query` and `--ring`.

## CLI quirks
- `--store-query` defaults to the official ChatGPT Store listing URL.
- `--ring` values are converted to uppercase codes expected by rg-adguard (`Retail`, `RP`, `WIF`, `WIS`).
- `--maintainer` defaults to `Local Build` unless `MAINTAINER` env var is set.

## Code quirks
- The build always deletes `dist/build-tmp` at startup if it exists (no incremental reuse).
- If the extracted `assets/` directory is missing, it is created anyway and the build proceeds with a warning.
- The Electron `resources/default_app.asar` is removed and replaced with a freshly packed `resources/app.asar`.
- Custom icons are embedded in the binary and written into assets as `TrayTemplateDark.png` and `AppList.targetsize-256.png`.
