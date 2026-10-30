// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe convert | validate | render | diff`. See
//! `docs/implementation/M1-core/cli.md`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::json;
use tsukumoe_core::eval::Frame;
use tsukumoe_format::{Project, read_model};
use tsukumoe_gpu::{Headless, Image, Texture};

const USAGE: &str = "usage:
  tsukumoe convert <in> <out>
  tsukumoe validate <project>
  tsukumoe render <project> -o <png> [--param NAME=VALUE]... [--size WxH] [--view X0,Y0,X1,Y1]
  tsukumoe diff <a.png> <b.png> [--tolerance N] [-o <png>]";

/// A failure: `Usage` exits 2, `Failed` exits 1.
enum Fail {
    Usage(String),
    Failed(String),
}

impl<E: std::fmt::Display> From<E> for Fail {
    fn from(e: E) -> Self {
        Fail::Failed(e.to_string())
    }
}

fn usage<T>(msg: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Usage(msg.into()))
}

/// Positional arguments and `--name value` options, checked against `allowed`.
struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(args: &[String], allowed: &[&str]) -> Result<Self, Fail> {
        let (mut positional, mut options) = (Vec::new(), Vec::new());
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a.starts_with('-') && a.len() > 1 {
                if !allowed.contains(&a.as_str()) {
                    return usage(format!("unknown option {a}"));
                }
                let Some(v) = it.next() else {
                    return usage(format!("{a} needs a value"));
                };
                options.push((a.clone(), v.clone()));
            } else {
                positional.push(a.clone());
            }
        }
        Ok(Self {
            positional,
            options,
        })
    }

    fn positional<const N: usize>(&self) -> Result<[&str; N], Fail> {
        match self
            .positional
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .try_into()
        {
            Ok(p) => Ok(p),
            Err(_) => usage(format!("expected {N} argument(s)")),
        }
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.options
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn last<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.all(name).last()
    }
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, Fail> {
    text.trim()
        .parse()
        .or_else(|_| usage(format!("{what}: {text:?} is not a number")))
}

fn load(path: &str) -> Result<Project, Fail> {
    let p = Path::new(path);
    if p.is_dir() {
        Ok(Project::read_dir(p)?)
    } else {
        let bytes = fs::read(p).map_err(|e| Fail::Failed(format!("{path}: {e}")))?;
        Ok(Project::read_zip(&bytes)?)
    }
}

fn write(path: &str, bytes: &[u8]) -> Result<(), Fail> {
    fs::write(path, bytes).map_err(|e| Fail::Failed(format!("{path}: {e}")))
}

fn convert(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &[])?;
    let [input, output] = args.positional()?;
    let project = load(input)?;
    if output.ends_with(".zip") || output.ends_with(".tsukumoe") {
        write(output, &project.write_zip()?)?;
    } else {
        project.write_dir(Path::new(output))?;
    }
    Ok(0)
}

fn validate(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &[])?;
    let [input] = args.positional()?;
    let result = load(input).and_then(|project| {
        let model = read_model(&project)?;
        let compiled = model
            .compile()
            .map_err(|e| Fail::Failed(format!("{e:?}")))?;
        Ok((model.parameters().len(), compiled.meshes.len()))
    });
    let (report, code) = match result {
        Ok((parameters, meshes)) => (
            json!({ "ok": true, "parameters": parameters, "meshes": meshes }),
            0,
        ),
        Err(Fail::Usage(m) | Fail::Failed(m)) => (json!({ "ok": false, "errors": [m] }), 1),
    };
    println!("{report}");
    Ok(code)
}

