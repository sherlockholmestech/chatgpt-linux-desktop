mod asar;
mod cli;
mod extract;
mod fetch;
mod package;
mod patch;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::Args;
use std::path::{Path, PathBuf};

const ICON_256_PNG: &[u8] = include_bytes!(
    "icon/icnsFile_4bb691b5e9d5669d3512f34f842f074c_ChatGPT__Dark___macOS_26.2___256x256x32.png"
);
const ICON_32_PNG: &[u8] = include_bytes!(
    "icon/icnsFile_4bb691b5e9d5669d3512f34f842f074c_ChatGPT__Dark___macOS_26.2___32x32x32.png"
);

fn section(title: &str) {
    eprintln!("\n\x1b[1;36m== {title} ==\x1b[0m");
}

fn main() -> Result<()> {
    let args = Args::parse();

    let build_dir = Path::new("build-tmp");
    std::fs::create_dir_all(build_dir)?;
    std::fs::create_dir_all(&args.out_dir)?;
    let cache = fetch::cache_dir()?;

    // 1. Acquire source and extract to a common (asar_path, assets_dir, version)
    let msix_path = match &args.msix {
        Some(path) => path.clone(),
        None => {
            section("Download Payload");
            fetch::download_msix_from_rg_adguard(&cache, &args.store_query, args.ring.as_str())?
        }
    };
    let (app_asar, assets_dir, detected_version) = acquire_msix(&msix_path, build_dir)?;

    let version = args.version.clone().unwrap_or(detected_version);

    if !app_asar.exists() {
        bail!("app.asar not found at {}", app_asar.display());
    }

    // 2. Extract ASAR
    section("Extract ASAR");
    let app_src = build_dir.join("app");
    asar::extract(&app_asar, &app_src)?;
    eprintln!("  extracted to {}", app_src.display());

    // 3. Patch app
    section("Patch App");
    patch::apply(&app_src)?;

    // 4. Fetch Electron
    section("Fetch Electron");
    let electron_src = fetch::fetch_electron(&args.electron_version, &cache)?;
    let staged_electron = build_dir.join("staged-electron");
    package::copy_dir(&electron_src, &staged_electron)?;
    eprintln!("  staged electron v{}", args.electron_version);

    // 5. Pack patched app into electron resources
    let _ = std::fs::remove_file(staged_electron.join("resources/default_app.asar"));
    let new_asar = staged_electron.join("resources/app.asar");
    asar::pack(&app_src, &new_asar).context("packing app.asar")?;
    eprintln!("  packed app.asar");

    // 6. Stage assets — apply custom icons
    let staged_assets = build_dir.join("staged-assets");
    if assets_dir.exists() {
        package::copy_dir(&assets_dir, &staged_assets)?;
    } else {
        eprintln!("  warning: no assets/ directory found — icon will be missing");
        std::fs::create_dir_all(&staged_assets)?;
    }
    apply_custom_icons(&staged_assets)?;

    // 7. Stage shared package root
    section("Stage Package Root");
    let pkg_root = package::stage(&staged_electron, &staged_assets, build_dir)?;
    eprintln!("  pkgroot: {}", pkg_root.display());

    // 8. Build requested format(s)
    if args.format.builds_deb() {
        section("Build DEB");
        package::build_deb(&pkg_root, &version, &args.maintainer, &args.out_dir)?;
    }
    if args.format.builds_rpm() {
        section("Build RPM");
        package::build_rpm(
            &pkg_root,
            build_dir,
            &version,
            &args.maintainer,
            &args.out_dir,
        )?;
    }

    // 9. Cleanup
    if !args.no_clean {
        std::fs::remove_dir_all(build_dir)
            .with_context(|| format!("cleaning {}", build_dir.display()))?;
    }

    eprintln!(
        "\n\x1b[1;32mDone.\x1b[0m  Output in {}",
        args.out_dir.display()
    );
    Ok(())
}

// ── source-specific acquisition ───────────────────────────────────────────────

fn acquire_msix(path: &PathBuf, build_dir: &Path) -> Result<(PathBuf, PathBuf, String)> {
    section("Validate Payload");
    if !path.exists() {
        bail!(
            "file not found: {}\n\n\
             Provide --msix PATH or omit --msix to auto-fetch via rg-adguard.",
            path.display()
        );
    }
    eprintln!("  payload: {}", path.display());

    section("Extract Payload");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (payload_dir, version) = match ext.as_str() {
        "msixbundle" | "appxbundle" => extract::extract_msixbundle(path, build_dir)?,
        "msix" | "appx" => extract::extract_msix(path, build_dir)?,
        other => bail!("unsupported file extension: .{other}"),
    };

    let app_asar = payload_dir.join("app/resources/app.asar");
    let assets_dir = payload_dir.join("assets");
    Ok((app_asar, assets_dir, version))
}

fn apply_custom_icons(assets: &Path) -> Result<()> {
    let icon_cmd = if which::which("magick").is_ok() {
        "magick"
    } else {
        which::which("convert").context(
            "ImageMagick not found — install it (e.g. apt install imagemagick)",
        )?;
        "convert"
    };

    let tmp = assets.join("_tray_tmp.png");
    std::fs::write(&tmp, ICON_32_PNG)
        .with_context(|| format!("writing {}", tmp.display()))?;

    for name in ["TrayDark.ico", "TrayLight.ico"] {
        let dest = assets.join(name);
        let status = std::process::Command::new(icon_cmd)
            .arg(&tmp)
            .arg(&dest)
            .status()
            .with_context(|| format!("running {icon_cmd} to create {name}"))?;
        if !status.success() {
            anyhow::bail!("{icon_cmd} failed while creating {name}");
        }
    }
    let _ = std::fs::remove_file(&tmp);

    std::fs::write(assets.join("AppList.targetsize-256.png"), ICON_256_PNG)
        .with_context(|| "writing custom AppList icon")?;
    Ok(())
}
