use anyhow::{bail, Context, Result};
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
        "  applyMainWindowStyle(u) {\n    u.setVibrancy(\"sidebar\");\n  }",
        "  applyMainWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"sidebar\");\n  }",
    ),
    (
        "  applyCompanionWindowStyle(u) {\n    u.setVibrancy(\"hud\");\n  }",
        "  applyCompanionWindowStyle(u) {\n    process.platform === \"darwin\" && u.setVibrancy(\"hud\");\n  }",
    ),
    (
        "function jpa() {\n  try {",
        "function jpa() {\n  if (process.platform === \"linux\")\n    return hu.hostname();\n  try {",
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
            bail!(
                "patch target not found (app may have updated):\n  {}",
                &from[..from.len().min(80)]
            );
        }
        src = src.replacen(from, to, 1);
    }

    std::fs::write(&js_path, src)?;
    eprintln!("  {} patches applied", PATCHES.len());
    Ok(())
}