fn render(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &["-o", "--param", "--size", "--view"])?;
    let [input] = args.positional()?;
    let Some(output) = args.last("-o") else {
        return usage("render needs -o <png>");
    };
    let project = load(input)?;
    let compiled = read_model(&project)?
        .compile()
        .map_err(|e| Fail::Failed(format!("{e:?}")))?;
    let mut frame = Frame::new(&compiled);
    for p in args.all("--param") {
        let Some((name, value)) = p.split_once('=') else {
            return usage(format!("--param expects NAME=VALUE, got {p:?}"));
        };
        let value: f32 = number(value, "--param")?;
        let Some(index) = compiled.parameter_index(name) else {
            return Err(Fail::Failed(format!("no parameter named {name:?}")));
        };
        frame.set_parameter(index, value);
    }
    frame.evaluate(&compiled);
    let list = frame.draw_list(&compiled);

    let view = match args.last("--view") {
        Some(v) => {
            let parts: Vec<f32> = v
                .split(',')
                .map(|n| number(n, "--view"))
                .collect::<Result<_, _>>()?;
            let Ok(view) = <[f32; 4]>::try_from(parts) else {
                return usage("--view expects X0,Y0,X1,Y1");
            };
            view
        }
        None => bounds(list.positions())?,
    };
    if !(view[2] > view[0] && view[3] > view[1]) {
        return usage("the view must have a positive width and height");
    }
    let (width, height) = match args.last("--size") {
        Some(s) => match s.split_once('x') {
            Some((w, h)) => (number(w, "--size")?, number(h, "--size")?),
            None => return usage("--size expects WxH"),
        },
        None => {
            let (w, h) = (view[2] - view[0], view[3] - view[1]);
            let scale = 512.0 / w.max(h);
            (
                ((w * scale).round() as u32).max(1),
                ((h * scale).round() as u32).max(1),
            )
        }
    };
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return usage("--size must be between 1x1 and 16384x16384");
    }

    let textures = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(path, bytes)| {
            Texture::from_png(bytes).map_err(|e| Fail::Failed(format!("{path}: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut gpu = Headless::new()?;
    gpu.set_textures(&textures)?;
    let image = gpu.render(&list, view, width, height)?;
    write(output, &image.to_png())?;
    Ok(0)
}

/// The bounding box of `x, y` pairs.
fn bounds(positions: &[f32]) -> Result<[f32; 4], Fail> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in positions.as_chunks::<2>().0 {
        b = [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ];
    }
    if positions.is_empty() {
        return Err(Fail::Failed(
            "the model has nothing to draw; pass --view".into(),
        ));
    }
    Ok(b)
}

fn diff(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &["--tolerance", "-o"])?;
    let [a, b] = args.positional()?;
    let tolerance: u8 = match args.last("--tolerance") {
        Some(t) => number(t, "--tolerance")?,
        None => 0,
    };
    let read = |path: &str| -> Result<Image, Fail> {
        let bytes = fs::read(path).map_err(|e| Fail::Failed(format!("{path}: {e}")))?;
        Image::from_png(&bytes).map_err(|e| Fail::Failed(format!("{path}: {e}")))
    };
    let (a, b) = (read(a)?, read(b)?);
    let Some((d, image)) = a.diff(&b, tolerance) else {
        return Err(Fail::Failed(format!(
            "sizes differ: {}x{} and {}x{}",
            a.width, a.height, b.width, b.height
        )));
    };
    if let Some(out) = args.last("-o") {
        write(out, &image.to_png())?;
    }
    let same = d.differing == 0;
    println!(
        "{}",
        json!({ "same": same, "max": d.max, "differing": d.differing, "pixels": d.pixels, "tolerance": tolerance })
    );
    Ok(u8::from(!same))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) => match cmd.as_str() {
            "convert" => convert(rest),
            "validate" => validate(rest),
            "render" => render(rest),
            "diff" => diff(rest),
            other => usage(format!("unknown command {other:?}")),
        },
        None => usage("no command"),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(Fail::Usage(m)) => {
            eprintln!("error: {m}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Fail::Failed(m)) => {
            eprintln!("error: {m}");
            ExitCode::from(1)
        }
    }
}
