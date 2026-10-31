// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Verified weight storage. The only I/O is the filesystem under the store
//! directory and the caller-supplied [`Fetcher`].

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::AiError;
use crate::hash::Sha256Hex;
use crate::spec::{LicenseNotice, ModelSpec};

/// Opens a byte stream for a URL. A real HTTP implementation belongs to the
/// host; this crate has none.
pub trait Fetcher {
    fn fetch(&self, url: &str) -> io::Result<Box<dyn Read>>;
}

/// State of a model file in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Missing,
    Present,
    /// A file exists but its size or SHA-256 is wrong.
    Corrupt,
}

/// Weights under `<dir>/<id>/<version>/<file_name>`.
pub struct ModelStore {
    dir: PathBuf,
}

fn io_err(e: io::Error) -> AiError {
    AiError::Io(e.to_string())
}

/// Hashes a file, or returns `None` when its size differs from `size`.
fn file_hash_if_size(path: &Path, size: u64) -> io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let meta = f.metadata()?;
    if !meta.is_file() || meta.len() != size {
        return Ok(None);
    }
    let mut h = Sha256Hex::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(Some(h.finish()));
        }
        h.update(&buf[..n]);
    }
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Final location of `spec`'s file. Callers must have validated `spec`.
    pub fn path(&self, spec: &ModelSpec) -> PathBuf {
        self.dir
            .join(&spec.id)
            .join(&spec.version)
            .join(&spec.file_name)
    }

    pub fn status(&self, spec: &ModelSpec) -> Result<Status, AiError> {
        spec.validate()?;
        let path = self.path(spec);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Status::Missing),
            Err(e) => return Err(io_err(e)),
            Ok(_) => {}
        }
        match file_hash_if_size(&path, spec.size) {
            Ok(Some(h)) if h.eq_ignore_ascii_case(&spec.sha256) => Ok(Status::Present),
            Ok(_) => Ok(Status::Corrupt),
            Err(e) => Err(io_err(e)),
        }
    }

    /// Returns the verified file's path, fetching it first when it is missing
    /// or corrupt. `confirm` sees the license notice before any fetch and
    /// must return `true` to proceed; it is not called when the file is
    /// already present. The download goes to a temporary file that replaces
    /// the target only after size and SHA-256 check out; otherwise it is
    /// removed and any existing file is left as it was.
    pub fn ensure(
        &self,
        spec: &ModelSpec,
        fetcher: &dyn Fetcher,
        confirm: impl FnOnce(&LicenseNotice) -> bool,
    ) -> Result<PathBuf, AiError> {
        let path = self.path(spec);
        if self.status(spec)? == Status::Present {
            return Ok(path);
        }
        if !confirm(&spec.license_notice()) {
            return Err(AiError::Declined);
        }
        let reader = fetcher
            .fetch(&spec.url)
            .map_err(|e| AiError::Fetch(e.to_string()))?;
        let parent = path
            .parent()
            .ok_or_else(|| AiError::Io("no parent".into()))?;
        fs::create_dir_all(parent).map_err(io_err)?;
        // shortcut: fixed temp name, so two processes fetching the same
        // file race; upgrade if the host ever runs concurrent fetches.
        let tmp = path.with_extension("part");
        let result =
            download(reader, &tmp, spec).and_then(|()| fs::rename(&tmp, &path).map_err(io_err));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map(|()| path)
    }
}

