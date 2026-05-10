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
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

// ── read ─────────────────────────────────────────────────────────────────────

pub fn extract(asar_path: &Path, dest: &Path) -> Result<()> {
    let raw =
        std::fs::read(asar_path).with_context(|| format!("reading {}", asar_path.display()))?;

    if raw.len() < 16 {
        bail!("file too small to be an ASAR archive");
    }

    let pickle_payload_size = u32::from_le_bytes(raw[0..4].try_into()?);
    if pickle_payload_size != 4 {
        bail!("invalid ASAR size-pickle payload size: {pickle_payload_size}");
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
            let size = usize::try_from(info["size"].as_u64().context("size must be a number")?)?;
            let start = base
                .checked_add(offset)
                .context("ASAR file offset overflows usize")?;
            let end = start
                .checked_add(size)
                .context("ASAR file size overflows usize")?;
            if end > data.len() {
                bail!("ASAR entry {} points beyond archive data", path.display());
            }

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &data[start..end])?;

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

    // size-pickle
    out.write_all(&4u32.to_le_bytes())?;
    out.write_all(&u32::try_from(header_pickle_total)?.to_le_bytes())?;
    // header-pickle
    out.write_all(&u32::try_from(header_pickle_payload)?.to_le_bytes())?;
    out.write_all(&u32::try_from(json_len)?.to_le_bytes())?;
    out.write_all(json_bytes)?;
    write_padding(&mut out, padded_len - json_len)?;
    // file data
    out.write_all(&file_data)?;

    Ok(())
}

fn write_padding(out: &mut std::fs::File, len: usize) -> Result<()> {
    const ZEROES: [u8; 4] = [0; 4];
    out.write_all(&ZEROES[..len])?;
    Ok(())
}

fn build_tree(dir: &Path, data: &mut Vec<u8>) -> Result<Value> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

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
    fn pack_extract_round_trip_files_and_executable_bit() {
        let root = temp_dir("asar-round-trip");
        let src = root.join("src");
        let dest = root.join("dest");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("hello.txt"), b"hello").unwrap();
        std::fs::write(src.join("nested/run.sh"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(
            src.join("nested/run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let archive = root.join("app.asar");
        pack(&src, &archive).unwrap();
        extract(&archive, &dest).unwrap();

        assert_eq!(std::fs::read(dest.join("hello.txt")).unwrap(), b"hello");
        assert_eq!(
            std::fs::read(dest.join("nested/run.sh")).unwrap(),
            b"#!/bin/sh\n"
        );
        let mode = std::fs::metadata(dest.join("nested/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extract_rejects_invalid_size_pickle() {
        let root = temp_dir("asar-invalid");
        let archive = root.join("bad.asar");
        std::fs::write(&archive, [0u8; 16]).unwrap();

        let err = extract(&archive, &root.join("out"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ASAR size-pickle"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
