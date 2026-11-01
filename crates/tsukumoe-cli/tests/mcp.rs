// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The MCP server: protocol round trips, malformed input, command results,
//! and a child process driven over standard input and output.

mod common;

use std::io::{BufRead, BufReader, Cursor, Write};
use std::process::{Command, Stdio};

use common::empty_project;
use serde_json::{Value, json};
use tsukumoe_cli::commands::examples;
use tsukumoe_cli::mcp::{MAX_MESSAGE_BYTES, Session, serve};
use tsukumoe_cli::store;
use tsukumoe_core::command::Command as Edit;
use tsukumoe_core::model::Model;

fn session(name: &str) -> Session {
    Session::open(empty_project(name, false).to_str().unwrap()).unwrap()
}

fn send(session: &mut Session, message: Value) -> Value {
    session.handle(message.to_string().as_bytes()).unwrap()
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn call(session: &mut Session, name: &str, arguments: Value) -> Value {
    let response = send(
        session,
        request(
            1,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        ),
    );
    response["result"].clone()
}

fn is_error(result: &Value) -> bool {
    result["isError"] == true
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap()
}

/// The commands as `tools/call` arguments: the tool name and the other fields.
fn as_call(command: &Edit) -> (String, Value) {
    let mut value = serde_json::to_value(command).unwrap();
    let name = value["type"].as_str().unwrap().to_owned();
    value.as_object_mut().unwrap().remove("type");
    (name, value)
}

fn scene() -> Vec<Edit> {
    serde_json::from_str(
        r#"[
        {"type":"add_parameter","name":"P","min":0,"max":1,"default":0},
        {"type":"add_part","name":"Face"},
        {"type":"add_mesh","name":"Skin","part":"Face","positions":[[0,0],[1,0],[0,1]],"uvs":[[0,0],[1,0],[0,1]],"indices":[0,1,2]},
        {"type":"set_part_opacity","name":"Face","opacity":0.5},
        {"type":"set_mesh_positions","name":"Skin","positions":[[0,0],[2,0],[0,2]]}
    ]"#,
    )
    .unwrap()
}

#[test]
fn handshake_list_and_ping() {
    let mut s = session("mcp_handshake");
    let init = send(
        &mut s,
        request(1, "initialize", json!({"protocolVersion": "1999-01-01"})),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    // A notification gets no response.
    assert!(
        s.handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );
    assert_eq!(
        send(&mut s, request(2, "ping", Value::Null))["result"],
        json!({})
    );

    let list = send(&mut s, request(3, "tools/list", Value::Null));
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "add_parameter",
        "remove_parameter",
        "set_parameter_range",
        "add_part",
        "remove_part",
        "set_part_opacity",
        "add_mesh",
        "remove_mesh",
        "set_mesh_positions",
        "batch",
        "undo",
        "redo",
        "validate",
        "get_model",
        "save",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(!tool["description"].as_str().unwrap().is_empty());
    }
    assert_eq!(list["id"], 3);
}

