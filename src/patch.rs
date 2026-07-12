use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

struct Patch {
    from: &'static str,
    to: &'static str,
}

// Each patch is an exact string to find and its replacement. Order matters.
const MAIN_PATCHES: &[Patch] = &[
    Patch {
        from: "const _ua = process.platform === \"darwin\", Mua = process.platform === \"win32\";",
        to: "const _ua = process.platform === \"darwin\", Mua = process.platform === \"win32\", oqa_linux = process.platform === \"linux\";",
    },
    Patch {
        from: "if (_ua)\n    return u();",
        to: "if (_ua || oqa_linux)\n    return u();",
    },
    Patch {
        from: "  getBuildOsIdentifier() {\n    return `Mac OS X ${hu.release()}`;\n  }",
        to: "  getBuildOsIdentifier() {\n    return process.platform === \"linux\" ? `Linux ${hu.release()}` : `Mac OS X ${hu.release()}`;\n  }",
    },
    Patch {
        from: "  applyMainWindowStyle(u) {\n    u.setVibrancy(\"sidebar\");\n  }",
        to: "  applyMainWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"sidebar\");\n  }",
    },
    Patch {
        from: "  applyCompanionWindowStyle(u) {\n    u.setVibrancy(\"hud\");\n  }",
        to: "  applyCompanionWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"hud\");\n  }",
    },
    Patch {
        // Match only the stable beginning of the macOS tray implementation.
        // The path-module identifier is minified and changes between releases.
        from: "  createTray() {\n    const u = jnr.createFromPath(",
        to: "  createTray() {\n    const u = process.platform === \"linux\" ? $Ye() : jnr.createFromPath(",
    },
    Patch {
        from: "function jpa() {\n  try {",
        to: "function jpa() {\n  if (process.platform === \"linux\")\n    return hu.hostname();\n  try {",
    },
    Patch {
        // Insert the Linux branch without referring to the release-specific,
        // minified path-module identifier used by the Windows implementation.
        from: "function $Ye() {\n",
        to: "function $Ye() {\n  if (process.platform === \"linux\") {\n    const assetPath = `${process.resourcesPath}/../../assets/TrayTemplateDark.png`;\n    return jnr.createFromPath(assetPath);\n  }\n",
    },
];

const APP_BOOTSTRAP_PATCHES: &[Patch] = &[Patch {
    from: "Gu(), await Promise.allSettled(t), we().getState().appReady(), Pe && console.log(e);",
    to: "Gu(), await Promise.allSettled(t), we().getState().appReady();\n    const c = process.argv.find((u) => u.startsWith(`${ut.defaultDesktopURLScheme}://`) || u.startsWith(`${ut.altDesktopURLScheme}://`));\n    c && bi(c), Pe && console.log(e);",
}];

/// Find the vite-built main JS in `app_dir/.vite/build/main-*.js`.
fn find_build_js(app_dir: &Path, prefix: &str) -> Result<PathBuf> {
    let build_dir = app_dir.join(".vite/build");
    for entry in
        std::fs::read_dir(&build_dir).with_context(|| format!("reading {}", build_dir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(".js") {
            return Ok(entry.path());
        }
    }
    bail!("no {prefix}*.js found in {}", build_dir.display())
}

pub fn apply(app_dir: &Path) -> Result<()> {
    let main_js_path = find_build_js(app_dir, "main-")?;
    let app_bootstrap_path = find_build_js(app_dir, "app-bootstrap-")?;

    // Prepare both files before writing either one. If a new upstream release
    // changes any target, the extracted app remains untouched and can be
    // inspected or retried without being left half-patched.
    let patched_main = prepare_patches(&main_js_path, MAIN_PATCHES)?;
    let patched_app_bootstrap = prepare_patches(&app_bootstrap_path, APP_BOOTSTRAP_PATCHES)?;

    std::fs::write(&main_js_path, patched_main)?;
    std::fs::write(&app_bootstrap_path, patched_app_bootstrap)?;
    eprintln!(
        "  {} patches applied",
        MAIN_PATCHES.len() + APP_BOOTSTRAP_PATCHES.len()
    );
    Ok(())
}

fn prepare_patches(js_path: &Path, patches: &[Patch]) -> Result<String> {
    eprintln!("  patching {}", js_path.display());

    let mut src = std::fs::read_to_string(js_path)
        .with_context(|| format!("reading {}", js_path.display()))?;

    for patch in patches {
        if !src.contains(patch.from) {
            let context = diagnostic_context(&src, patch.from);
            bail!(
                "patch target not found (app may have updated):\n  expected: {}\n  nearest context: {}",
                preview(patch.from, 120),
                context.unwrap_or_else(|| "no related context found".to_string())
            );
        }
        src = src.replacen(patch.from, patch.to, 1);
    }

    Ok(src)
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
        let main_js_path = build.join("main-test.js");
        let main_src = MAIN_PATCHES
            .iter()
            .map(|patch| patch.from)
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(&main_js_path, main_src).unwrap();
        let app_bootstrap_path = build.join("app-bootstrap-test.js");
        let app_bootstrap_src = APP_BOOTSTRAP_PATCHES
            .iter()
            .map(|patch| patch.from)
            .collect::<Vec<_>>()
            .join("\n\n");
        std::fs::write(&app_bootstrap_path, app_bootstrap_src).unwrap();

        apply(&root).unwrap();

        let patched_main = std::fs::read_to_string(&main_js_path).unwrap();
        for patch in MAIN_PATCHES {
            if !patch.to.contains(patch.from) {
                assert!(!patched_main.contains(patch.from));
            }
            assert!(patched_main.contains(patch.to));
        }
        let patched_app_bootstrap = std::fs::read_to_string(&app_bootstrap_path).unwrap();
        for patch in APP_BOOTSTRAP_PATCHES {
            if !patch.to.contains(patch.from) {
                assert!(!patched_app_bootstrap.contains(patch.from));
            }
            assert!(patched_app_bootstrap.contains(patch.to));
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
        std::fs::write(
            build.join("app-bootstrap-test.js"),
            APP_BOOTSTRAP_PATCHES[0].from,
        )
        .unwrap();

        let err = apply(&root).unwrap_err().to_string();
        assert!(err.contains("patch target not found"));
        assert!(err.contains("nearest context"));

        let main = std::fs::read_to_string(build.join("main-test.js")).unwrap();
        assert_eq!(main, "function $Ye() {\n  changed();\n}");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_does_not_write_main_when_bootstrap_patch_is_missing() {
        let root = temp_dir("patch-atomic");
        let build = root.join(".vite/build");
        std::fs::create_dir_all(&build).unwrap();
        let main_src = MAIN_PATCHES
            .iter()
            .map(|patch| patch.from)
            .collect::<Vec<_>>()
            .join("\n\n");
        let main_path = build.join("main-test.js");
        std::fs::write(&main_path, &main_src).unwrap();
        std::fs::write(build.join("app-bootstrap-test.js"), "changed bootstrap").unwrap();

        assert!(apply(&root).is_err());
        assert_eq!(std::fs::read_to_string(main_path).unwrap(), main_src);

        std::fs::remove_dir_all(root).unwrap();
    }
}
