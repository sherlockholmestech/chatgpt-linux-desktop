use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const PACKAGE_NAME: &str = "chatgpt-desktop-native";
const DESCRIPTION: &str =
    "ChatGPT desktop app repackaged from the official Windows MSIX into a native Linux Electron package";

// ── filesystem helpers ────────────────────────────────────────────────────────

pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            std::fs::copy(entry.path(), &dest)?;
            let mode = entry.metadata()?.permissions().mode();
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn write_exec(path: &Path, content: &str) -> Result<()> {
    write_file(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

// ── shared staging ────────────────────────────────────────────────────────────

/// Build the shared package root tree (everything except format-specific metadata).
/// Returns the path to the pkgroot directory.
pub fn stage(electron_dir: &Path, assets_dir: &Path, work_dir: &Path) -> Result<PathBuf> {
    let pkg_root = work_dir.join("pkgroot");
    let install_root = pkg_root.join(format!("opt/{PACKAGE_NAME}"));
    let bin_dir = pkg_root.join("usr/bin");
    let app_dir = pkg_root.join("usr/share/applications");
    let icon_dir = pkg_root.join("usr/share/icons/hicolor/256x256/apps");

    std::fs::create_dir_all(&install_root)?;
    std::fs::create_dir_all(&bin_dir)?;
    std::fs::create_dir_all(&app_dir)?;
    std::fs::create_dir_all(&icon_dir)?;

    // electron binaries + app.asar already packed inside
    copy_dir(electron_dir, &install_root.join("electron"))?;
    // official app assets (icons, sounds, etc.)
    copy_dir(assets_dir, &install_root.join("assets"))?;

    // launcher
    write_exec(
        &bin_dir.join(PACKAGE_NAME),
        &format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nexec /opt/{PACKAGE_NAME}/electron/electron --no-sandbox \"$@\"\n"
        ),
    )?;

    // URL-handler registration helper
    write_exec(
        &bin_dir.join(format!("{PACKAGE_NAME}-register")),
        &format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
if ! command -v xdg-mime >/dev/null 2>&1; then
  echo "xdg-mime not found" >&2
  exit 1
fi
xdg-mime default "{PACKAGE_NAME}.desktop" x-scheme-handler/chatgpt
xdg-mime default "{PACKAGE_NAME}.desktop" x-scheme-handler/chatgpt-alt
echo "Registered URL handlers:"
echo "  chatgpt -> $(xdg-mime query default x-scheme-handler/chatgpt)"
echo "  chatgpt-alt -> $(xdg-mime query default x-scheme-handler/chatgpt-alt)"
"#
        ),
    )?;

    // icon
    let icon_src = assets_dir.join("AppList.targetsize-256.png");
    if icon_src.exists() {
        std::fs::copy(&icon_src, &icon_dir.join(format!("{PACKAGE_NAME}.png")))?;
    }

    // desktop entry
    write_file(
        &app_dir.join(format!("{PACKAGE_NAME}.desktop")),
        &format!(
            "[Desktop Entry]\n\
             Name=ChatGPT\n\
             Comment=ChatGPT Desktop\n\
             Exec={PACKAGE_NAME} %u\n\
             Icon={PACKAGE_NAME}\n\
             Type=Application\n\
             Terminal=false\n\
             Categories=Utility;\n\
             StartupWMClass=electron\n\
             X-GNOME-WMClass=electron\n\
             MimeType=x-scheme-handler/chatgpt;x-scheme-handler/chatgpt-alt;\n"
        ),
    )?;

    Ok(pkg_root)
}

// ── deb ───────────────────────────────────────────────────────────────────────

