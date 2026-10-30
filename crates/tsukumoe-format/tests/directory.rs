// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::fs;
use std::path::PathBuf;

use tsukumoe_format::{Error, Manifest, Project};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample() -> Project {
    let mut p = Project::new(Manifest::new(Some("test".into())));
    p.files.insert("model.json".into(), b"{}\n".to_vec());
    p.files.insert("motions/idle.json".into(), b"[]\n".to_vec());
    p.files.insert("textures/a.png".into(), vec![1, 2, 3]);
    p
}

#[test]
fn a_directory_round_trips_and_matches_the_archive() {
    let dir = scratch("round-trip");
    let p = sample();
    p.write_dir(&dir).unwrap();
    assert!(dir.join("manifest.json").is_file());
    assert!(dir.join("textures/a.png").is_file());
    let read = Project::read_dir(&dir).unwrap();
    assert_eq!(read, p);
    // Both layouts hold the same content.
    assert_eq!(Project::read_zip(&read.write_zip().unwrap()).unwrap(), p);
}

#[test]
fn an_edited_directory_is_read_again() {
    let dir = scratch("edited");
    sample().write_dir(&dir).unwrap();
    fs::write(dir.join("model.json"), b"{\"edited\":true}\n").unwrap();
    let read = Project::read_dir(&dir).unwrap();
    assert_eq!(read.files["model.json"], b"{\"edited\":true}\n");
}

#[test]
fn a_directory_without_a_manifest_is_an_error() {
    let dir = scratch("no-manifest");
    fs::write(dir.join("model.json"), b"{}").unwrap();
    assert!(matches!(
        Project::read_dir(&dir),
        Err(Error::MissingFile(_))
    ));
    assert!(matches!(
        Project::read_dir(&dir.join("missing")),
        Err(Error::Io { .. })
    ));
}

#[cfg(unix)]
#[test]
fn links_are_not_followed() {
    let dir = scratch("link");
    sample().write_dir(&dir).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", dir.join("textures/link.png")).unwrap();
    assert!(matches!(Project::read_dir(&dir), Err(Error::UnsafePath(_))));
}
