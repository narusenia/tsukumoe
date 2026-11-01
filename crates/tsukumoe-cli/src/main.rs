// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe convert | generate | validate | inspect | render | diff | parity | lipsync | commands | docs`.
//! See `docs/implementation/M1-core/cli.md` and `M4-vtuber/lipsync.md`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::json;
use tsukumoe_ai::LipsyncConfig;
use tsukumoe_ai::wav::{self, MAX_WAV_BYTES};
use tsukumoe_cli::{commands, generate, lipsync, parity};
use tsukumoe_core::eval::Frame;
use tsukumoe_core::tmod::{self, AlignedBuf, PlaybackData, Tmod};
use tsukumoe_format::{Project, read_model};
use tsukumoe_gpu::{Headless, Image, Texture};

const USAGE: &str = "usage:
  tsukumoe convert <in> <out>      (out: a directory, .zip / .tsukumoe, or .tmod)
  tsukumoe generate <parts.json> <out>
  tsukumoe validate <project>
  tsukumoe inspect <file.tmod>
  tsukumoe render <project> -o <png> [--param NAME=VALUE]... [--size WxH] [--view X0,Y0,X1,Y1]
  tsukumoe diff <a.png> <b.png> [--tolerance N] [-o <png>]
  tsukumoe parity <a> <b> [--width N] [--pixel-tolerance N] [--number-tolerance X]
      compares <a>.rgba / <a>.json with <b>.rgba / <b>.json (premultiplied RGBA8, draw-list JSON)
  tsukumoe lipsync <wav> -o <motion.json> [--fps N] [--tolerance X] [--config <json>] [--name NAME]
      mouth motion (ParamMouthOpenY, ParamMouthForm) from speech; the output is a
      project motion file (motions/<name>.json)
  tsukumoe commands
      the edit commands with their argument schemas (JSON, the MCP tool list)
  tsukumoe docs commands
      the command API reference (Markdown)";

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
    save(&load(input)?, output)?;
    Ok(0)
}

/// The runtime format of a project: the compiled model, its motions,
/// expressions and pose, and its textures (`textures/*.png`, in path order).
fn tmod_bytes(project: &Project) -> Result<Vec<u8>, Fail> {
    let model = read_model(project)?;
    let compiled = model
        .compile()
        .map_err(|e| Fail::Failed(format!("{e:?}")))?;
    let playback = PlaybackData::from_model(&model)?;
    let textures: Vec<&[u8]> = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(_, bytes)| bytes.as_slice())
        .collect();
    Ok(tmod::write_with(&compiled, &playback, &textures)?)
}

/// A `.tmod` path gets the runtime format, a `.zip` or `.tsukumoe` path an
/// archive, any other a directory.
fn save(project: &Project, output: &str) -> Result<(), Fail> {
    if output.ends_with(".tmod") {
        write(output, &tmod_bytes(project)?)
    } else if output.ends_with(".zip") || output.ends_with(".tsukumoe") {
        write(output, &project.write_zip()?)
    } else {
        Ok(project.write_dir(Path::new(output))?)
    }
}

fn generate(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &[])?;
    let [table, output] = args.positional()?;
    let bytes = fs::read(table).map_err(|e| Fail::Failed(format!("{table}: {e}")))?;
    let table: generate::Table =
        serde_json::from_slice(&bytes).map_err(|e| Fail::Failed(format!("{table}: {e}")))?;
    let (project, model) = generate::project(&table).map_err(Fail::Failed)?;
    save(&project, output)?;
    let vertices: usize = model
        .meshes()
        .iter()
        .map(|(_, _, m)| m.positions.len())
        .sum();
    println!(
        "{}",
        json!({
            "meshes": model.meshes().len(),
            "vertices": vertices,
            "parameters": model.parameters().len(),
            "deformers": model.warps().len() + model.rotations().len(),
            "physics": model.physics().chains.len() + model.physics().pendulums.len(),
        })
    );
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

/// Summarizes a `.tmod`: version, element counts (motions, expressions and
/// pose groups included) and the section table.
fn inspect(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &[])?;
    let [input] = args.positional()?;
    let buf = AlignedBuf::read_file(input)?;
    let t = Tmod::parse(&buf)?;
    let m = &t.model;
    let sections: Vec<_> = t
        .sections
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": tmod::section_name(s.id).unwrap_or("unknown"),
                "required": s.required,
                "offset": s.offset,
                "length": s.length,
                "count": s.count,
            })
        })
        .collect();
    println!(
        "{}",
        json!({
            "version": format!("{}.{}", t.major, t.minor),
            "bytes": buf.len(),
            "parameters": m.parameters.len(),
            "bindings": m.bindings.len(),
            "drivers": m.drivers.len(),
            "parts": m.part_parents.len(),
            "meshes": m.meshes.len(),
            "vertices": m.vertex_count(),
            "warps": m.warps.len(),
            "rotations": m.rotations.len(),
            "bones": m.bones.len(),
            "physics": m.physics.len(),
            "glues": m.glues.len(),
            "blend_shapes": m.blends.len(),
            "motions": t.playback.motions.len(),
            "curves": t.playback.curves.len(),
            "expressions": t.playback.expressions.len(),
            "pose_groups": t.playback.pose_groups.len(),
            "textures": t.texture_count(),
            "sections": sections,
        })
    );
    Ok(0)
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

