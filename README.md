# chatgpt-linux-desktop

Repack the official ChatGPT Windows MSIX into a native Linux (Fedora/openSUSE) Electron package.

This tool downloads the official ChatGPT Windows MSIX/MSIXBundle, extracts the app and its assets, patches the JavaScript bundle to work on Linux, and bundles it into a native Linux package (DEB or RPM).

---

## Prerequisites

- **Rust** (latest stable)
- **dpkg-dev** (for DEB builds) or **rpmbuild** (for RPM builds)
  ```bash
  # Fedora
  sudo dnf install rpm-build
  # Ubuntu/Debian
  sudo apt install dpkg-dev
  # openSUSE
  sudo zypper install rpmdevtools
  ```

---

## Install

```bash
cargo build --release
```

The binary will be at `target/release/chatgpt-linux-desktop`.

---

## Usage

### Basic — auto-fetch MSIX and build RPM

```bash
cargo run --release
```

This will:
1. Download the latest ChatGPT MSIX from the Microsoft Store via rg-adguard
2. Extract the app and assets
3. Patch the JavaScript for Linux compatibility
4. Download Electron and bundle everything into an RPM

Output goes into `dist/` by default.

### Provide your own MSIX

```bash
cargo run --release -- --msix /path/to/ChatGPT.msixbundle
```

### Build DEB instead

```bash
cargo run --release -- --format deb
```

### Build both DEB and RPM

```bash
cargo run --release -- --format both
```

### Specify output directory

```bash
cargo run --release -- --out-dir ./output
```

### Keep build artifacts

By default the tool cleans up its temporary build directory after a successful run. Keep it with:

```bash
cargo run --release -- --no-clean
```

### Custom maintainer

```bash
cargo run --release -- --maintainer "Your Name <you@example.com>"
```

Or set the `MAINTAINER` environment variable:

```bash
export MAINTAINER="Your Name <you@example.com>"
cargo run --release
```

---

## How It Works

1. **Acquire MSIX** — Downloads the official ChatGPT MSIXBundle from the Microsoft Store (via rg-adguard) or uses a local file you provide.

2. **Extract payload** — Unwraps the MSIX/MSIXBundle and extracts `app.asar` and the `assets/` directory (containing icons, sounds, etc.).

3. **Patch app** — Modifies the bundled JavaScript to fix Linux-incompatible behaviors:
   - Disables macOS-only voice read-aloud functionality on Linux
   - Removes macOS-only window vibrancy effects
   - Fixes Linux hostname detection
   - **Fixes tray icon path resolution** — the original app uses `app.isPackaged` to locate assets, which resolves incorrectly on Linux and points inside the asar instead of the real `assets/` directory. The patch makes Linux use `process.resourcesPath` to reliably find the tray icons.

4. **Fetch Electron** — Downloads a matching Electron binary from GitHub releases.

5. **Bundle** — Packs the patched app into Electron and stages all assets.

6. **Apply custom icons** — Writes the bundled dark-themed ChatGPT icons into the staged assets:
   - `TrayTemplateDark.png` (32×32, used for the tray)
   - `AppList.targetsize-256.png` (256×256, used for the desktop/menus icon)

7. **Build package** — Produces a DEB or RPM installer.

---

## Command-Line Options

| Flag                        | Default                                | Description                                              |
|-----------------------------|----------------------------------------|----------------------------------------------------------|
| `--msix PATH`               | auto-fetch via rg-adguard              | Path to a local MSIX/MSIXBundle                          |
| `--store-query QUERY`       | Microsoft Store URL                    | Query passed to rg-adguard for auto-fetch                |
| `--ring`                    | `retail`                               | Update ring: `retail`, `rp`, `wif`, `wis`               |
| `--version VERSION`         | detected from MSIX                     | Override package version string                          |
| `--out-dir DIR`             | `dist`                                 | Output directory for built packages                       |
| `--format FORMAT`           | `rpm`                                  | Package format: `deb`, `rpm`, `both`                     |
| `--electron-version VERSION`| `41.2.2`                               | Electron version to bundle from GitHub releases         |
| `--maintainer STRING`       | `Local Build`                          | Maintainer string for the package                        |
| `--no-clean`                | false                                  | Keep the temporary build directory after completion      |

---

## Custom Icons

The tray icon and the desktop/app icon are replaced during the build process with dark-themed ChatGPT icons extracted from the official macOS `.icns` resource.

The icon replacement is handled by `apply_custom_icons()` in `src/main.rs`, which:
1. Writes `TrayTemplateDark.png` (32×32 PNG) from the embedded dark-themed macOS icon
2. Overwrites `AppList.targetsize-256.png` with the 256×256 PNG (used for the desktop/menus icon)

The JavaScript patch makes Linux use `process.resourcesPath` to locate the real `assets/` directory (instead of the incorrect `app.isPackaged` path that points inside the asar). On Linux the tray icon always loads `TrayTemplateDark.png`.

---

## Dependencies

Runtime dependencies for the built package (declared in `DEBIAN/control` and the RPM spec):

- libgtk-3-0
- libnss3
- libxss1
- libasound2t64 (or libasound2)
- libgbm1
- libxshmfence1
- libatk-bridge2.0-0
- libdrm2
- libxkbcommon0

These are typically already installed on a standard Linux desktop environment.