#[test]
fn bad_requests_are_errors_and_the_server_survives() {
    let mut s = session("mcp_errors");
    let code = |response: Value| response["error"]["code"].as_i64().unwrap();
    assert_eq!(code(send(&mut s, request(1, "nope", Value::Null))), -32601);
    assert_eq!(
        code(send(
            &mut s,
            request(2, "tools/call", json!({"name": "nope"}))
        )),
        -32602
    );
    assert_eq!(
        code(send(&mut s, request(3, "tools/call", json!({})))),
        -32602
    );
    assert_eq!(
        code(send(&mut s, request(4, "tools/call", json!("x")))),
        -32602
    );
    assert_eq!(code(s.handle(b"{not json").unwrap()), -32700);
    assert_eq!(code(s.handle(&[0xff, 0xfe, b'{']).unwrap()), -32700);
    assert_eq!(code(s.handle(b"[1,2]").unwrap()), -32600);
    assert_eq!(code(s.handle(b"42").unwrap()), -32600);
    assert_eq!(
        code(s.handle(br#"{"id":{},"method":"ping"}"#).unwrap()),
        -32600
    );
    assert_eq!(code(s.handle(br#"{"id":1,"method":7}"#).unwrap()), -32600);
    // Arguments of the wrong shape are tool errors, not protocol errors.
    for arguments in [
        json!({}),
        json!({"name": 5}),
        json!({"name": "P", "min": "x", "max": 1, "default": 0}),
    ] {
        assert!(is_error(&call(&mut s, "add_parameter", arguments)));
    }
    assert!(is_error(&call(
        &mut s,
        "batch",
        json!({"commands": [{"type": "nope"}]})
    )));
    assert_eq!(
        send(&mut s, request(5, "ping", Value::Null))["result"],
        json!({})
    );
    assert!(*s.model() == Model::new());
}

#[test]
fn mutated_messages_never_panic() {
    let mut s = session("mcp_mutation");
    let seeds = [
        request(
            1,
            "tools/call",
            json!({"name": "add_part", "arguments": {"name": "Face"}}),
        ),
        request(2, "tools/list", Value::Null),
        request(3, "initialize", json!({})),
    ];
    let mut state = 12345u32;
    let mut next = move || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 8) as usize
    };
    for seed in &seeds {
        let bytes = seed.to_string().into_bytes();
        for _ in 0..500 {
            let mut m = bytes.clone();
            match next() % 3 {
                0 => m.truncate(next() % (m.len() + 1)),
                1 => {
                    let i = next() % m.len();
                    m[i] = next() as u8;
                }
                _ => {
                    let i = next() % m.len();
                    m.remove(i);
                }
            }
            let _ = s.handle(&m);
        }
    }
}

#[test]
fn tool_calls_match_apply() {
    let mut s = session("mcp_apply");
    let mut direct = Model::new();
    for command in scene() {
        let (name, arguments) = as_call(&command);
        let result = call(&mut s, &name, arguments);
        assert!(!is_error(&result), "{name}: {}", text(&result));
        direct.apply(command).unwrap();
    }
    assert!(*s.model() == direct);
    let summary: Value = serde_json::from_str(text(&call(&mut s, "get_model", json!({})))).unwrap();
    assert_eq!(summary["parts"]["names"], json!(["Face"]));
    assert_eq!(summary["meshes"]["count"], 1);
    let validation: Value =
        serde_json::from_str(text(&call(&mut s, "validate", json!({})))).unwrap();
    assert_eq!(validation["ok"], true);
}

#[test]
fn a_failing_command_leaves_the_model_unchanged() {
    let mut s = session("mcp_fail");
    call(&mut s, "add_part", json!({"name": "Face"}));
    let before = s.model().history().commands().count();
    let result = call(&mut s, "add_part", json!({"name": "Face"}));
    assert!(is_error(&result));
    assert_eq!(text(&result), "part 'Face' already exists");
    let result = call(
        &mut s,
        "batch",
        json!({"commands": [{"type": "add_part", "name": "B"}, {"type": "remove_part", "name": "Nope"}]}),
    );
    assert!(is_error(&result));
    assert_eq!(s.model().parts().len(), 1);
    assert_eq!(s.model().history().commands().count(), before);
}

#[test]
fn batch_is_one_undo_and_undo_redo_round_trip() {
    let mut s = session("mcp_undo");
    let result = call(
        &mut s,
        "batch",
        json!({"commands": [{"type": "add_part", "name": "A"}, {"type": "add_part", "name": "B"}]}),
    );
    assert!(!is_error(&result));
    assert_eq!(s.model().parts().len(), 2);
    assert!(!is_error(&call(&mut s, "undo", json!({}))));
    assert_eq!(s.model().parts().len(), 0);
    assert!(is_error(&call(&mut s, "undo", json!({}))));
    assert!(!is_error(&call(&mut s, "redo", json!({}))));
    assert_eq!(s.model().parts().len(), 2);
    assert!(is_error(&call(&mut s, "redo", json!({}))));
}

#[test]
fn every_example_command_runs_as_a_tool() {
    // Examples refer to each other's elements, so replay the whole list in a
    // batch-free order: a failure is allowed only as a command error.
    let mut s = session("mcp_examples");
    for command in examples() {
        let (name, arguments) = as_call(&command);
        let result = call(&mut s, &name, arguments);
        assert!(result["content"].is_array(), "{name} gave a protocol error");
    }
}

#[test]
fn an_oversized_line_is_rejected_and_the_next_one_served() {
    let mut s = session("mcp_big");
    let mut input = vec![b'x'; MAX_MESSAGE_BYTES + 10];
    input.push(b'\n');
    input.extend_from_slice(request(7, "ping", Value::Null).to_string().as_bytes());
    input.push(b'\n');
    let mut output = Vec::new();
    serve(Cursor::new(input), &mut output, &mut s).unwrap();
    let lines: Vec<Value> = output
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["error"]["code"], -32600);
    assert_eq!(lines[1]["id"], 7);
}

#[test]
fn child_process_edits_saves_and_validates() {
    let project = empty_project("mcp_e2e", false);
    let path = project.to_str().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(["mcp", path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut roundtrip = |message: Value| -> Value {
        writeln!(stdin, "{message}").unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert_eq!(
        roundtrip(request(1, "initialize", json!({})))["result"]["serverInfo"]["name"],
        "tsukumoe"
    );
    assert!(roundtrip(request(2, "tools/list", Value::Null))["result"]["tools"].is_array());
    let mut direct = Model::new();
    for (i, command) in scene().into_iter().enumerate() {
        let (name, arguments) = as_call(&command);
        let response = roundtrip(request(
            10 + i as u64,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        ));
        assert_eq!(response["result"]["isError"], false, "{response}");
        direct.apply(command).unwrap();
    }
    let saved = roundtrip(request(30, "tools/call", json!({"name": "save"})));
    assert_eq!(saved["result"]["isError"], false, "{saved}");
    drop(stdin);
    assert!(child.wait().unwrap().success());

    let (_, model) = store::open(path).unwrap();
    assert!(model == direct);
    let validate = Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(["validate", path])
        .output()
        .unwrap();
    assert!(
        validate.status.success(),
        "{}",
        String::from_utf8_lossy(&validate.stdout)
    );
}