/// Compares two parity captures (`<prefix>.rgba` and `<prefix>.json`). A file
/// both sides lack is skipped; one that only a side has fails. Exits 1 when
/// a tolerance is exceeded.
fn parity(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(
        args,
        &["--width", "--pixel-tolerance", "--number-tolerance"],
    )?;
    let [a, b] = args.positional()?;
    let width: Option<usize> = args
        .last("--width")
        .map(|w| number(w, "--width"))
        .transpose()?;
    let pixel_tolerance: u8 = match args.last("--pixel-tolerance") {
        Some(t) => number(t, "--pixel-tolerance")?,
        None => 2,
    };
    let number_tolerance: f32 = match args.last("--number-tolerance") {
        Some(t) => number(t, "--number-tolerance")?,
        None => 1e-4,
    };
    let read = |prefix: &str, ext: &str| -> Result<Option<Vec<u8>>, Fail> {
        let path = format!("{prefix}.{ext}");
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Fail::Failed(format!("{path}: {e}"))),
        }
    };
    let mut report = serde_json::Map::new();
    let mut ok = true;
    for ext in ["rgba", "json"] {
        let (x, y) = (read(a, ext)?, read(b, ext)?);
        let (x, y) = match (x, y) {
            (Some(x), Some(y)) => (x, y),
            (None, None) => continue,
            _ => return Err(Fail::Failed(format!("only one side has the .{ext} file"))),
        };
        if ext == "rgba" {
            let r = parity::compare_images(&x, &y, pixel_tolerance, width).map_err(Fail::Failed)?;
            ok &= r.ok;
            report.insert("image".into(), serde_json::to_value(r)?);
        } else {
            let parse = |bytes: &[u8], which: &str| {
                serde_json::from_slice::<parity::DrawListFile>(bytes)
                    .map_err(|e| Fail::Failed(format!("{which}.json: {e}")))
            };
            let r = parity::compare_draw_lists(&parse(&x, a)?, &parse(&y, b)?, number_tolerance);
            ok &= r.ok;
            report.insert("draw_list".into(), serde_json::to_value(r)?);
        }
    }
    if report.is_empty() {
        return Err(Fail::Failed(
            "neither .rgba nor .json found on either side".into(),
        ));
    }
    report.insert("ok".into(), ok.into());
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(u8::from(!ok))
}

/// Speech audio to a mouth motion file. Prints a summary.
fn lipsync(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &["-o", "--fps", "--tolerance", "--config", "--name"])?;
    let [input] = args.positional()?;
    let Some(output) = args.last("-o") else {
        return usage("lipsync needs -o <motion.json>");
    };
    let fps: f32 = match args.last("--fps") {
        Some(f) => number(f, "--fps")?,
        None => lipsync::DEFAULT_FPS,
    };
    let tolerance: f32 = match args.last("--tolerance") {
        Some(t) => number(t, "--tolerance")?,
        None => lipsync::DEFAULT_TOLERANCE,
    };
    let config = match args.last("--config") {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|e| Fail::Failed(format!("{path}: {e}")))?;
            LipsyncConfig::from_json(&text).map_err(|e| Fail::Failed(format!("{path}: {e}")))?
        }
        None => LipsyncConfig::default(),
    };
    let name = match args.last("--name") {
        Some(n) => n.to_owned(),
        None => Path::new(output)
            .file_stem()
            .map_or_else(|| "lipsync".into(), |s| s.to_string_lossy().into_owned()),
    };
    let fail = |e: String| Fail::Failed(format!("{input}: {e}"));
    let size = fs::metadata(input).map_err(|e| fail(e.to_string()))?.len();
    if size > MAX_WAV_BYTES as u64 {
        return Err(fail("file is too large".into()));
    }
    let bytes = fs::read(input).map_err(|e| fail(e.to_string()))?;
    let audio = wav::parse(&bytes).map_err(|e| fail(e.to_string()))?;
    let motion = lipsync::motion(&audio, config, fps, tolerance, &name).map_err(fail)?;
    write(output, &tsukumoe_format::to_json(&motion)?)?;
    println!(
        "{}",
        json!({
            "name": motion.name,
            "duration": motion.duration,
            "fps": motion.fps,
            "curves": motion.curves.len(),
            "points": motion.curves.iter().map(|c| c.segments.len() + 1).sum::<usize>(),
        })
    );
    Ok(0)
}

/// The edit commands as tools: name, description and argument schema.
fn commands(args: &[String]) -> Result<u8, Fail> {
    Args::parse(args, &[])?.positional::<0>()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&commands::command_tools())?
    );
    Ok(0)
}

fn docs(args: &[String]) -> Result<u8, Fail> {
    let args = Args::parse(args, &[])?;
    let ["commands"] = args.positional()? else {
        return usage("docs expects: commands");
    };
    print!("{}", commands::reference());
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) => match cmd.as_str() {
            "convert" => convert(rest),
            "generate" => generate(rest),
            "validate" => validate(rest),
            "inspect" => inspect(rest),
            "render" => render(rest),
            "diff" => diff(rest),
            "parity" => parity(rest),
            "lipsync" => lipsync(rest),
            "commands" => commands(rest),
            "docs" => docs(rest),
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
