use anyhow::{Context, Result, bail};
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub const PACKAGE_NAME: &str = "chatgpt-desktop-native";
const APP_NAME: &str = "ChatGPT Classic";
const DESCRIPTION: &str = "ChatGPT Classic desktop app repackaged from the official Windows MSIX into a native Linux Electron package";
const DESKTOP_FILE: &str = "chatgpt-desktop-native.desktop";

// ── filesystem helpers ────────────────────────────────────────────────────────

pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target_path = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target_path)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target_path)?;
            let mode = entry.metadata()?.permissions().mode();
            std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(mode))?;
        } else if entry.file_type().is_symlink() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let target = std::fs::read_link(entry.path())
                .with_context(|| format!("reading symlink {}", entry.path().display()))?;
            unix_fs::symlink(&target, &target_path)
                .with_context(|| format!("copying symlink {}", entry.path().display()))?;
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
    let license_dir = pkg_root.join(format!("usr/share/licenses/{PACKAGE_NAME}"));

    std::fs::create_dir_all(&install_root)?;
    std::fs::create_dir_all(&bin_dir)?;
    std::fs::create_dir_all(&app_dir)?;
    std::fs::create_dir_all(&icon_dir)?;
    std::fs::create_dir_all(&license_dir)?;

    // electron binaries + app.asar already packed inside
    let electron_install_dir = install_root.join("electron");
    copy_dir(electron_dir, &electron_install_dir)?;
    let generic_electron = electron_install_dir.join("electron");
    if !generic_electron.exists() {
        bail!(
            "Electron binary not found at {}; downloaded archive may be incomplete",
            generic_electron.display()
        );
    }
    std::fs::rename(&generic_electron, electron_install_dir.join(PACKAGE_NAME))
        .with_context(|| format!("renaming {}", generic_electron.display()))?;
    // official app assets (icons, sounds, etc.)
    copy_dir(assets_dir, &install_root.join("assets"))?;

    // launcher
    write_exec(
        &bin_dir.join(PACKAGE_NAME),
        &format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
export CHROME_DESKTOP={DESKTOP_FILE}
if command -v xdg-mime >/dev/null 2>&1; then
  xdg-mime default "{DESKTOP_FILE}" x-scheme-handler/chatgpt >/dev/null 2>&1 || true
  xdg-mime default "{DESKTOP_FILE}" x-scheme-handler/chatgpt-alt >/dev/null 2>&1 || true
fi
exec /opt/{PACKAGE_NAME}/electron/{PACKAGE_NAME} --no-sandbox --class={PACKAGE_NAME} "$@"
"#
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
xdg-mime default "{DESKTOP_FILE}" x-scheme-handler/chatgpt
xdg-mime default "{DESKTOP_FILE}" x-scheme-handler/chatgpt-alt
echo "Registered URL handlers:"
echo "  chatgpt -> $(xdg-mime query default x-scheme-handler/chatgpt)"
echo "  chatgpt-alt -> $(xdg-mime query default x-scheme-handler/chatgpt-alt)"
"#
        ),
    )?;

    // icon
    let icon_src = assets_dir.join("AppList.targetsize-256.png");
    if icon_src.exists() {
        std::fs::copy(&icon_src, icon_dir.join(format!("{PACKAGE_NAME}.png")))?;
    }

    // desktop entry
    write_file(
        &app_dir.join(DESKTOP_FILE),
        &format!(
            "[Desktop Entry]\n\
             Name={APP_NAME}\n\
             Comment=ChatGPT Classic Desktop\n\
             Exec={PACKAGE_NAME} %u\n\
             Icon={PACKAGE_NAME}\n\
             Type=Application\n\
             Terminal=false\n\
             Categories=Utility;\n\
             StartupWMClass={PACKAGE_NAME}\n\
             X-GNOME-WMClass={PACKAGE_NAME}\n\
             MimeType=x-scheme-handler/chatgpt;x-scheme-handler/chatgpt-alt;\n"
        ),
    )?;

    write_file(
        &license_dir.join("LICENSE"),
        "This package repackages the official ChatGPT Classic application.\nAll rights reserved by OpenAI.\n",
    )?;

    Ok(pkg_root)
}

// ── arch ──────────────────────────────────────────────────────────────────────

