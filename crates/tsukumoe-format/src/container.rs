// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::error::Error;
use crate::manifest::Manifest;

pub const MANIFEST_PATH: &str = "manifest.json";

/// Bounds on what a reader accepts, so a damaged or hostile file cannot
/// exhaust memory.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_file_bytes: 256 << 20,
            max_total_bytes: 1 << 30,
        }
    }
}

/// The content of a project: the manifest and every other file, by path
/// (relative, `/`-separated). Both layouts read into and write from this.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}

/// JSON as the format writes it: struct fields in declaration order, maps in
/// key order, two-space indent and a final newline, so diffs stay readable.
pub fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let mut out = serde_json::to_vec_pretty(value).map_err(|e| Error::Json {
        file: String::new(),
        message: e.to_string(),
    })?;
    out.push(b'\n');
    Ok(out)
}

fn check_path(path: &str) -> Result<(), Error> {
    let bad = path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|s| matches!(s, "" | "." | ".."));
    if bad {
        return Err(Error::UnsafePath(path.to_owned()));
    }
    Ok(())
}

impl Project {
    pub fn new(manifest: Manifest) -> Self {
        Self {
            manifest,
            files: BTreeMap::new(),
        }
    }

    fn from_files(mut files: BTreeMap<String, Vec<u8>>) -> Result<Self, Error> {
        let bytes = files
            .remove(MANIFEST_PATH)
            .ok_or_else(|| Error::MissingFile(MANIFEST_PATH.to_owned()))?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| Error::Json {
            file: MANIFEST_PATH.to_owned(),
            message: e.to_string(),
        })?;
        manifest.check()?;
        Ok(Self { manifest, files })
    }

    pub fn read_zip(bytes: &[u8]) -> Result<Self, Error> {
        Self::read_zip_with(bytes, &Limits::default())
    }

    pub fn read_zip_with(bytes: &[u8], limits: &Limits) -> Result<Self, Error> {
        let zip_error = |e: zip::result::ZipError| Error::Zip(e.to_string());
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
        if archive.len() > limits.max_entries {
            return Err(Error::TooLarge("archive (entries)".to_owned()));
        }
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(zip_error)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().map_err(zip_error)?.into_owned();
            check_path(&name)?;
            // The declared size is not trusted: read at most one byte past the limit.
            let mut data = Vec::new();
            entry
                .take(limits.max_file_bytes + 1)
                .read_to_end(&mut data)
                .map_err(|e| Error::Zip(e.to_string()))?;
            if data.len() as u64 > limits.max_file_bytes {
                return Err(Error::TooLarge(name));
            }
            total += data.len() as u64;
            if total > limits.max_total_bytes {
                return Err(Error::TooLarge("archive".to_owned()));
            }
            if files.insert(name.clone(), data).is_some() {
                return Err(Error::DuplicatePath(name));
            }
        }
        Self::from_files(files)
    }

    /// The same project gives the same bytes: entries are sorted, timestamps
    /// are fixed and permissions are normalized.
    pub fn write_zip(&self) -> Result<Vec<u8>, Error> {
        let zip_error = |e: zip::result::ZipError| Error::Zip(e.to_string());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let mut put = |name: &str, data: &[u8]| -> Result<(), Error> {
            check_path(name)?;
            writer.start_file(name, options).map_err(zip_error)?;
            writer
                .write_all(data)
                .map_err(|e| Error::Zip(e.to_string()))
        };
        put(MANIFEST_PATH, &to_json(&self.manifest)?)?;
        for (name, data) in &self.files {
            if name == MANIFEST_PATH {
                return Err(Error::DuplicatePath(name.clone()));
            }
            put(name, data)?;
        }
        Ok(writer.finish().map_err(zip_error)?.into_inner())
    }

    pub fn read_dir(dir: &Path) -> Result<Self, Error> {
        Self::read_dir_with(dir, &Limits::default())
    }

    pub fn read_dir_with(dir: &Path, limits: &Limits) -> Result<Self, Error> {
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        walk(dir, "", limits, &mut files, &mut total)?;
        Self::from_files(files)
    }

    /// Writes the files under `dir`, creating it. Files already there that
    /// the project does not contain are left alone.
    pub fn write_dir(&self, dir: &Path) -> Result<(), Error> {
        let write = |name: &str, data: &[u8]| -> Result<(), Error> {
            check_path(name)?;
            let path = dir.join(name);
            let io = |source| Error::Io {
                path: path.display().to_string(),
                source,
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(io)?;
            }
            fs::write(&path, data).map_err(io)
        };
        write(MANIFEST_PATH, &to_json(&self.manifest)?)?;
        for (name, data) in &self.files {
            if name == MANIFEST_PATH {
                return Err(Error::DuplicatePath(name.clone()));
            }
            write(name, data)?;
        }
        Ok(())
    }
}

