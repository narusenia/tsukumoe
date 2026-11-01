// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command line: `tsukumoe-host <project> [--vmc-port <n>] [--mapping <json>] [--transparent]`.

use std::fs;
use std::path::PathBuf;

use tsukumoe_tracking::{Overrides, Rule};

pub const USAGE: &str = "usage: tsukumoe-host <project directory or archive> [--vmc-port <n>] [--mapping <json>] [--transparent]";

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub project: PathBuf,
    /// Tracking starts on this UDP port when set.
    pub vmc_port: Option<u16>,
    /// The mapping rules from `--mapping`, defaults merged in as the file says.
    pub mapping: Option<Vec<Rule>>,
    pub transparent: bool,
}

/// Parses the arguments after the program name. Anything wrong is an error:
/// nothing falls back to a default silently.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut project = None;
    let mut vmc_port = None;
    let mut mapping = None;
    let mut transparent = false;
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
            flag if flag.starts_with("--") => return Err(format!("unknown option '{flag}'")),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }
    Ok(Options {
        project: project.ok_or("missing project")?,
        vmc_port,
        mapping,
        transparent,
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
