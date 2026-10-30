//! Loading and saving buffers.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::buffer::Buffer;
use crate::line_ending::LineEnding;

const BOM: &str = "\u{feff}";

impl Buffer {
    /// Reads a UTF-8 file. A leading BOM is stripped (and restored on save);
    /// invalid UTF-8 yields an [`io::ErrorKind::InvalidData`] error.
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Buffer> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let text =
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let (bom, body) = match text.strip_prefix(BOM) {
            Some(rest) => (true, rest),
            None => (false, text.as_str()),
        };
        let mut buffer = Buffer::new(body);
        buffer.bom = bom;
        buffer.path = Some(path.to_path_buf());
        Ok(buffer)
    }

    /// Writes the buffer to its path, restoring line ending and BOM.
    ///
    /// Fails with [`io::ErrorKind::InvalidInput`] if no path is set.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self.path.clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "buffer has no file path")
        })?;
        self.write_to_path(&path)?;
        self.mark_saved();
        Ok(())
    }

    /// Writes the buffer to `path` and associates it with that path.
    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> io::Result<()> {
        let path = path.into();
        self.write_to_path(&path)?;
        self.path = Some(path);
        self.mark_saved();
        Ok(())
    }

    /// Writes the on-disk representation (line ending, BOM) to `writer`.
    /// Streams the rope chunk by chunk without copying the whole text.
    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<()> {
        if self.bom {
            writer.write_all(BOM.as_bytes())?;
        }
        for chunk in self.rope.chunks() {
            match self.line_ending {
                LineEnding::Lf => writer.write_all(chunk.as_bytes())?,
                LineEnding::CrLf => {
                    let mut parts = chunk.split('\n');
                    if let Some(first) = parts.next() {
                        writer.write_all(first.as_bytes())?;
                    }
                    for part in parts {
                        writer.write_all(b"\r\n")?;
                        writer.write_all(part.as_bytes())?;
                    }
                }
            }
        }
        writer.flush()
    }

    fn write_to_path(&self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        self.write_to(&mut writer)?;
        writer.into_inner().map_err(|e| e.into_error())?.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn crlf_round_trip() {
        let dir = tmp();
        let path = dir.path().join("crlf.txt");
        std::fs::write(&path, "one\r\ntwo\r\nthree\r\n").expect("write");
        let mut b = Buffer::from_file(&path).expect("load");
        assert_eq!(b.to_string(), "one\ntwo\nthree\n");
        assert_eq!(b.line_ending(), LineEnding::CrLf);
        assert_eq!(b.path(), Some(path.as_path()));
        b.edit(vec![(3..3, "!".into())]);
        assert!(b.is_dirty());
        b.save().expect("save");
        assert!(!b.is_dirty());
        let on_disk = std::fs::read_to_string(&path).expect("read");
        assert_eq!(on_disk, "one!\r\ntwo\r\nthree\r\n");
    }

    #[test]
    fn lf_without_trailing_newline_round_trip() {
        let dir = tmp();
        let path = dir.path().join("lf.txt");
        std::fs::write(&path, "a\nb").expect("write");
        let mut b = Buffer::from_file(&path).expect("load");
        assert_eq!(b.line_ending(), LineEnding::Lf);
        b.save().expect("save");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "a\nb");
    }

    #[test]
    fn bom_is_preserved() {
        let dir = tmp();
        let path = dir.path().join("bom.txt");
        std::fs::write(&path, "\u{feff}héllo\r\n").expect("write");
        let mut b = Buffer::from_file(&path).expect("load");
        assert!(b.has_bom());
        assert_eq!(b.to_string(), "héllo\n");
        b.save().expect("save");
        assert_eq!(
            std::fs::read(&path).expect("read"),
            "\u{feff}héllo\r\n".as_bytes()
        );
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        let dir = tmp();
        let path = dir.path().join("bin");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("write");
        let err = Buffer::from_file(&path).expect_err("must fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn missing_file_and_missing_path() {
        let dir = tmp();
        assert!(Buffer::from_file(dir.path().join("nope")).is_err());
        let mut b = Buffer::new("x");
        let err = b.save().expect_err("no path");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn save_as_sets_path_and_cleans() {
        let dir = tmp();
        let path = dir.path().join("new.txt");
        let mut b = Buffer::new("x");
        b.edit(vec![(1..1, "\ny".into())]);
        assert!(b.is_dirty());
        b.save_as(&path).expect("save_as");
        assert_eq!(b.path(), Some(path.as_path()));
        assert!(!b.is_dirty());
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "x\ny");
        b.set_line_ending(LineEnding::CrLf);
        b.save().expect("save");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "x\r\ny");
    }

    #[test]
    fn crlf_across_many_chunks() {
        let text: String = (0..5000).map(|i| format!("line {i}\r\n")).collect();
        let b = Buffer::new(&text);
        let mut out = Vec::new();
        b.write_to(&mut out).expect("write");
        assert_eq!(String::from_utf8(out).expect("utf8"), text);
    }
}