fn download(reader: Box<dyn Read>, tmp: &Path, spec: &ModelSpec) -> Result<(), AiError> {
    let mut out = File::create(tmp).map_err(io_err)?;
    // One byte past the size is enough to detect an oversized stream.
    let mut src = reader.take(spec.size.saturating_add(1));
    let mut hash = Sha256Hex::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = src
            .read(&mut buf)
            .map_err(|e| AiError::Fetch(e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > spec.size {
            return Err(AiError::SizeExceeded {
                expected: spec.size,
            });
        }
        hash.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(io_err)?;
    }
    if total != spec.size {
        return Err(AiError::SizeMismatch {
            expected: spec.size,
            actual: total,
        });
    }
    if !hash.finish().eq_ignore_ascii_case(&spec.sha256) {
        return Err(AiError::HashMismatch);
    }
    out.sync_all().map_err(io_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_hex;
    use crate::spec::License;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "tsukumoe-ai-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Serves fixed bytes; optionally fails after `fail_after` bytes.
    struct Mem {
        bytes: Vec<u8>,
        fail_after: Option<usize>,
        calls: Cell<u32>,
    }
    impl Mem {
        fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.to_vec(),
                fail_after: None,
                calls: Cell::new(0),
            }
        }
    }
    struct FailingRead {
        data: Vec<u8>,
        pos: usize,
    }
    impl Read for FailingRead {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Err(io::Error::other("connection reset"));
            }
            let n = buf.len().min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }
    impl Fetcher for Mem {
        fn fetch(&self, _url: &str) -> io::Result<Box<dyn Read>> {
            self.calls.set(self.calls.get() + 1);
            match self.fail_after {
                Some(n) => Ok(Box::new(FailingRead {
                    data: self.bytes[..n].to_vec(),
                    pos: 0,
                })),
                None => Ok(Box::new(io::Cursor::new(self.bytes.clone()))),
            }
        }
    }

    const WEIGHTS: &[u8] = b"tiny weights for tests";

    fn spec_for(bytes: &[u8]) -> ModelSpec {
        ModelSpec {
            id: "m".into(),
            version: "1".into(),
            file_name: "m.bin".into(),
            size: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            license: License {
                spdx: "MIT".into(),
                name: "MIT".into(),
                url: "https://example.com/l".into(),
            },
            url: "https://example.com/m.bin".into(),
        }
    }

    fn leftovers(store: &ModelStore, spec: &ModelSpec) -> Vec<String> {
        fs::read_dir(store.path(spec).parent().unwrap())
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn fetch_verify_commit() {
        let t = TempDir::new();
        let (store, spec, f) = (ModelStore::new(&t.0), spec_for(WEIGHTS), Mem::new(WEIGHTS));
        assert_eq!(store.status(&spec).unwrap(), Status::Missing);
        let mut seen = None;
        let p = store
            .ensure(&spec, &f, |n| {
                seen = Some(n.clone());
                true
            })
            .unwrap();
        assert_eq!(seen.unwrap().license_spdx, "MIT");
        assert_eq!(fs::read(&p).unwrap(), WEIGHTS);
        assert_eq!(store.status(&spec).unwrap(), Status::Present);
        assert_eq!(leftovers(&store, &spec), vec!["m.bin"]);
    }

    #[test]
    fn present_file_skips_fetcher_and_confirmation() {
        let t = TempDir::new();
        let (store, spec, f) = (ModelStore::new(&t.0), spec_for(WEIGHTS), Mem::new(WEIGHTS));
        store.ensure(&spec, &f, |_| true).unwrap();
        store.ensure(&spec, &f, |_| panic!("must not ask")).unwrap();
        assert_eq!(f.calls.get(), 1);
    }

    #[test]
    fn declined_confirmation_does_not_fetch() {
        let t = TempDir::new();
        let (store, spec, f) = (ModelStore::new(&t.0), spec_for(WEIGHTS), Mem::new(WEIGHTS));
        assert!(matches!(
            store.ensure(&spec, &f, |_| false),
            Err(AiError::Declined)
        ));
        assert_eq!(f.calls.get(), 0);
        assert_eq!(store.status(&spec).unwrap(), Status::Missing);
    }

    #[test]
    fn oversized_stream_aborts_without_temp_file() {
        let t = TempDir::new();
        let store = ModelStore::new(&t.0);
        let spec = spec_for(WEIGHTS);
        let mut big = WEIGHTS.to_vec();
        big.extend_from_slice(&[0; 100_000]);
        let r = store.ensure(&spec, &Mem::new(&big), |_| true);
        assert!(matches!(r, Err(AiError::SizeExceeded { .. })));
        assert!(leftovers(&store, &spec).is_empty());
    }

    #[test]
    fn short_stream_and_hash_mismatch_are_discarded() {
        let t = TempDir::new();
        let store = ModelStore::new(&t.0);
        let spec = spec_for(WEIGHTS);
        let r = store.ensure(&spec, &Mem::new(&WEIGHTS[..5]), |_| true);
        assert!(matches!(r, Err(AiError::SizeMismatch { .. })));
        let mut wrong = WEIGHTS.to_vec();
        wrong[0] ^= 1;
        let r = store.ensure(&spec, &Mem::new(&wrong), |_| true);
        assert!(matches!(r, Err(AiError::HashMismatch)));
        assert!(leftovers(&store, &spec).is_empty());
        assert_eq!(store.status(&spec).unwrap(), Status::Missing);
    }

    #[test]
    fn corrupt_file_is_refetched() {
        let t = TempDir::new();
        let (store, spec, f) = (ModelStore::new(&t.0), spec_for(WEIGHTS), Mem::new(WEIGHTS));
        let p = store.path(&spec);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut bad = WEIGHTS.to_vec();
        bad[3] ^= 1;
        fs::write(&p, &bad).unwrap();
        assert_eq!(store.status(&spec).unwrap(), Status::Corrupt);
        store.ensure(&spec, &f, |_| true).unwrap();
        assert_eq!(fs::read(&p).unwrap(), WEIGHTS);
        assert_eq!(f.calls.get(), 1);
        fs::write(&p, b"short").unwrap();
        assert_eq!(store.status(&spec).unwrap(), Status::Corrupt);
    }

    #[test]
    fn midway_failure_leaves_existing_file_untouched() {
        let t = TempDir::new();
        let store = ModelStore::new(&t.0);
        let spec = spec_for(WEIGHTS);
        let p = store.path(&spec);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"old corrupt bytes").unwrap();
        let mut f = Mem::new(WEIGHTS);
        f.fail_after = Some(6);
        assert!(matches!(
            store.ensure(&spec, &f, |_| true),
            Err(AiError::Fetch(_))
        ));
        assert_eq!(fs::read(&p).unwrap(), b"old corrupt bytes");
        assert_eq!(leftovers(&store, &spec), vec!["m.bin"]);
    }

    #[test]
    fn invalid_spec_is_rejected_before_any_io() {
        let t = TempDir::new();
        let store = ModelStore::new(&t.0);
        let mut spec = spec_for(WEIGHTS);
        spec.id = "../evil".into();
        let f = Mem::new(WEIGHTS);
        assert!(matches!(
            store.ensure(&spec, &f, |_| true),
            Err(AiError::InvalidSpec(_))
        ));
        assert_eq!(f.calls.get(), 0);
    }
}
