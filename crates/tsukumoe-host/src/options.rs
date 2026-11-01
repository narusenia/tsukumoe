// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command line: `tsukumoe-host <project> [--vmc-port <n>] [--mapping <json>] [--transparent]
//! [--syphon [--syphon-name <name>] [--syphon-size <w>x<h>]]
//! [--spout [--spout-name <name>] [--spout-size <w>x<h>]]`.

use std::fs;
use std::path::PathBuf;

use tsukumoe_tracking::{Overrides, Rule};

use crate::output::{SPOUT_AVAILABLE, SYPHON_AVAILABLE, parse_size};

pub const USAGE: &str = "usage: tsukumoe-host <project directory or archive> [--vmc-port <n>] [--mapping <json>] [--transparent] [--syphon [--syphon-name <name>] [--syphon-size <w>x<h>]] [--spout [--spout-name <name>] [--spout-size <w>x<h>]]";

/// The Syphon server name when `--syphon-name` is not given.
pub const DEFAULT_SYPHON_NAME: &str = "Tsukumoe";

/// The Spout sender name when `--spout-name` is not given.
pub const DEFAULT_SPOUT_NAME: &str = "Tsukumoe";

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub project: PathBuf,
    /// Tracking starts on this UDP port when set.
    pub vmc_port: Option<u16>,
    /// The mapping rules from `--mapping`, defaults merged in as the file says.
    pub mapping: Option<Vec<Rule>>,
    pub transparent: bool,
    /// Publish to Syphon from the start (`--syphon`).
    pub syphon: bool,
    pub syphon_name: String,
    /// Publish at this size instead of the window's (`--syphon-size`).
    pub syphon_size: Option<(u32, u32)>,
    /// Publish to Spout2 from the start (`--spout`).
    pub spout: bool,
    pub spout_name: String,
    /// Publish at this size instead of the window's (`--spout-size`).
    pub spout_size: Option<(u32, u32)>,
}

