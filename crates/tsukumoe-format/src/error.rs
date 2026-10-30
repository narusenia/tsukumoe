// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io {
        path: String,
        source: std::io::Error,
    },
    /// The archive is damaged or not a zip file.
    Zip(String),
    Json {
        file: String,
        message: String,
    },
    /// `manifest.json` does not name this format.
    NotAProject,
    /// The file's major version is newer than this reader supports.
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
    /// A feature the file requires is unknown to this reader.
    RequiredFeature(String),
    MissingFile(String),
    /// An absolute path, a `..` segment or another path that could escape the
    /// project directory.
    UnsafePath(String),
    DuplicatePath(String),
    TooLarge(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{path}: {source}"),
            Self::Zip(message) => write!(f, "invalid archive: {message}"),
            Self::Json { file, message } => write!(f, "{file}: invalid JSON: {message}"),
            Self::NotAProject => write!(f, "not a Tsukumoe project (manifest.json)"),
            Self::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "format version {found} is not supported (up to {supported})"
                )
            }
            Self::RequiredFeature(name) => write!(f, "required feature '{name}' is unknown"),
            Self::MissingFile(path) => write!(f, "missing file '{path}'"),
            Self::UnsafePath(path) => write!(f, "unsafe path '{path}'"),
            Self::DuplicatePath(path) => write!(f, "duplicate path '{path}'"),
            Self::TooLarge(what) => write!(f, "{what} is too large"),
        }
    }
}

impl std::error::Error for Error {}
