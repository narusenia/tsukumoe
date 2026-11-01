// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command schemas, the CLI listing and the generated reference.

mod common;

use std::process::Command;

use common::empty_project;
use serde_json::Value;
use tsukumoe_cli::commands::{command_schema, command_tools, examples, reference, tag};
use tsukumoe_cli::mcp::Session;
use tsukumoe_core::command::Command as Edit;

#[test]
fn every_variant_has_a_tool_with_a_type_constant_and_an_example() {
    let tools = command_tools();
    let examples = examples();
    assert_eq!(tools.len(), examples.len());
    for example in &examples {
        let name = tag(example);
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no tool for {name}"));
        assert_eq!(tool["inputSchema"]["properties"]["type"]["const"], name);
        assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));

        // The example validates against its own schema and the whole one,
        // and comes back as the same command.
        let json = serde_json::to_value(example).unwrap();
        jsonschema::validate(&tool["inputSchema"], &json)
            .unwrap_or_else(|e| panic!("{name} against its tool schema: {e}"));
        jsonschema::validate(&command_schema(), &json)
            .unwrap_or_else(|e| panic!("{name} against the Command schema: {e}"));
        assert_eq!(&serde_json::from_value::<Edit>(json).unwrap(), example);
    }
}

#[test]
fn schemas_reject_a_wrong_type_or_a_missing_field() {
    let tools = command_tools();
    let schema =
        |name: &str| tools.iter().find(|t| t["name"] == name).unwrap()["inputSchema"].clone();
    let add_part = schema("add_part");
    assert!(jsonschema::validate(&add_part, &serde_json::json!({"type": "add_part"})).is_err());
    assert!(
        jsonschema::validate(
            &add_part,
            &serde_json::json!({"type": "add_part", "name": 3})
        )
        .is_err()
    );
    assert!(
        jsonschema::validate(
            &add_part,
            &serde_json::json!({"type": "remove_part", "name": "x"})
        )
        .is_err()
    );
    // The nested commands of a batch are checked against the whole enum.
    let batch = schema("batch");
    let bad = serde_json::json!({"type": "batch", "commands": [{"type": "add_part"}]});
    assert!(jsonschema::validate(&batch, &bad).is_err());
}

#[test]
fn commands_lists_the_same_tools_as_the_server() {
    let out = Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .arg("commands")
        .output()
        .unwrap();
    assert!(out.status.success());
    let listed: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    let mut session =
        Session::open(empty_project("commands_list", false).to_str().unwrap()).unwrap();
    let response = session
        .handle(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .unwrap();
    let served = response["result"]["tools"].as_array().unwrap();
    assert_eq!(listed.len(), 10);
    for tool in &listed {
        assert!(
            served.contains(tool),
            "{} differs from tools/list",
            tool["name"]
        );
    }
}

#[test]
fn reference_lists_every_command_and_matches_the_committed_file() {
    let generated = reference();
    for tool in command_tools() {
        let name = tool["name"].as_str().unwrap();
        assert!(
            generated.contains(&format!("### `{name}`")),
            "{name} missing"
        );
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reference/commands.md"
    );
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        committed == generated,
        "docs/reference/commands.md is stale; run `cargo run -p tsukumoe-cli -- docs commands > docs/reference/commands.md`"
    );
}
