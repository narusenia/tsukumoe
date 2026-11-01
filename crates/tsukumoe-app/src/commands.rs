// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command list for the palette, generated from the `Command` JSON Schema
//! (`tsukumoe-core`'s `schema` feature). `tsukumoe-cli` builds the same list
//! for MCP; this is a small separate reading of the schema so the editor does
//! not depend on the CLI crate. Moving the shared generator into a common
//! crate is an open item.

use schemars::schema_for;
use serde_json::{Map, Value, json};
use tsukumoe_core::command::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Bool,
    /// Arrays and objects: typed as JSON.
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    /// The `type` tag, such as `add_parameter`.
    pub name: String,
    pub description: String,
    pub fields: Vec<Field>,
}

fn kind_of(schema: &Value) -> FieldKind {
    let types: Vec<&str> = match &schema["type"] {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    let non_null: Vec<&&str> = types.iter().filter(|t| **t != "null").collect();
    match non_null.as_slice() {
        [t] if **t == "string" => FieldKind::Text,
        [t] if **t == "number" || **t == "integer" => FieldKind::Number,
        [t] if **t == "boolean" => FieldKind::Bool,
        _ => FieldKind::Json,
    }
}

/// Every command with its fields, in the enum's order.
pub fn all() -> Vec<CommandInfo> {
    let root = serde_json::to_value(schema_for!(Command)).expect("schema is JSON");
    let variants = root["oneOf"].as_array().cloned().unwrap_or_default();
    variants
        .iter()
        .filter_map(|v| {
            let name = v["properties"]["type"]["const"].as_str()?.to_owned();
            let required: Vec<&str> = v["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let fields = v["properties"]
                .as_object()?
                .iter()
                .filter(|(k, _)| *k != "type")
                .map(|(k, s)| Field {
                    name: k.clone(),
                    kind: kind_of(s),
                    required: required.contains(&k.as_str()),
                })
                .collect();
            Some(CommandInfo {
                name,
                description: v["description"].as_str().unwrap_or_default().to_owned(),
                fields,
            })
        })
        .collect()
}

/// Builds a command from the text typed for each field (same order as
/// `info.fields`). An empty optional field is left out; the result is checked
/// by deserializing into `Command`, so a wrong type is an error message.
pub fn build(info: &CommandInfo, inputs: &[String]) -> Result<Command, String> {
    let mut object = Map::new();
    object.insert("type".into(), json!(info.name));
    for (field, text) in info.fields.iter().zip(inputs) {
        let text = text.trim();
        if text.is_empty() {
            if field.required {
                return Err(format!("{}: required", field.name));
            }
            continue;
        }
        let value = match field.kind {
            FieldKind::Text => json!(text),
            FieldKind::Number => json!(
                text.parse::<f64>()
                    .map_err(|_| format!("{}: not a number", field.name))?
            ),
            FieldKind::Bool => json!(
                text.parse::<bool>()
                    .map_err(|_| format!("{}: true or false", field.name))?
            ),
            FieldKind::Json => {
                serde_json::from_str(text).map_err(|e| format!("{}: {e}", field.name))?
            }
        };
        object.insert(field.name.clone(), value);
    }
    serde_json::from_value(Value::Object(object)).map_err(|e| e.to_string())
}

/// Commands whose name or description contains `query` (case-insensitive).
pub fn search<'a>(list: &'a [CommandInfo], query: &str) -> Vec<&'a CommandInfo> {
    let q = query.trim().to_lowercase();
    list.iter()
        .filter(|c| {
            q.is_empty()
                || c.name.contains(&q)
                || c.name.replace('_', " ").contains(&q)
                || c.description.to_lowercase().contains(&q)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adding a variant to `Command` makes this match fail to compile; add it
    /// here and the palette test below then checks the schema lists it.
    fn tag(c: &Command) -> &'static str {
        match c {
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
            Command::SetRotationBase { .. } => "set_rotation_base",
            Command::RenamePart { .. } => "rename_part",
            Command::RenameMesh { .. } => "rename_mesh",
            Command::RenameWarp { .. } => "rename_warp",
            Command::RenameRotation { .. } => "rename_rotation",
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

    #[test]
    fn every_command_variant_is_in_the_list() {
        let samples = [
            Command::AddParameter {
                name: String::new(),
                min: 0.0,
                max: 0.0,
                default: 0.0,
                repeat: false,
            },
            Command::RemoveParameter {
                name: String::new(),
            },
            Command::SetParameterRange {
                name: String::new(),
                min: 0.0,
                max: 0.0,
                default: 0.0,
            },
            Command::AddPart {
                name: String::new(),
                parent: None,
            },
            Command::RemovePart {
                name: String::new(),
            },
            Command::SetPartOpacity {
                name: String::new(),
                opacity: 0.0,
            },
            Command::SetPartVisible {
                name: String::new(),
                visible: true,
            },
            Command::AddMesh {
                name: String::new(),
                part: String::new(),
                positions: vec![],
                uvs: vec![],
                indices: vec![],
            },
            Command::RemoveMesh {
                name: String::new(),
            },
            Command::SetMeshPositions {
                name: String::new(),
                positions: vec![],
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
                base: tsukumoe_core::command::RotationBase {
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
            Command::SetRotationBase {
                name: "HeadRotation".into(),
                base: tsukumoe_core::command::RotationBase {
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
                name: String::new(),
                positions: vec![],
                uvs: vec![],
                indices: vec![],
            },
            Command::SetMeshTexture {
                name: String::new(),
                texture: 0,
            },
            Command::SetMeshBlendMode {
                name: String::new(),
                blend_mode: tsukumoe_core::rig::BlendMode::Normal,
            },
            Command::SetMeshOpacity {
                name: String::new(),
                opacity: 0.0,
            },
            Command::SetMeshDrawOrder {
                name: String::new(),
                draw_order: 0.0,
            },
            Command::SetMeshUvs {
                name: String::new(),
                uvs: vec![],
            },
            Command::SetMeshMasks {
                name: String::new(),
                masks: vec![],
                inverted: false,
            },
            Command::SetMeshVisible {
                name: String::new(),
                visible: true,
            },
            Command::Batch { commands: vec![] },
        ];
        let listed: Vec<String> = all().into_iter().map(|c| c.name).collect();
        let mut expected: Vec<&str> = samples.iter().map(tag).collect();
        expected.sort_unstable();
        let mut got: Vec<&str> = listed.iter().map(String::as_str).collect();
        got.sort_unstable();
        assert_eq!(got, expected);
        // the sample tags are what serde names them
        for s in &samples {
            assert_eq!(serde_json::to_value(s).unwrap()["type"], tag(s));
        }
    }

    #[test]
    fn fields_get_kinds_and_required_flags() {
        let list = all();
        let add = list.iter().find(|c| c.name == "add_parameter").unwrap();
        let f = |n: &str| add.fields.iter().find(|f| f.name == n).unwrap();
        assert_eq!(
            (f("name").kind, f("name").required),
            (FieldKind::Text, true)
        );
        assert_eq!(f("min").kind, FieldKind::Number);
        assert_eq!(
            (f("repeat").kind, f("repeat").required),
            (FieldKind::Bool, false)
        );
        let part = list.iter().find(|c| c.name == "add_part").unwrap();
        let parent = part.fields.iter().find(|f| f.name == "parent").unwrap();
        assert_eq!((parent.kind, parent.required), (FieldKind::Text, false));
        let batch = list.iter().find(|c| c.name == "batch").unwrap();
        assert_eq!(batch.fields[0].kind, FieldKind::Json);
    }

    #[test]
    fn forms_build_commands_and_report_bad_input() {
        let list = all();
        let info = list.iter().find(|c| c.name == "add_parameter").unwrap();
        let text = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let names: Vec<&str> = info.fields.iter().map(|f| f.name.as_str()).collect();
        let mut inputs = vec![String::new(); names.len()];
        let set = |inputs: &mut Vec<String>, n: &str, v: &str| {
            inputs[names.iter().position(|x| *x == n).unwrap()] = v.to_owned();
        };
        assert!(build(info, &inputs).unwrap_err().contains("required"));
        set(&mut inputs, "name", "P");
        for (n, v) in [("min", "-1"), ("max", "1"), ("default", "0")] {
            set(&mut inputs, n, v);
        }
        assert_eq!(
            build(info, &inputs).unwrap(),
            Command::AddParameter {
                name: "P".into(),
                min: -1.0,
                max: 1.0,
                default: 0.0,
                repeat: false
            }
        );
        set(&mut inputs, "max", "x");
        assert!(build(info, &inputs).unwrap_err().contains("not a number"));
        let batch = list.iter().find(|c| c.name == "batch").unwrap();
        let ok = build(batch, &text(&[r#"[{"type":"add_part","name":"A"}]"#])).unwrap();
        assert!(matches!(ok, Command::Batch { commands } if commands.len() == 1));
        assert!(build(batch, &text(&["[1]"])).is_err());
    }

    #[test]
    fn search_matches_names_and_descriptions() {
        let list = all();
        assert_eq!(search(&list, "").len(), list.len());
        assert!(
            search(&list, "add part")
                .iter()
                .any(|c| c.name == "add_part")
        );
        assert!(
            search(&list, "OPACITY")
                .iter()
                .any(|c| c.name == "set_part_opacity")
        );
        assert!(search(&list, "zzzz").is_empty());
    }
}
