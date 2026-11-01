// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The edit commands as tools: names, descriptions and argument schemas come
//! from [`Command`]'s JSON Schema, so `tsukumoe commands`, the MCP `tools/list`
//! and `docs/reference/commands.md` cannot drift from the type.

use std::fmt::Write;

use schemars::schema_for;
use serde_json::{Value, json};
use tsukumoe_core::command::Command;

/// One tool per `Command` variant, as `{name, description, inputSchema}`.
/// The name is the variant's `type`; the schema is that variant's.
pub fn command_tools() -> Vec<Value> {
    let mut root = serde_json::to_value(schema_for!(Command)).expect("schema is JSON");
    let variants = root["oneOf"].take();
    let variants = variants.as_array().expect("Command is an enum");
    variants
        .iter()
        .map(|variant| {
            let mut schema = variant.clone();
            let name = schema["properties"]["type"]["const"]
                .as_str()
                .expect("variants are tagged by type")
                .to_owned();
            let description = schema
                .as_object_mut()
                .and_then(|o| o.remove("description"))
                .unwrap_or_default();
            // Batch refers to the whole enum (`$ref: "#"`), which a lone
            // variant schema does not contain.
            if retarget(&mut schema) {
                schema["$defs"] = json!({ "Command": command_schema() });
            }
            json!({ "name": name, "description": description, "inputSchema": schema })
        })
        .collect()
}

/// The schema of the whole `Command` enum.
pub fn command_schema() -> Value {
    let mut schema = serde_json::to_value(schema_for!(Command)).expect("schema is JSON");
    schema
        .as_object_mut()
        .expect("schema is an object")
        .remove("$schema");
    schema
}

/// Points `$ref: "#"` at `#/$defs/Command`. True if there was one.
fn retarget(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            if map.get("$ref").and_then(Value::as_str) == Some("#") {
                map.insert("$ref".into(), "#/$defs/Command".into());
                return true;
            }
            map.values_mut().fold(false, |any, v| retarget(v) | any)
        }
        Value::Array(items) => items.iter_mut().fold(false, |any, v| retarget(v) | any),
        _ => false,
    }
}

/// Tools of an MCP session that are not commands (they take no arguments).
pub fn session_tools() -> Vec<Value> {
    let none = json!({ "type": "object", "properties": {} });
    [
        ("undo", "Reverts the last applied command or batch."),
        ("redo", "Re-applies the last undone command."),
        (
            "validate",
            "Checks the model; returns {ok, errors} (see Model::validate).",
        ),
        (
            "get_model",
            "Summarizes the model: names and counts of its elements.",
        ),
        (
            "save",
            "Writes the model to the project; nothing is saved until this is called.",
        ),
    ]
    .into_iter()
    .map(|(name, description)| json!({ "name": name, "description": description, "inputSchema": none }))
    .collect()
}

/// A command of each variant (a test checks that none is missing).
pub fn examples() -> Vec<Command> {
    let triangle = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let add_parameter = Command::AddParameter {
        name: "ParamAngleX".into(),
        min: -30.0,
        max: 30.0,
        default: 0.0,
        repeat: false,
    };
    let add_part = Command::AddPart {
        name: "Face".into(),
        parent: None,
    };
    vec![
        add_parameter.clone(),
        Command::RemoveParameter {
            name: "ParamAngleX".into(),
        },
        Command::SetParameterRange {
            name: "ParamAngleX".into(),
            min: -45.0,
            max: 45.0,
            default: 0.0,
        },
        add_part.clone(),
        Command::RemovePart {
            name: "Face".into(),
        },
        Command::SetPartOpacity {
            name: "Face".into(),
            opacity: 0.5,
        },
        Command::AddMesh {
            name: "Skin".into(),
            part: "Face".into(),
            positions: triangle.clone(),
            uvs: triangle.clone(),
            indices: vec![0, 1, 2],
        },
        Command::RemoveMesh {
            name: "Skin".into(),
        },
        Command::SetMeshPositions {
            name: "Skin".into(),
            positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
        },
        Command::Batch {
            commands: vec![add_parameter, add_part],
        },
    ]
}

/// The `type` of a command. The exhaustive `match` makes a new variant fail
/// to compile until it is handled here, which is the cue to add an example.
pub fn tag(command: &Command) -> &'static str {
    match command {
        Command::AddParameter { .. } => "add_parameter",
        Command::RemoveParameter { .. } => "remove_parameter",
        Command::SetParameterRange { .. } => "set_parameter_range",
        Command::AddPart { .. } => "add_part",
        Command::RemovePart { .. } => "remove_part",
        Command::SetPartOpacity { .. } => "set_part_opacity",
        Command::AddMesh { .. } => "add_mesh",
        Command::RemoveMesh { .. } => "remove_mesh",
        Command::SetMeshPositions { .. } => "set_mesh_positions",
        Command::Batch { .. } => "batch",
    }
}

/// The API reference in Markdown: one section per tool with its description,
/// argument table and (for commands) a JSON example.
pub fn reference() -> String {
    let mut out = String::from(
        "# Command reference\n\n\
         <!-- Generated by `tsukumoe docs commands` from the `Command` type. Do not edit. -->\n\n\
         Every editing command is a JSON object whose `type` names it. The same commands run \
         through `tsukumoe edit <project> <command.json|->` and as MCP tools of \
         `tsukumoe mcp <project>` (the tool name is the `type`, the arguments are the other \
         fields). Elements are named by their persistent name. `tsukumoe commands` prints the \
         argument schemas as JSON.\n\n## Commands\n",
    );
    let examples = examples();
    for tool in command_tools() {
        let name = tool["name"].as_str().unwrap_or_default();
        let _ = write!(
            out,
            "\n### `{name}`\n\n{}\n\n",
            tool["description"].as_str().unwrap_or_default()
        );
        let schema = &tool["inputSchema"];
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        out.push_str("| Field | Type | Required | Default |\n|---|---|---|---|\n");
        for (field, property) in schema["properties"].as_object().into_iter().flatten() {
            let default = property
                .get("default")
                .map_or(String::new(), |d| format!("`{d}`"));
            let _ = writeln!(
                out,
                "| `{field}` | {} | {} | {default} |",
                type_name(property),
                if required.contains(&field.as_str()) {
                    "yes"
                } else {
                    "no"
                },
            );
        }
        if let Some(example) = examples.iter().find(|c| tag(c) == name) {
            let _ = write!(
                out,
                "\n```json\n{}\n```\n",
                serde_json::to_string_pretty(example).unwrap_or_default()
            );
        }
    }
    out.push_str("\n## Session tools (MCP only)\n\nThese take no arguments.\n\n");
    for tool in session_tools() {
        let _ = writeln!(
            out,
            "- `{}`: {}",
            tool["name"].as_str().unwrap_or_default(),
            tool["description"].as_str().unwrap_or_default()
        );
    }
    out
}

/// A short type name for an argument table.
fn type_name(schema: &Value) -> String {
    if schema.get("$ref").is_some() {
        return "Command".into();
    }
    if let Some(constant) = schema.get("const") {
        return format!("`{constant}`");
    }
    match &schema["type"] {
        Value::String(t) if t == "array" => {
            let size = match (schema.get("minItems"), schema.get("maxItems")) {
                (Some(min), Some(max)) if min == max => format!("[{min}]"),
                _ => "[]".into(),
            };
            format!("{}{size}", type_name(&schema["items"]))
        }
        Value::String(t) if t == "integer" => "integer".into(),
        Value::String(t) => t.clone(),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "any".into(),
    }
}