fn walk(
    dir: &Path,
    prefix: &str,
    limits: &Limits,
    files: &mut BTreeMap<String, Vec<u8>>,
    total: &mut u64,
) -> Result<(), Error> {
    let io = |path: &Path| {
        let path = path.display().to_string();
        move |source| Error::Io { path, source }
    };
    for entry in fs::read_dir(dir).map_err(io(dir))? {
        let entry = entry.map_err(io(dir))?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| Error::UnsafePath(n.to_string_lossy().into_owned()))?;
        let relative = format!("{prefix}{name}");
        // A link could point outside the project, so links are not followed.
        let kind = fs::symlink_metadata(&path).map_err(io(&path))?;
        if kind.is_dir() {
            walk(&path, &format!("{relative}/"), limits, files, total)?;
        } else if kind.is_file() {
            check_path(&relative)?;
            if kind.len() > limits.max_file_bytes {
                return Err(Error::TooLarge(relative));
            }
            *total += kind.len();
            if *total > limits.max_total_bytes || files.len() >= limits.max_entries {
                return Err(Error::TooLarge("directory".to_owned()));
            }
            files.insert(relative, fs::read(&path).map_err(io(&path))?);
        } else {
            return Err(Error::UnsafePath(relative));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Feature;

    fn sample() -> Project {
        let mut p = Project::new(Manifest::new(Some("test".into())));
        p.files.insert("model.json".into(), b"{}\n".to_vec());
        p.files
            .insert("textures/a.png".into(), vec![0x89, b'P', b'N', b'G']);
        p
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn manifest_json(version: u32, features: &str) -> Vec<u8> {
        format!(
            r#"{{"format":"tsukumoe-project","version":{{"major":{version},"minor":0}},"features":{features}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn zip_round_trip_keeps_every_byte() {
        let p = sample();
        let bytes = p.write_zip().unwrap();
        assert_eq!(Project::read_zip(&bytes).unwrap(), p);
        assert_eq!(bytes, p.write_zip().unwrap(), "output is deterministic");
    }

    #[test]
    fn manifest_json_is_stable_and_readable() {
        let text = String::from_utf8(to_json(&Manifest::new(Some("g".into()))).unwrap()).unwrap();
        assert_eq!(
            text,
            "{\n  \"format\": \"tsukumoe-project\",\n  \"version\": {\n    \"major\": 1,\n    \"minor\": 0\n  },\n  \"generator\": \"g\",\n  \"features\": []\n}\n"
        );
    }

    #[test]
    fn damaged_archives_return_errors_without_panicking() {
        let bytes = sample().write_zip().unwrap();
        for len in 0..bytes.len() {
            let _ = Project::read_zip(&bytes[..len]);
        }
        for i in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[i] ^= 0xFF;
            let _ = Project::read_zip(&damaged);
        }
        assert!(matches!(
            Project::read_zip(b"not a zip"),
            Err(Error::Zip(_))
        ));
    }

    #[test]
    fn invalid_json_is_an_error() {
        let bytes = zip_with(&[(MANIFEST_PATH, b"{ nope")]);
        assert!(matches!(Project::read_zip(&bytes), Err(Error::Json { .. })));
        let bytes = zip_with(&[("model.json", b"{}")]);
        assert!(matches!(
            Project::read_zip(&bytes),
            Err(Error::MissingFile(_))
        ));
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for name in ["../evil", "/abs", "a/../b", "a\\b", "C:/x", "a//b"] {
            let bytes = zip_with(&[(MANIFEST_PATH, &manifest_json(1, "[]")), (name, b"x")]);
            assert!(
                matches!(Project::read_zip(&bytes), Err(Error::UnsafePath(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn newer_major_versions_are_rejected() {
        let bytes = zip_with(&[(MANIFEST_PATH, &manifest_json(2, "[]"))]);
        assert!(matches!(
            Project::read_zip(&bytes),
            Err(Error::UnsupportedVersion {
                found: 2,
                supported: 1
            })
        ));
        let bytes = zip_with(&[(MANIFEST_PATH, &manifest_json(0, "[]"))]);
        assert!(matches!(
            Project::read_zip(&bytes),
            Err(Error::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn unknown_optional_features_are_kept_and_required_ones_rejected() {
        let optional = r#"[{"name":"future","required":false}]"#;
        let bytes = zip_with(&[(MANIFEST_PATH, &manifest_json(1, optional))]);
        let p = Project::read_zip(&bytes).unwrap();
        assert_eq!(
            p.manifest.features,
            [Feature {
                name: "future".into(),
                required: false
            }]
        );
        assert_eq!(Project::read_zip(&p.write_zip().unwrap()).unwrap(), p);

        let required = r#"[{"name":"future","required":true}]"#;
        let bytes = zip_with(&[(MANIFEST_PATH, &manifest_json(1, required))]);
        assert!(matches!(
            Project::read_zip(&bytes),
            Err(Error::RequiredFeature(name)) if name == "future"
        ));
    }

    #[test]
    fn other_formats_are_not_projects() {
        let json = br#"{"format":"other","version":{"major":1,"minor":0}}"#;
        let bytes = zip_with(&[(MANIFEST_PATH, json)]);
        assert!(matches!(Project::read_zip(&bytes), Err(Error::NotAProject)));
    }

    #[test]
    fn size_limits_apply_to_the_data_read_not_the_declared_size() {
        let p = sample();
        let bytes = p.write_zip().unwrap();
        let tight = Limits {
            max_file_bytes: 3,
            ..Limits::default()
        };
        assert!(matches!(
            Project::read_zip_with(&bytes, &tight),
            Err(Error::TooLarge(_))
        ));
        let few = Limits {
            max_entries: 1,
            ..Limits::default()
        };
        assert!(matches!(
            Project::read_zip_with(&bytes, &few),
            Err(Error::TooLarge(_))
        ));
        let small = Limits {
            max_total_bytes: 10,
            ..Limits::default()
        };
        assert!(matches!(
            Project::read_zip_with(&bytes, &small),
            Err(Error::TooLarge(_))
        ));
    }
}