pub fn build_arch(
    pkg_root: &Path,
    work_dir: &Path,
    version: &str,
    maintainer: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    which::which("bsdtar").context("bsdtar not found — install libarchive")?;

    let arch_version = arch_pkgver(version);
    let arch_root = work_dir.join("archpkg");
    if arch_root.exists() {
        std::fs::remove_dir_all(&arch_root)
            .with_context(|| format!("removing {}", arch_root.display()))?;
    }
    std::fs::create_dir_all(&arch_root)?;
    copy_dir(pkg_root, &arch_root)?;

    let installed_size = installed_size(pkg_root)?;
    let build_date = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs();

    write_file(
        &arch_root.join(".PKGINFO"),
        &format!(
            "pkgname = {PACKAGE_NAME}\n\
             pkgbase = {PACKAGE_NAME}\n\
             pkgver = {arch_version}-1\n\
             pkgdesc = {DESCRIPTION}\n\
             builddate = {build_date}\n\
             packager = {maintainer}\n\
             size = {installed_size}\n\
             arch = x86_64\n\
             license = custom:proprietary\n\
             depend = gtk3\n\
             depend = nss\n\
             depend = libxss\n\
             depend = alsa-lib\n\
             depend = mesa\n\
             depend = libxshmfence\n\
             depend = at-spi2-core\n\
             depend = libdrm\n\
             depend = libxkbcommon\n\
             depend = xdg-utils\n"
        ),
    )?;

    write_file(
        &arch_root.join(".INSTALL"),
        "post_install() {\n  update-desktop-database /usr/share/applications >/dev/null 2>&1 || true\n}\n\npost_upgrade() {\n  post_install\n}\n\npost_remove() {\n  update-desktop-database /usr/share/applications >/dev/null 2>&1 || true\n}\n",
    )?;

    let out = out_dir.join(format!(
        "{PACKAGE_NAME}-{arch_version}-1-x86_64.pkg.tar.zst"
    ));
    let out_abs = std::fs::canonicalize(out_dir)
        .with_context(|| format!("resolving {}", out_dir.display()))?
        .join(
            out.file_name()
                .context("Arch package output path has no file name")?,
        );
    let status = std::process::Command::new("bsdtar")
        .arg("--zstd")
        .arg("-cf")
        .arg(&out_abs)
        .args([".PKGINFO", ".INSTALL", "opt", "usr"])
        .current_dir(&arch_root)
        .status()
        .context("running bsdtar")?;

    if !status.success() {
        anyhow::bail!("bsdtar failed with {status}");
    }
    eprintln!("  built: {}", out.display());
    Ok(out)
}

fn arch_pkgver(version: &str) -> String {
    version
        .chars()
        .map(|c| match c {
            '-' | ':' => '_',
            c => c,
        })
        .collect()
}

fn installed_size(pkg_root: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in WalkDir::new(pkg_root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            size += entry.metadata()?.len();
        }
    }
    Ok(size)
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
             Depends: libgtk-3-0, libnss3, libxss1, libasound2t64 | libasound2, libgbm1, libxshmfence1, libatk-bridge2.0-0, libdrm2, libxkbcommon0, xdg-utils\n\
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
             /usr/share/icons/hicolor/256x256/apps/{PACKAGE_NAME}.png\n\
             /usr/share/licenses/{PACKAGE_NAME}/LICENSE\n"
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
        if entry.path().extension().is_some_and(|e| e == "rpm") {
            return Ok(entry.path().to_owned());
        }
    }
    anyhow::bail!(
        "rpmbuild produced no .rpm file under {}",
        rpm_root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_desktop_entry_uses_classic_name() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "chatgpt-classic-package-{}-{suffix}",
            std::process::id(),
        ));
        let electron = root.join("electron");
        let assets = root.join("assets");
        let work = root.join("work");
        std::fs::create_dir_all(&electron).unwrap();
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(electron.join("electron"), b"electron").unwrap();

        let pkg_root = stage(&electron, &assets, &work).unwrap();
        let desktop_entry = std::fs::read_to_string(
            pkg_root.join(format!("usr/share/applications/{DESKTOP_FILE}")),
        )
        .unwrap();

        assert!(desktop_entry.contains("Name=ChatGPT Classic\n"));
        assert!(desktop_entry.contains("Comment=ChatGPT Classic Desktop\n"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
