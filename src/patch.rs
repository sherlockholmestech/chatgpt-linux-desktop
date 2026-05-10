use anyhow::{Context, Result, bail};
use std::path::Path;

// Each tuple is (exact string to find, replacement). Order matters.
const PATCHES: &[(&str, &str)] = &[
    (
        "const _ua = process.platform === \"darwin\", Mua = process.platform === \"win32\";",
        "const _ua = process.platform === \"darwin\", Mua = process.platform === \"win32\", oqa_linux = process.platform === \"linux\";",
    ),
    (
        "if (_ua)\n    return u();",
        "if (_ua || oqa_linux)\n    return u();",
    ),
    (
        "  getBuildOsIdentifier() {\n    return `Mac OS X ${hu.release()}`;\n  }",
        "  getBuildOsIdentifier() {\n    return process.platform === \"linux\" ? `Linux ${hu.release()}` : `Mac OS X ${hu.release()}`;\n  }",
    ),
    (
        "  applyMainWindowStyle(u) {\n    u.setVibrancy(\"sidebar\");\n  }",
        "  applyMainWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"sidebar\");\n  }",
    ),
    (
        "  applyCompanionWindowStyle(u) {\n    u.setVibrancy(\"hud\");\n  }",
        "  applyCompanionWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"hud\");\n  }",
    ),
    (
        "  createTray() {\n    const u = jnr.createFromPath(Tg.join(oor(), \"TrayTemplate.png\"));\n    return this.tray = new znr(u), this.tray;\n  }",
        "  createTray() {\n    const u = process.platform === \"linux\" ? $Ye() : jnr.createFromPath(Tg.join(oor(), \"TrayTemplate.png\"));\n    return this.tray = new znr(u), this.tray;\n  }",
    ),
    (
        "function jpa() {\n  try {",
        "function jpa() {\n  if (process.platform === \"linux\")\n    return hu.hostname();\n  try {",
    ),
    (
        "function $Ye() {\n  const e = pca() === \"dark\" ? \"TrayDark.ico\" : \"TrayLight.ico\";\n  return jnr.createFromPath(Tg.join(oor(), e));\n}",
        "function $Ye() {\n  if (process.platform === \"linux\") {\n    const assetDir = Tg.resolve(process.resourcesPath, \"..\", \"..\", \"assets\");\n    return jnr.createFromPath(Tg.join(assetDir, \"TrayTemplateDark.png\"));\n  }\n  const e = pca() === \"dark\" ? \"TrayDark.ico\" : \"TrayLight.ico\";\n  return jnr.createFromPath(Tg.join(oor(), e));\n}",
    ),
];

/// Find the vite-built main JS in `app_dir/.vite/build/main-*.js`.
fn find_main_js(app_dir: &Path) -> Result<std::path::PathBuf> {
    let build_dir = app_dir.join(".vite/build");
    for entry in
        std::fs::read_dir(&build_dir).with_context(|| format!("reading {}", build_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("main-") && name.ends_with(".js") {
            return Ok(entry.path());
        }
    }
    bail!("no main-*.js found in {}", build_dir.display())
}

pub fn apply(app_dir: &Path) -> Result<()> {
    let js_path = find_main_js(app_dir)?;
    eprintln!("  patching {}", js_path.display());

    let mut src = std::fs::read_to_string(&js_path)
        .with_context(|| format!("reading {}", js_path.display()))?;

    for (from, to) in PATCHES {
        if !src.contains(from) {
            let context = diagnostic_context(&src, from);
            bail!(
                "patch target not found (app may have updated):\n  expected: {}\n  nearest context: {}",
                preview(from, 120),
                context.unwrap_or_else(|| "no related context found".to_string())
            );
        }
        src = src.replacen(from, to, 1);
    }

    std::fs::write(&js_path, src)?;
    eprintln!("  {} patches applied", PATCHES.len());
    Ok(())
}

fn preview(value: &str, max_chars: usize) -> String {
    let value = value.replace('\n', "\\n");
    if value.len() <= max_chars {
        value
    } else {
        format!("{}...", &value[..max_chars])
    }
}

fn diagnostic_context(src: &str, expected: &str) -> Option<String> {
    let needle = expected
        .lines()
        .map(str::trim)
        .find(|line| line.len() >= 12)
        .or_else(|| expected.split_whitespace().find(|part| part.len() >= 12))?;
    let pos = src.find(needle)?;
    let start = src[..pos]
        .char_indices()
        .rev()
        .nth(100)
        .map_or(0, |(idx, _)| idx);
    let end = src[pos..]
        .char_indices()
        .nth(200)
        .map_or(src.len(), |(idx, _)| pos + idx);
    Some(preview(src[start..end].trim(), 240))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{name}-{suffix}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn apply_replaces_each_patch_target() {
        let root = temp_dir("patch-apply");
        let build = root.join(".vite/build");
        std::fs::create_dir_all(&build).unwrap();
        let js_path = build.join("main-test.js");
        let src = PATCHES
            .iter()
            .map(|(from, _)| *from)
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(&js_path, src).unwrap();

        apply(&root).unwrap();

        let patched = std::fs::read_to_string(&js_path).unwrap();
        for (from, to) in PATCHES {
            assert!(!patched.contains(from));
            assert!(patched.contains(to));
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_reports_context_for_missing_patch_target() {
        let root = temp_dir("patch-missing");
        let build = root.join(".vite/build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(
            build.join("main-test.js"),
            "function $Ye() {\n  changed();\n}",
        )
        .unwrap();

        let err = apply(&root).unwrap_err().to_string();
        assert!(err.contains("patch target not found"));
        assert!(err.contains("nearest context"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
