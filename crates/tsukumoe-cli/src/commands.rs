// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The edit commands as tools: names, descriptions and argument schemas come
//! from [`Command`]'s JSON Schema, so `tsukumoe commands`, the MCP `tools/list`
//! and `docs/reference/commands.md` cannot drift from the type.

use std::fmt::Write;

use schemars::schema_for;
use serde_json::{Value, json};
use tsukumoe_core::command::{Command, ElementRef, RotationBase};
use tsukumoe_core::rig::{BlendMode, GlueNode};

/// One tool per `Command` variant, as `{name, description, inputSchema}`.
/// The name is the variant's `type`; the schema is that variant's.
pub fn command_tools() -> Vec<Value> {
    let mut root = serde_json::to_value(schema_for!(Command)).expect("schema is JSON");
    let variants = root["oneOf"].take();
    let variants = variants.as_array().expect("Command is an enum");
    // Definitions the variants share (a type used by two variants is not inlined).
    let defs = root["$defs"].as_object().cloned().unwrap_or_default();
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
            if schema.to_string().contains("#/$defs/") {
                let own = schema["$defs"].as_object().cloned().unwrap_or_default();
                let all: serde_json::Map<String, Value> =
                    defs.clone().into_iter().chain(own).collect();
                schema["$defs"] = Value::Object(all);
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
        Command::SetPartVisible {
            name: "Face".into(),
            visible: false,
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
        Command::AddGlue {
            name: "Seam".into(),
            mesh_a: "Skin".into(),
            mesh_b: "Clip".into(),
            nodes: vec![GlueNode {
                index_a: 0,
                index_b: 1,
                weight: 0.5,
            }],
            intensity: 1.0,
        },
        Command::RemoveGlue {
            name: "Seam".into(),
        },
        Command::SetGlueIntensity {
            name: "Seam".into(),
            intensity: 0.5,
        },
        Command::AddWarpDeformer {
            name: "FaceWarp".into(),
            part: "Face".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        },
        Command::AddRotationDeformer {
            name: "HeadRotation".into(),
            part: "Face".into(),
            parent: Some("FaceWarp".into()),
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 0.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        },
        Command::RemoveDeformer {
            name: "HeadRotation".into(),
        },
        Command::SetDeformerParent {
            name: "HeadRotation".into(),
            parent: Some("FaceWarp".into()),
        },
        Command::SetMeshParent {
            name: "Skin".into(),
            parent: Some("FaceWarp".into()),
        },
        Command::SetWarpBasePoints {
            name: "FaceWarp".into(),
            points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
        },
        Command::ClearWarpRestPoints {
            name: "FaceWarp".into(),
        },
        Command::SetWarpBezier {
            name: "FaceWarp".into(),
            bezier: true,
        },
        Command::SetParameterRepeat {
            name: "ParamAngleX".into(),
            repeat: true,
        },
        Command::SetParameterGroup {
            name: "ParamAngleX".into(),
            group: Some("Head".into()),
        },
        Command::MoveParameter {
            name: "ParamAngleX".into(),
            to: 0,
        },
        Command::MoveParameterKeyforms {
            from: "ParamAngleX".into(),
            to: "ParamAngleY".into(),
        },
        Command::BindParameter {
            parameter: "ParamAngleX".into(),
            elements: vec![
                ElementRef::Mesh {
                    name: "Skin".into(),
                },
                ElementRef::Deformer {
                    name: "FaceWarp".into(),
                },
            ],
        },
        Command::AddParameterKey {
            parameter: "ParamAngleX".into(),
            value: 10.0,
            element: ElementRef::Mesh {
                name: "Skin".into(),
            },
        },
        Command::RemoveParameterKey {
            parameter: "ParamAngleX".into(),
            value: 10.0,
            element: ElementRef::Mesh {
                name: "Skin".into(),
            },
        },
        Command::SetAxisOrder {
            element: ElementRef::Mesh {
                name: "Skin".into(),
            },
            parameters: vec!["ParamAngleY".into(), "ParamAngleX".into()],
        },
        Command::UnbindParameter {
            parameter: "ParamAngleX".into(),
            elements: vec![ElementRef::Mesh {
                name: "Skin".into(),
            }],
        },
        Command::RenameParameter {
            name: "ParamAngleX".into(),
            new_name: "AngleX".into(),
        },
        Command::SetMeshKeyform {
            name: "Skin".into(),
            key: vec![1],
            positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
            opacity: 1.0,
            draw_order: 500.0,
        },
        Command::SetWarpKeyform {
            name: "FaceWarp".into(),
            key: vec![1],
            points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
            opacity: 1.0,
        },
        Command::SetGlueKeyform {
            name: "Seam".into(),
            key: vec![2],
            intensity: 0.5,
        },
        Command::SetRotationKeyform {
            name: "HeadRotation".into(),
            key: vec![1],
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 15.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
            opacity: 1.0,
        },
        Command::SetRotationBase {
            name: "HeadRotation".into(),
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 15.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        },
        Command::RenamePart {
            name: "Face".into(),
            new_name: "Head".into(),
        },
        Command::RenameMesh {
            name: "Skin".into(),
            new_name: "Scalp".into(),
        },
        Command::RenameWarp {
            name: "FaceWarp".into(),
            new_name: "HeadWarp".into(),
        },
        Command::RenameRotation {
            name: "HeadRotation".into(),
            new_name: "Turn".into(),
        },
        Command::RenameGlue {
            name: "Seam".into(),
            new_name: "Stitch".into(),
        },
        Command::SetMeshPart {
            name: "Skin".into(),
            part: "Head".into(),
        },
        Command::SetDeformerPart {
            name: "FaceWarp".into(),
            part: "Head".into(),
        },
        Command::SetPartParent {
            name: "Face".into(),
            parent: Some("Body".into()),
        },
        Command::SetMeshGeometry {
            name: "Skin".into(),
            positions: triangle.clone(),
            uvs: triangle.clone(),
            indices: vec![0, 1, 2],
        },
        Command::SetMeshTexture {
            name: "Skin".into(),
            texture: 1,
        },
        Command::SetMeshBlendMode {
            name: "Skin".into(),
            blend_mode: BlendMode::Multiply,
        },
        Command::SetMeshOpacity {
            name: "Skin".into(),
            opacity: 0.5,
        },
        Command::SetMeshDrawOrder {
            name: "Skin".into(),
            draw_order: 700.0,
        },
        Command::SetMeshColors {
            name: "Skin".into(),
            multiply_color: [0.5, 0.75, 1.0],
            screen_color: [0.25, 0.0, 0.0],
        },
        Command::SetMeshKeyformColors {
            name: "Skin".into(),
            key: vec![1],
            multiply_color: [0.5, 0.5, 0.5],
            screen_color: [0.0, 0.25, 0.0],
        },
        Command::SetMeshUvs {
            name: "Skin".into(),
            uvs: triangle.clone(),
        },
        Command::SetMeshMasks {
            name: "Skin".into(),
            masks: vec!["Clip".into()],
            inverted: false,
        },
        Command::SetMeshVisible {
            name: "Skin".into(),
            visible: false,
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
        Command::SetPartVisible { .. } => "set_part_visible",
        Command::AddMesh { .. } => "add_mesh",
        Command::RemoveMesh { .. } => "remove_mesh",
        Command::SetMeshPositions { .. } => "set_mesh_positions",
        Command::SetMeshGeometry { .. } => "set_mesh_geometry",
        Command::AddWarpDeformer { .. } => "add_warp_deformer",
        Command::AddRotationDeformer { .. } => "add_rotation_deformer",
        Command::RemoveDeformer { .. } => "remove_deformer",
        Command::SetDeformerParent { .. } => "set_deformer_parent",
        Command::SetMeshParent { .. } => "set_mesh_parent",
        Command::SetWarpBasePoints { .. } => "set_warp_base_points",
        Command::ClearWarpRestPoints { .. } => "clear_warp_rest_points",
        Command::SetWarpBezier { .. } => "set_warp_bezier",
        Command::RenameParameter { .. } => "rename_parameter",
        Command::SetParameterRepeat { .. } => "set_parameter_repeat",
        Command::SetParameterGroup { .. } => "set_parameter_group",
        Command::MoveParameter { .. } => "move_parameter",
        Command::MoveParameterKeyforms { .. } => "move_parameter_keyforms",
        Command::BindParameter { .. } => "bind_parameter",
        Command::UnbindParameter { .. } => "unbind_parameter",
        Command::AddParameterKey { .. } => "add_parameter_key",
        Command::SetAxisOrder { .. } => "set_axis_order",
        Command::RemoveParameterKey { .. } => "remove_parameter_key",
        Command::SetMeshKeyform { .. } => "set_mesh_keyform",
        Command::SetMeshKeyformColors { .. } => "set_mesh_keyform_colors",
        Command::SetMeshColors { .. } => "set_mesh_colors",
        Command::SetWarpKeyform { .. } => "set_warp_keyform",
        Command::SetRotationKeyform { .. } => "set_rotation_keyform",
        Command::SetRotationBase { .. } => "set_rotation_base",
        Command::SetGlueKeyform { .. } => "set_glue_keyform",
        Command::AddGlue { .. } => "add_glue",
        Command::RemoveGlue { .. } => "remove_glue",
        Command::SetGlueIntensity { .. } => "set_glue_intensity",
        Command::RenamePart { .. } => "rename_part",
        Command::RenameMesh { .. } => "rename_mesh",
        Command::RenameWarp { .. } => "rename_warp",
        Command::RenameRotation { .. } => "rename_rotation",
        Command::RenameGlue { .. } => "rename_glue",
        Command::SetMeshPart { .. } => "set_mesh_part",
        Command::SetDeformerPart { .. } => "set_deformer_part",
        Command::SetPartParent { .. } => "set_part_parent",
        Command::SetMeshTexture { .. } => "set_mesh_texture",
        Command::SetMeshBlendMode { .. } => "set_mesh_blend_mode",
        Command::SetMeshOpacity { .. } => "set_mesh_opacity",
        Command::SetMeshDrawOrder { .. } => "set_mesh_draw_order",
        Command::SetMeshUvs { .. } => "set_mesh_uvs",
        Command::SetMeshMasks { .. } => "set_mesh_masks",
        Command::SetMeshVisible { .. } => "set_mesh_visible",
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
                serde_json::to_string(example).unwrap_or_default()
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
