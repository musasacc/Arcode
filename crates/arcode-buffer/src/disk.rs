//! What a buffer's file held when the buffer last matched it (loaded,
//! saved or reloaded), to tell whether another program changed the file
//! since.

use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::SystemTime;

/// A fingerprint of a file's bytes and the metadata seen with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStamp {
    modified: Option<SystemTime>,
    len: u64,
    hash: u64,
}

/// How a file compares to a [`DiskStamp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskCheck {
    /// Same content (the metadata may differ, e.g. after `touch`).
    Same,
    /// Other content.
    Changed,
    /// The file is gone.
    Missing,
}

impl DiskStamp {
    /// The stamp of `bytes`, read along with `meta`.
    pub fn of_bytes(bytes: &[u8], meta: Option<&Metadata>) -> Self {
        let mut hash = Fnv::default();
        hash.update(bytes);
        Self::new(hash.0, bytes.len() as u64, meta)
    }

    /// Reads the file at `path` and stamps it.
    pub fn read(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let meta = file.metadata().ok();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Self::of_bytes(&bytes, meta.as_ref()))
    }

    fn new(hash: u64, len: u64, meta: Option<&Metadata>) -> Self {
        Self {
            modified: meta.and_then(|m| m.modified().ok()),
            len,
            hash,
        }
    }

    /// Compares the file at `path` with this stamp: unchanged metadata
    /// means the same content; otherwise the file is read and hashed. An
    /// unreadable file counts as the same (nothing to warn about).
    pub fn check(&self, path: &Path) -> DiskCheck {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return DiskCheck::Missing,
            Err(_) => return DiskCheck::Same,
        };
        if meta.len() == self.len
            && self.modified.is_some()
            && meta.modified().ok() == self.modified
        {
            return DiskCheck::Same;
        }
        match Self::read(path) {
            Ok(now) if now.hash == self.hash && now.len == self.len => DiskCheck::Same,
            Ok(_) => DiskCheck::Changed,
            Err(err) if err.kind() == io::ErrorKind::NotFound => DiskCheck::Missing,
            Err(_) => DiskCheck::Same,
        }
    }
}

/// FNV-1a: a small streaming hash (the same for any split of the bytes).
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A writer that hashes and counts what passes through it, so a save can
/// stamp the bytes it wrote without reading the file back.
pub(crate) struct StampingWriter<W> {
    inner: W,
    hash: Fnv,
    len: u64,
}

impl<W: Write> StampingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hash: Fnv::default(),
            len: 0,
        }
    }

    /// The inner writer and a function stamping everything written with
    /// the file's metadata (read once the data is on disk).
    pub(crate) fn finish(self) -> (W, impl FnOnce(Option<&Metadata>) -> DiskStamp) {
        let (hash, len) = (self.hash.0, self.len);
        (self.inner, move |meta| DiskStamp::new(hash, len, meta))
    }
}

impl<W: Write> Write for StampingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_how_bytes_are_split() {
        let whole = DiskStamp::of_bytes(b"hello world", None);
        let mut w = StampingWriter::new(Vec::new());
        w.write_all(b"hello").unwrap();
        w.write_all(b" world").unwrap();
        let (_, stamp) = w.finish();
        let split = stamp(None);
        assert_eq!(whole, split);
        assert_ne!(whole, DiskStamp::of_bytes(b"hello World", None));
    }

    #[test]
    fn check_tells_same_changed_and_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\n").unwrap();
        let stamp = DiskStamp::read(&path).unwrap();
        assert_eq!(stamp.check(&path), DiskCheck::Same);
        // Rewriting the same bytes (new mtime) is not a change.
        std::fs::write(&path, "one\n").unwrap();
        assert_eq!(stamp.check(&path), DiskCheck::Same);
        std::fs::write(&path, "two\n").unwrap();
        assert_eq!(stamp.check(&path), DiskCheck::Changed);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(stamp.check(&path), DiskCheck::Missing);
    }
}
