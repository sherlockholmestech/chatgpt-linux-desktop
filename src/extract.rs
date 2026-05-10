use anyhow::{Context, Result, bail};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

fn sanitize(name: &str) -> Result<PathBuf> {
    let path = PathBuf::from(name);
    if path.components().all(|c| matches!(c, Component::Normal(_))) {
        Ok(path)
    } else {
        bail!("invalid zip entry path: {name}");
    }
}

/// Extract a ZIP/MSIX/MSIX file to `dest`, preserving Unix permissions.
pub fn unzip(zip_path: &Path, dest: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let file =
        std::fs::File::open(zip_path).with_context(|| format!("opening {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let out_path = dest.join(sanitize(entry.name())?);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
            if let Some(mode) = entry.unix_mode() {
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

fn find_x64_inner(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<String> {
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let lower = entry.name().to_lowercase();
        if lower.ends_with("_x64.msix") || lower.ends_with("_x64.appx") {
            return Ok(entry.name().to_string());
        }
    }
    bail!("no x64 .msix/.appx found inside bundle")
}

fn read_bundle_version(zip_path: &Path) -> Result<String> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entry = archive.by_name("AppxMetadata/AppxBundleManifest.xml")?;
    let mut xml = String::new();
    entry.read_to_string(&mut xml)?;
    version_from_xml(&xml)
}

pub fn version_from_xml(xml: &str) -> Result<String> {
    let after = xml
        .find("<Identity")
        .and_then(|i| xml.get(i..))
        .context("no <Identity> in manifest")?;
    let after = after
        .find("Version=\"")
        .and_then(|i| after.get(i + 9..))
        .context("no Version= attribute")?;
    let end = after.find('"').context("unclosed Version attribute")?;
    Ok(after[..end].to_string())
}

/// Unpack an MSIXBundle: extracts the x64 inner MSIX into `work_dir/payload/`.
/// Returns `(payload_dir, version)`.
pub fn extract_msixbundle(bundle: &Path, work_dir: &Path) -> Result<(PathBuf, String)> {
    let version = read_bundle_version(bundle).unwrap_or_else(|err| {
        eprintln!("  warning: could not read bundle version ({err}); using 1.0.0");
        "1.0.0".to_string()
    });
    eprintln!("  version: {version}");

    let inner_name = {
        let file = std::fs::File::open(bundle)?;
        let mut archive = zip::ZipArchive::new(file)?;
        find_x64_inner(&mut archive)?
    };

    let inner_path = work_dir.join("inner_x64.msix");
    {
        let file = std::fs::File::open(bundle)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut entry = archive.by_name(&inner_name)?;
        let mut out = std::fs::File::create(&inner_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    let payload_dir = work_dir.join("payload");
    std::fs::create_dir_all(&payload_dir)?;
    unzip(&inner_path, &payload_dir)?;

    Ok((payload_dir, version))
}

// ── single MSIX/APPX ─────────────────────────────────────────────────────────

/// Unpack a single MSIX/APPX directly into `work_dir/payload/`.
pub fn extract_msix(msix: &Path, work_dir: &Path) -> Result<(PathBuf, String)> {
    let version = {
        let file = std::fs::File::open(msix)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut entry = archive.by_name("AppxManifest.xml")?;
        let mut xml = String::new();
        entry.read_to_string(&mut xml)?;
        version_from_xml(&xml).unwrap_or_else(|err| {
            eprintln!("  warning: could not read MSIX version ({err}); using 1.0.0");
            "1.0.0".to_string()
        })
    };
    eprintln!("  version: {version}");

    let payload_dir = work_dir.join("payload");
    std::fs::create_dir_all(&payload_dir)?;
    unzip(msix, &payload_dir)?;

    Ok((payload_dir, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_from_xml_reads_identity_version() {
        let xml = r#"<Package><Identity Name="ChatGPT" Version="1.2.3.4"/></Package>"#;
        assert_eq!(version_from_xml(xml).unwrap(), "1.2.3.4");
    }

    #[test]
    fn version_from_xml_rejects_missing_identity() {
        let err = version_from_xml(r#"<Package/>"#).unwrap_err().to_string();
        assert!(err.contains("no <Identity>"));
    }

    #[test]
    fn version_from_xml_rejects_missing_version() {
        let err = version_from_xml(r#"<Identity Name="ChatGPT"/>"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no Version"));
    }

    #[test]
    fn version_from_xml_rejects_unclosed_version() {
        let err = version_from_xml(r#"<Identity Version="1.2.3"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unclosed"));
    }

    #[test]
    fn sanitize_accepts_plain_relative_paths() {
        assert_eq!(
            sanitize("app/resources/app.asar").unwrap(),
            PathBuf::from("app/resources/app.asar")
        );
    }

    #[test]
    fn sanitize_rejects_traversal_and_absolute_paths() {
        assert!(sanitize("../escape").is_err());
        assert!(sanitize("/absolute").is_err());
    }
}
