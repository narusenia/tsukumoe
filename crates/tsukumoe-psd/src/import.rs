// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Reading a PSD file for an import: the part the CLI and the editor share.
//! The file is read once to hash it, once to scan it, and again when the
//! textures are written, so no more than one layer is in memory at a time.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use tsukumoe_core::model::Model;

use crate::map::{PsdMap, Source};
use crate::plan::{Choices, Existing, Plan, plan, render_textures};
use crate::reader::{Limits, Psd};
use crate::scan::Scan;

/// A PSD that has been read and scanned.
pub struct Prepared {
    pub path: PathBuf,
    pub psd: Psd,
    pub scan: Scan,
    pub source: Source,
    pub limits: Limits,
}

fn open_file(path: &Path) -> Result<BufReader<File>, String> {
    File::open(path)
        .map(|f| BufReader::with_capacity(1 << 20, f))
        .map_err(|e| format!("{}: {e}", path.display()))
}

impl Prepared {
    /// Reads the structure and scans every layer. `progress` gets (done, total).
    pub fn open(
        path: &Path,
        limits: Limits,
        progress: impl FnMut(usize, usize),
    ) -> Result<Self, String> {
        let sha256 = crate::hash_reader(&mut open_file(path)?).map_err(|e| e.to_string())?;
        let mut reader = open_file(path)?;
        let psd = Psd::read(&mut reader, &limits).map_err(|e| e.to_string())?;
        let scan = Scan::new(&psd, &mut reader, &limits, progress).map_err(|e| e.to_string())?;
        let source = Source {
            path: path.display().to_string(),
            sha256,
            canvas: [psd.width, psd.height],
        };
        Ok(Self {
            path: path.to_owned(),
            psd,
            scan,
            source,
            limits,
        })
    }

    /// The plan against a project. `map` is the content of `sources/psd-map.json`.
    pub fn plan(
        &self,
        model: &Model,
        textures: &[String],
        map: Option<&[u8]>,
        choices: &Choices,
    ) -> Result<Plan, String> {
        let map = map.map(PsdMap::parse).transpose()?;
        Ok(plan(
            &self.scan,
            &Existing { model, textures },
            map.as_ref(),
            choices,
            &self.source,
        ))
    }

    /// Decodes the planned layers again, one at a time, and hands each PNG to
    /// `sink`. Call this after the plan's batch applied.
    pub fn render(
        &self,
        plan: &Plan,
        sink: impl FnMut(&str, Vec<u8>) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut reader = open_file(&self.path)?;
        render_textures(plan, &self.scan, &self.psd, &mut reader, &self.limits, sink)
    }
}
