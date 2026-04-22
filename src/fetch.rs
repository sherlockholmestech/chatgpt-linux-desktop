use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use std::cmp::Reverse;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const ELECTRON_BASE: &str = "https://github.com/electron/electron/releases/download";
const RG_ADGUARD_API: &str = "https://store.rg-adguard.net/api/GetFiles";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

// ── internal download ─────────────────────────────────────────────────────────

fn progress_bar(total: Option<u64>) -> ProgressBar {
    let pb = ProgressBar::new(total.unwrap_or(0));
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb
}

fn download(url: &str, dest: &Path) -> Result<()> {
    let resp = ureq::get(url)
        .header("User-Agent", UA)
        .call()
        .with_context(|| format!("GET {url}"))?;

    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let pb = progress_bar(total);
    let mut reader = resp.into_body().into_reader();
    let mut file =
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        pb.inc(n as u64);
    }
    pb.finish_and_clear();
    Ok(())
}

struct RgFile {
    filename: String,
    url: String,
}

fn classify_score(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    let mut score = 0;
    if lower.ends_with(".msixbundle") || lower.ends_with(".appxbundle") {
        score += 100;
    }
    if lower.ends_with(".msix") || lower.ends_with(".appx") {
        score += 50;
    }
    if lower.contains("_x64") {
        score += 20;
    }
    if lower.contains("neutral") {
        score += 5;
    }
    if lower.contains("blockmap")
        || lower.contains("eappx")
        || lower.contains("symbol")
        || lower.contains("test")
    {
        score -= 200;
    }
    score
}

fn is_package_candidate(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".msixbundle")
        || lower.ends_with(".appxbundle")
        || lower.ends_with(".msix")
        || lower.ends_with(".appx"))
        && !lower.contains("blockmap")
        && !lower.contains("eappx")
        && !lower.contains("symbol")
        && !lower.contains("test")
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn filename_from_url(url: &str) -> Option<String> {
    let no_query = url.split('?').next()?;
    let raw = no_query.rsplit('/').next()?;
    if raw.is_empty() {
        return None;
    }
    Some(raw.to_string())
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn parse_rg_adguard_html(body: &str) -> Vec<RgFile> {
    let mut out = Vec::new();
    let mut rest = body;
    let anchor_start = "<a";

    while let Some(a_pos) = rest.find(anchor_start) {
        let after_a = &rest[a_pos..];
        let Some(tag_end) = after_a.find('>') else {
            break;
        };
        let tag = &after_a[..tag_end + 1];
        let Some(href_pos) = tag.find("href=\"") else {
            rest = &after_a[tag_end + 1..];
            continue;
        };
        let href_after = &tag[href_pos + 6..];
        let Some(href_end) = href_after.find('"') else {
            rest = &after_a[tag_end + 1..];
            continue;
        };
        let href_raw = &href_after[..href_end];

        let content_after = &after_a[tag_end + 1..];
        let Some(close_pos) = content_after.find("</a>") else {
            break;
        };
        let anchor_text = strip_tags(&content_after[..close_pos]);

        rest = &content_after[close_pos + 4..];

        if !href_raw.starts_with("http") {
            continue;
        }

        let url = href_raw.replace("&amp;", "&");
        let filename = {
            let text = anchor_text.trim();
            if text.is_empty() {
                filename_from_url(&url)
            } else {
                Some(text.to_string())
            }
        };
        let Some(filename) = filename else {
            continue;
        };

        out.push(RgFile { filename, url });
    }

    out
}

pub fn download_msix_from_rg_adguard(
    cache_dir: &Path,
    store_query: &str,
    ring: &str,
) -> Result<PathBuf> {
    let resp = ureq::post(RG_ADGUARD_API)
        .header("User-Agent", UA)
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Origin", "https://store.rg-adguard.net")
        .header("Referer", "https://store.rg-adguard.net/")
        .send_form([
            ("type", "url"),
            ("url", store_query),
            ("ring", ring),
            ("lang", "en-US"),
        ])
        .with_context(|| format!("POST {RG_ADGUARD_API}"))?;

    let mut body = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut body)
        .context("reading rg-adguard response")?;

    if body.contains("Just a moment") || body.contains("cf_chl_opt") {
        bail!("rg-adguard request blocked by Cloudflare challenge (try again later)");
    }

    let mut files = parse_rg_adguard_html(&body);

    files.retain(|f| is_package_candidate(&f.filename));
    files.sort_by_key(|f| {
        (
            Reverse(classify_score(&f.filename)),
            Reverse(f.filename.len()),
        )
    });

    let best = files
        .first()
        .context("rg-adguard returned no usable msix/appx package links")?;

    let filename = sanitize_filename(&best.filename);
    let dest = cache_dir.join(filename);
    if dest.exists() {
        eprintln!("  cached: {}", dest.display());
        return Ok(dest);
    }

    if best.url.trim().is_empty() {
        bail!("rg-adguard returned an empty download URL")
    }

    eprintln!("  downloading {}...", best.filename);
    download(&best.url, &dest)?;
    Ok(dest)
}

// ── public API ────────────────────────────────────────────────────────────────

/// Download and unzip Electron `version` into `cache_dir/electron-{version}/`.
/// Returns the directory containing the electron binary and resources.
pub fn fetch_electron(version: &str, cache_dir: &Path) -> Result<PathBuf> {
    let electron_dir = cache_dir.join(format!("electron-{version}"));
    if electron_dir.exists() {
        eprintln!("  cached electron v{version}");
        return Ok(electron_dir);
    }

    let zip_name = format!("electron-v{version}-linux-x64.zip");
    let url = format!("{ELECTRON_BASE}/v{version}/{zip_name}");
    let zip_path = cache_dir.join(&zip_name);

    if !zip_path.exists() {
        eprintln!("  downloading electron v{version}...");
        download(&url, &zip_path)?;
    }

    eprintln!("  extracting electron...");
    std::fs::create_dir_all(&electron_dir)?;
    crate::extract::unzip(&zip_path, &electron_dir)?;

    Ok(electron_dir)
}

/// Return (and create) the user-level cache directory.
pub fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".cache"));
    let dir = base.join("chatgpt-linux-desktop");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}