pub fn build_deb(
    pkg_root: &Path,
    version: &str,
    maintainer: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    which::which("dpkg-deb").context("dpkg-deb not found — install dpkg-dev")?;

    let debian = pkg_root.join("DEBIAN");
    std::fs::create_dir_all(&debian)?;

    write_file(
        &debian.join("control"),
        &format!(
            "Package: {PACKAGE_NAME}\n\
             Version: {version}\n\
             Section: utils\n\
             Priority: optional\n\
             Architecture: amd64\n\
             Maintainer: {maintainer}\n\
             Depends: libgtk-3-0, libnss3, libxss1, libasound2t64 | libasound2, libgbm1, libxshmfence1, libatk-bridge2.0-0, libdrm2, libxkbcommon0\n\
             Description: {DESCRIPTION}\n"
        ),
    )?;

    write_exec(
        &debian.join("postinst"),
        "#!/usr/bin/env bash\nset -euo pipefail\nupdate-desktop-database /usr/share/applications >/dev/null 2>&1 || true\n",
    )?;

    write_exec(
        &debian.join("postrm"),
        "#!/usr/bin/env bash\nset -euo pipefail\nif [[ \"${1:-}\" == \"remove\" || \"${1:-}\" == \"purge\" ]]; then\n  update-desktop-database /usr/share/applications >/dev/null 2>&1 || true\nfi\n",
    )?;

    let out = out_dir.join(format!("{PACKAGE_NAME}_{version}_amd64.deb"));
    let status = std::process::Command::new("dpkg-deb")
        .args(["--build", "--root-owner-group"])
        .arg(pkg_root)
        .arg(&out)
        .status()
        .context("running dpkg-deb")?;

    if !status.success() {
        anyhow::bail!("dpkg-deb failed with {status}");
    }
    eprintln!("  built: {}", out.display());
    Ok(out)
}

// ── rpm ───────────────────────────────────────────────────────────────────────

pub fn build_rpm(
    pkg_root: &Path,
    work_dir: &Path,
    version: &str,
    maintainer: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    which::which("rpmbuild").context("rpmbuild not found — install rpm-build (dnf/zypper)")?;

    // RPM version strings may not contain dashes
    let rpm_version = version.replace('-', "_");

    let rpm_root = work_dir.join("rpmbuild");
    for sub in &["BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
        std::fs::create_dir_all(rpm_root.join(sub))?;
    }
    let rpm_root_abs = std::fs::canonicalize(&rpm_root)
        .with_context(|| format!("resolving {}", rpm_root.display()))?;

    let spec_path = rpm_root.join(format!("SPECS/{PACKAGE_NAME}.spec"));
    let pkg_root_abs = std::fs::canonicalize(pkg_root)
        .with_context(|| format!("resolving {}", pkg_root.display()))?;
    let pkg_root_str = pkg_root_abs.display().to_string();

    write_file(
        &spec_path,
        &format!(
            "Name:           {PACKAGE_NAME}\n\
             Version:        {rpm_version}\n\
             Release:        1\n\
             Summary:        {DESCRIPTION}\n\
             License:        Proprietary\n\
             BuildArch:      x86_64\n\
             Packager:       {maintainer}\n\
             AutoReqProv:    no\n\
             Requires:       gtk3, nss, libXScrnSaver, alsa-lib, mesa-libgbm, libxshmfence, libdrm, libxkbcommon\n\
             \n\
             %description\n\
             {DESCRIPTION}\n\
             \n\
             %install\n\
             cp -a {pkg_root_str}/. %{{buildroot}}/\n\
             \n\
             %post\n\
             update-desktop-database /usr/share/applications >/dev/null 2>&1 || true\n\
             \n\
             %postun\n\
             if [ \"$1\" = \"0\" ]; then\n\
               update-desktop-database /usr/share/applications >/dev/null 2>&1 || true\n\
             fi\n\
             \n\
             %files\n\
             %defattr(-,root,root,-)\n\
             /opt/{PACKAGE_NAME}/\n\
             /usr/bin/{PACKAGE_NAME}\n\
             /usr/bin/{PACKAGE_NAME}-register\n\
             /usr/share/applications/{PACKAGE_NAME}.desktop\n\
             /usr/share/icons/hicolor/256x256/apps/{PACKAGE_NAME}.png\n"
        ),
    )?;

    let status = std::process::Command::new("rpmbuild")
        .arg("-bb")
        .arg("--define")
        .arg(format!("_topdir {}", rpm_root_abs.display()))
        .arg(&spec_path)
        .status()
        .context("running rpmbuild")?;

    if !status.success() {
        anyhow::bail!("rpmbuild failed with {status}");
    }

    // find the produced .rpm and copy it to out_dir
    let built = find_rpm(&rpm_root_abs)?;
    let out = out_dir.join(format!("{PACKAGE_NAME}-{rpm_version}-1.x86_64.rpm"));
    std::fs::copy(&built, &out)?;
    eprintln!("  built: {}", out.display());
    Ok(out)
}

fn find_rpm(rpm_root: &Path) -> Result<PathBuf> {
    for entry in WalkDir::new(rpm_root.join("RPMS")) {
        let entry = entry?;
        if entry.path().extension().map_or(false, |e| e == "rpm") {
            return Ok(entry.path().to_owned());
        }
    }
    anyhow::bail!(
        "rpmbuild produced no .rpm file under {}",
        rpm_root.display()
    )
}
