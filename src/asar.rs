// ASAR format (Chromium pickle encoding):
//
//   [u32: 4]             size-pickle payload size (always 4)
//   [u32: S]             size-pickle payload = S (= total bytes of header-pickle)
//   [u32: P]             header-pickle payload size (= 4 + roundUp(json_len, 4))
//   [u32: json_len]      length of JSON string
//   [json_len bytes]     JSON header (file tree with byte offsets)
//   [padding to 4-byte]
//   [file data...]       offsets in the JSON are relative to this position
//
// S = 4 + P_padded, where P_padded = roundUp(P, 4)
// data starts at byte offset: 8 + S = 16 + roundUp(json_len, 4)

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ── read ─────────────────────────────────────────────────────────────────────

pub fn extract(asar_path: &Path, dest: &Path) -> Result<()> {
    let raw =
        std::fs::read(asar_path).with_context(|| format!("reading {}", asar_path.display()))?;

    if raw.len() < 16 {
        bail!("file too small to be an ASAR archive");
    }

    // bytes[4..8] = total size of the header pickle (including its 4-byte length prefix)
    let header_pickle_size = u32::from_le_bytes(raw[4..8].try_into()?) as usize;
    // bytes[12..16] = JSON string length
    let json_len = u32::from_le_bytes(raw[12..16].try_into()?) as usize;

    if raw.len() < 16 + json_len {
        bail!("ASAR header truncated");
    }

    let json_str = std::str::from_utf8(&raw[16..16 + json_len])
        .context("ASAR header JSON is not valid UTF-8")?;
    let header: Value = serde_json::from_str(json_str).context("parsing ASAR header JSON")?;

    let data_base = 8 + header_pickle_size;
    std::fs::create_dir_all(dest)?;

    let files = header
        .get("files")
        .context("ASAR header missing 'files' key")?;
    extract_dir(&raw, data_base, dest, files)
}

fn extract_dir(data: &[u8], base: usize, dir: &Path, node: &Value) -> Result<()> {
    let map = node
        .as_object()
        .context("expected JSON object for directory")?;
    for (name, info) in map {
        let path = dir.join(name);
        if let Some(files) = info.get("files") {
            std::fs::create_dir_all(&path)?;
            extract_dir(data, base, &path, files)?;
        } else {
            let offset: usize = info["offset"]
                .as_str()
                .context("offset must be a string")?
                .parse()
                .context("offset not a valid integer")?;
            let size: usize = info["size"].as_u64().context("size must be a number")? as usize;

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &data[base + offset..base + offset + size])?;

            if info
                .get("executable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    Ok(())
}

// ── write ─────────────────────────────────────────────────────────────────────

pub fn pack(src_dir: &Path, dest_asar: &Path) -> Result<()> {
    let mut file_data: Vec<u8> = Vec::new();
    let files_node = build_tree(src_dir, &mut file_data)?;
    let header = json!({ "files": files_node });

    let header_json = serde_json::to_string(&header)?;
    let json_bytes = header_json.as_bytes();
    let json_len = json_bytes.len();
    let padded_len = (json_len + 3) & !3;

    // header-pickle payload size (stored in its own length field, before padding)
    let header_pickle_payload = 4 + json_len;
    // header-pickle total bytes on disk = length-field (4) + padded payload
    let header_pickle_total = 4 + ((header_pickle_payload + 3) & !3);

    let mut out = std::fs::File::create(dest_asar)
        .with_context(|| format!("creating {}", dest_asar.display()))?;

    use std::io::Write;
    // size-pickle
    out.write_all(&4u32.to_le_bytes())?;
    out.write_all(&(header_pickle_total as u32).to_le_bytes())?;
    // header-pickle
    out.write_all(&(header_pickle_payload as u32).to_le_bytes())?;
    out.write_all(&(json_len as u32).to_le_bytes())?;
    out.write_all(json_bytes)?;
    out.write_all(&vec![0u8; padded_len - json_len])?;
    // file data
    out.write_all(&file_data)?;

    Ok(())
}

fn build_tree(dir: &Path, data: &mut Vec<u8>) -> Result<Value> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());

    let mut map = serde_json::Map::new();
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = std::fs::symlink_metadata(&path)?;

        if meta.is_dir() {
            let child_files = build_tree(&path, data)?;
            map.insert(name, json!({ "files": child_files }));
        } else if meta.is_file() {
            let offset = data.len();
            let contents = std::fs::read(&path)?;
            let size = contents.len();
            data.extend_from_slice(&contents);

            let executable = (meta.permissions().mode() & 0o111) != 0;
            let mut info = json!({ "size": size, "offset": offset.to_string() });
            if executable {
                info["executable"] = json!(true);
            }
            map.insert(name, info);
        }
        // symlinks skipped — not used in app.asar content
    }
    Ok(Value::Object(map))
}