/// Parses the arguments after the program name. Anything wrong is an error:
/// nothing falls back to a default silently.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut project = None;
    let mut vmc_port = None;
    let mut mapping = None;
    let mut transparent = false;
    let mut syphon = false;
    let mut syphon_name = None;
    let mut syphon_size = None;
    let mut spout = false;
    let mut spout_name = None;
    let mut spout_size = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--vmc-port" => {
                let value = args.next().ok_or("--vmc-port needs a port number")?;
                let port: u16 = value
                    .parse()
                    .map_err(|_| format!("--vmc-port: '{value}' is not a port number"))?;
                if port == 0 {
                    return Err("--vmc-port: port must be 1-65535".into());
                }
                vmc_port = Some(port);
            }
            "--mapping" => {
                let path = args.next().ok_or("--mapping needs a JSON file")?;
                let json = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                let overrides = Overrides::from_json(&json).map_err(|e| format!("{path}: {e}"))?;
                mapping = Some(overrides.resolve());
            }
            "--transparent" => transparent = true,
            "--syphon" => syphon = true,
            "--syphon-name" => {
                let name = args.next().ok_or("--syphon-name needs a name")?;
                if name.is_empty() {
                    return Err("--syphon-name: the name is empty".into());
                }
                syphon_name = Some(name.clone());
            }
            "--syphon-size" => {
                let value = args.next().ok_or("--syphon-size needs <w>x<h>")?;
                syphon_size = Some(parse_size(value).map_err(|e| format!("--syphon-size: {e}"))?);
            }
            "--spout" => spout = true,
            "--spout-name" => {
                let name = args.next().ok_or("--spout-name needs a name")?;
                if name.is_empty() {
                    return Err("--spout-name: the name is empty".into());
                }
                spout_name = Some(name.clone());
            }
            "--spout-size" => {
                let value = args.next().ok_or("--spout-size needs <w>x<h>")?;
                spout_size = Some(parse_size(value).map_err(|e| format!("--spout-size: {e}"))?);
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option '{flag}'")),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }
    if syphon && !SYPHON_AVAILABLE {
        return Err("--syphon: this build has no Syphon support (it needs macOS and the 'syphon' cargo feature: cargo build -p tsukumoe-host --features syphon)".into());
    }
    if !syphon && (syphon_name.is_some() || syphon_size.is_some()) {
        return Err("--syphon-name and --syphon-size need --syphon".into());
    }
    if spout && !SPOUT_AVAILABLE {
        return Err("--spout: this build has no Spout2 support (it needs Windows and the 'spout' cargo feature: cargo build -p tsukumoe-host --features spout)".into());
    }
    if !spout && (spout_name.is_some() || spout_size.is_some()) {
        return Err("--spout-name and --spout-size need --spout".into());
    }
    if syphon && spout {
        return Err("--syphon and --spout cannot be used together".into());
    }
    Ok(Options {
        project: project.ok_or("missing project")?,
        vmc_port,
        mapping,
        transparent,
        syphon,
        syphon_name: syphon_name.unwrap_or_else(|| DEFAULT_SYPHON_NAME.to_owned()),
        syphon_size,
        spout,
        spout_name: spout_name.unwrap_or_else(|| DEFAULT_SPOUT_NAME.to_owned()),
        spout_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Options, String> {
        parse(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    fn mapping_file(tag: &str, json: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("tsukumoe-opt-{tag}-{}.json", std::process::id()));
        fs::write(&path, json).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_bare_project_gives_the_defaults() {
        let o = parse_args(&["model"]).unwrap();
        assert_eq!(o.project, PathBuf::from("model"));
        assert_eq!((o.vmc_port, o.mapping, o.transparent), (None, None, false));
        assert_eq!(
            (o.syphon, o.syphon_name.as_str(), o.syphon_size),
            (false, "Tsukumoe", None)
        );
    }

    #[test]
    fn syphon_options_parse_when_the_build_supports_it() {
        let args = [
            "m",
            "--syphon",
            "--syphon-name",
            "Live",
            "--syphon-size",
            "1280x720",
        ];
        if SYPHON_AVAILABLE {
            let o = parse_args(&args).unwrap();
            assert_eq!(
                (o.syphon, o.syphon_name.as_str(), o.syphon_size),
                (true, "Live", Some((1280, 720)))
            );
            assert!(parse_args(&["--syphon", "m"]).unwrap().syphon);
        } else {
            let e = parse_args(&args).unwrap_err();
            assert!(e.contains("--syphon") && e.contains("feature"), "{e}");
        }
    }

    #[test]
    fn syphon_without_the_build_support_or_the_flag_is_an_error() {
        // Without --syphon the other two are never ignored silently.
        assert!(parse_args(&["m", "--syphon-name", "x"]).is_err());
        assert!(parse_args(&["m", "--syphon-size", "10x10"]).is_err());
        if !SYPHON_AVAILABLE {
            assert!(parse_args(&["m", "--syphon"]).is_err());
        }
    }

    #[test]
    fn spout_options_parse_when_the_build_supports_it() {
        let args = [
            "m",
            "--spout",
            "--spout-name",
            "Live",
            "--spout-size",
            "1280x720",
        ];
        if SPOUT_AVAILABLE {
            let o = parse_args(&args).unwrap();
            assert_eq!(
                (o.spout, o.spout_name.as_str(), o.spout_size),
                (true, "Live", Some((1280, 720)))
            );
        } else {
            let e = parse_args(&args).unwrap_err();
            assert!(e.contains("--spout") && e.contains("feature"), "{e}");
            assert!(parse_args(&["m", "--spout"]).is_err());
        }
    }

    #[test]
    fn spout_name_and_size_need_the_flag() {
        assert!(parse_args(&["m", "--spout-name", "x"]).is_err());
        assert!(parse_args(&["m", "--spout-size", "10x10"]).is_err());
        let o = parse_args(&["m"]).unwrap();
        assert_eq!(
            (o.spout, o.spout_name.as_str(), o.spout_size),
            (false, "Tsukumoe", None)
        );
    }

    #[test]
    fn a_bad_syphon_size_or_name_is_an_error() {
        for args in [
            &["m", "--syphon", "--syphon-size", "0x10"][..],
            &["m", "--syphon", "--syphon-size", "wide"],
            &["m", "--syphon", "--syphon-size"],
            &["m", "--syphon", "--syphon-name"],
            &["m", "--syphon", "--syphon-name", ""],
        ] {
            assert!(parse_args(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn options_in_any_order() {
        let path = mapping_file(
            "ok",
            r#"{"rules":[{"output":"P","terms":[{"input":"jawOpen","weight":1.0}]}]}"#,
        );
        let o = parse_args(&[
            "--transparent",
            "--vmc-port",
            "39539",
            "m",
            "--mapping",
            &path,
        ])
        .unwrap();
        assert_eq!((o.vmc_port, o.transparent), (Some(39539), true));
        assert!(o.mapping.unwrap().iter().any(|r| r.output == "P"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn bad_input_is_an_error() {
        let broken = mapping_file("bad", "{");
        for args in [
            &[][..],
            &["m", "--nope"],
            &["m", "extra"],
            &["m", "--vmc-port"],
            &["m", "--vmc-port", "abc"],
            &["m", "--vmc-port", "70000"],
            &["m", "--vmc-port", "0"],
            &["m", "--mapping"],
            &["m", "--mapping", "/no/such/mapping.json"],
            &["m", "--mapping", &broken],
        ] {
            assert!(parse_args(args).is_err(), "{args:?}");
        }
        fs::remove_file(broken).unwrap();
    }
}
