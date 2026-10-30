//! The on-disk form of one kv scope (§3 "kv data"):
//! `<data_dir>/arcode/plugin-data/<id>/{global,<hash(root)>}.kv`.
//!
//! Format: the line `ARCODE-KV 1\n`, then per entry a little-endian `u32`
//! key length, the key, a `u32` value length and the value. Files are
//! written atomically (temp file + rename) into owner-only directories.
//! Reading is strict: a damaged file, an invalid key or a file over the
//! quota reads as empty (logged), never as partial data.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::paths::create_private_dir;

const MAGIC: &[u8] = b"ARCODE-KV 1\n";

/// Encodes entries in the file format.
pub fn encode(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let size: usize = entries.iter().map(|(k, v)| 8 + k.len() + v.len()).sum();
    let mut out = Vec::with_capacity(MAGIC.len() + size);
    out.extend_from_slice(MAGIC);
    for (key, value) in entries {
        for part in [key.as_bytes(), value.as_slice()] {
            let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(part);
        }
    }
    out
}

/// Decodes a file; `valid_key` and `quota` are re-checked so a tampered
/// file cannot bypass them.
pub fn decode(
    bytes: &[u8],
    valid_key: impl Fn(&str) -> bool,
    quota: usize,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut rest = bytes.strip_prefix(MAGIC).ok_or("not an Arcode kv file")?;
    let mut entries = BTreeMap::new();
    let mut used = 0usize;
    while !rest.is_empty() {
        let key = take(&mut rest, "key")?;
        let value = take(&mut rest, "value")?;
        let key = String::from_utf8(key.to_vec()).map_err(|_| "key is not UTF-8")?;
        if !valid_key(&key) {
            return Err(format!("invalid key {key:?}"));
        }
        used += key.len() + value.len();
        if used > quota {
            return Err("over the quota".into());
        }
        entries.insert(key, value.to_vec());
    }
    Ok(entries)
}

/// Splits one length-prefixed part off the front of `rest`.
fn take<'a>(rest: &mut &'a [u8], what: &str) -> Result<&'a [u8], String> {
    let (len, after) = rest
        .split_first_chunk::<4>()
        .ok_or_else(|| format!("truncated {what} length"))?;
    let len = u32::from_le_bytes(*len) as usize;
    if after.len() < len {
        return Err(format!("truncated {what}"));
    }
    let (part, after) = after.split_at(len);
    *rest = after;
    Ok(part)
}

/// Reads a file of at most `max_bytes`; a missing file is empty.
pub fn read(path: &Path, max_bytes: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::other("file too large"));
    }
    Ok(Some(bytes))
}

/// Writes `bytes` to `path` atomically, creating the directory owner-only.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    create_private_dir(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_strict_decoding() {
        let mut m = BTreeMap::new();
        m.insert("a".to_owned(), b"1".to_vec());
        m.insert("zz/key".to_owned(), vec![0, 255, 7]);
        m.insert("empty".to_owned(), Vec::new());
        let bytes = encode(&m);
        let ok = |_: &str| true;
        assert_eq!(decode(&bytes, ok, 1 << 20).unwrap(), m);
        assert_eq!(decode(MAGIC, ok, 0).unwrap(), BTreeMap::new());
        assert!(decode(b"junk", ok, 1 << 20).is_err());
        assert!(decode(&bytes[..bytes.len() - 1], ok, 1 << 20).is_err());
        assert!(decode(&bytes, |k| k != "a", 1 << 20).is_err());
        assert!(decode(&bytes, ok, 5).unwrap_err().contains("quota"));
    }

    #[test]
    fn atomic_write_and_bounded_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p/q/global.kv");
        assert_eq!(read(&path, 10).unwrap(), None);
        write_atomic(&path, b"hello").unwrap();
        write_atomic(&path, b"world").unwrap();
        assert_eq!(read(&path, 10).unwrap().as_deref(), Some(&b"world"[..]));
        assert!(read(&path, 4).is_err());
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temp files left");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }
}
