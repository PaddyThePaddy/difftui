use std::{io::Read as _, path::Path};

pub mod dir;
pub mod file;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum DiffSide {
    Left,
    Right,
}

impl DiffSide {
    pub fn oppsite(&self) -> Self {
        match self {
            DiffSide::Left => DiffSide::Right,
            DiffSide::Right => DiffSide::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffState {
    Unknown,
    Orphan(DiffSide),
    Different,
    Same,
}

impl DiffState {
    pub fn is_orphan(&self, side: DiffSide) -> bool {
        if let DiffState::Orphan(s) = self {
            *s == side
        } else {
            false
        }
    }
}

const CMP_BUFFER_SIZE: usize = 4096;

/// Compares two files byte-for-byte using buffered reads.
///
/// Reads both files in [`CMP_BUFFER_SIZE`]-byte chunks and returns as soon as
/// a difference is found, avoiding a full read when the files diverge early.
///
/// Returns `Ok(true)` when both files are identical, `Ok(false)` when they
/// differ, or an [`std::io::Error`] if either file cannot be opened or read.
fn compare_file<LP: AsRef<Path>, RP: AsRef<Path>>(lhs: LP, rhs: RP) -> std::io::Result<bool> {
    let lhs = lhs.as_ref();
    let rhs = rhs.as_ref();

    let mut lhs_f = std::fs::File::open(lhs)?;
    let mut rhs_f = std::fs::File::open(rhs)?;
    let mut lbuf = [0u8; CMP_BUFFER_SIZE];
    let mut rbuf = [0u8; CMP_BUFFER_SIZE];

    loop {
        let ln = lhs_f.read(&mut lbuf)?;
        let rn = rhs_f.read(&mut rbuf)?;
        if lbuf[..ln] != rbuf[..rn] {
            break Ok(false);
        }
        if ln == 0 {
            break Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use tempfile::NamedTempFile;

    use super::*;

    /// Writes `content` to a new temporary file and returns the handle.
    /// The file is deleted when the handle is dropped.
    fn tmp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f
    }

    #[test]
    fn identical_empty_files_are_same() {
        let a = tmp(b"");
        let b = tmp(b"");
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), true);
    }

    #[test]
    fn identical_content_is_same() {
        let a = tmp(b"hello world");
        let b = tmp(b"hello world");
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), true);
    }

    #[test]
    fn different_content_is_not_same() {
        let a = tmp(b"hello");
        let b = tmp(b"world");
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), false);
    }

    #[test]
    fn same_prefix_different_length_is_not_same() {
        let a = tmp(b"hello");
        let b = tmp(b"hello world");
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), false);
    }

    #[test]
    fn difference_at_last_byte_is_detected() {
        let mut content = vec![0u8; 100];
        let mut other = content.clone();
        *other.last_mut().unwrap() = 1;
        let a = tmp(&content);
        let b = tmp(&other);
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), false);
    }

    #[test]
    fn content_larger_than_buffer_identical_is_same() {
        let content = vec![0xABu8; CMP_BUFFER_SIZE * 3];
        let a = tmp(&content);
        let b = tmp(&content);
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), true);
    }

    #[test]
    fn content_larger_than_buffer_different_is_not_same() {
        let mut lhs = vec![0u8; CMP_BUFFER_SIZE * 3];
        let mut rhs = lhs.clone();
        // Difference sits in the third buffer chunk.
        rhs[CMP_BUFFER_SIZE * 2 + 1] = 1;
        let a = tmp(&lhs);
        let b = tmp(&rhs);
        assert_eq!(compare_file(a.path(), b.path()).unwrap(), false);
    }

    #[test]
    fn missing_file_returns_error() {
        let a = tmp(b"data");
        assert!(compare_file(a.path(), "/nonexistent/path/file.txt").is_err());
    }
}
