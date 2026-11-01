// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe mcp <project>`: a minimal MCP server over stdio (newline
//! delimited JSON-RPC 2.0: `initialize`, `ping`, `tools/list`, `tools/call`).
//! Standard output carries only JSON-RPC; logs go to standard error.

use std::io::{self, BufRead, Read, Write};

use serde_json::{Value, json};
use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_format::Project;

use crate::commands::{command_tools, session_tools};
use crate::store;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// A longer message is rejected.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// One project open in memory.
pub struct Session {
    path: String,
    project: Project,
    model: Model,
}

impl Session {
    pub fn open(path: &str) -> Result<Self, String> {
        let (project, model) = store::open(path)?;
        Ok(Self {
            path: path.to_owned(),
            project,
            model,
        })
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The response to one message line; `None` for a notification.
    pub fn handle(&mut self, line: &[u8]) -> Option<Value> {
        let Ok(text) = std::str::from_utf8(line) else {
            return Some(error(Value::Null, PARSE_ERROR, "message is not UTF-8"));
        };
        let message: Value = match serde_json::from_str(text) {
            Ok(m) => m,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, &e.to_string())),
        };
        let Some(object) = message.as_object() else {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "expected a JSON object",
            ));
        };
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to something we never asked: ignore it.
            return object
                .contains_key("method")
                .then(|| error(Value::Null, INVALID_REQUEST, "method must be a string"));
        };
        let id = object.get("id")?;
        if !(id.is_string() || id.is_number()) {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "id must be a string or number",
            ));
        }
        let params = object.get("params").unwrap_or(&Value::Null);
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "tsukumoe", "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": command_tools().into_iter().chain(session_tools()).collect::<Vec<_>>(),
            })),
            "tools/call" => self.call(params),
            other => Err((METHOD_NOT_FOUND, format!("unknown method {other:?}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id.clone(), code, &message),
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err((INVALID_PARAMS, "params.name is required".into()));
        };
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => return Err((INVALID_PARAMS, "arguments must be an object".into())),
        };
        let outcome = if command_tools().iter().any(|t| t["name"] == name) {
            self.command(name, arguments)
        } else {
            match name {
                "undo" => flag(self.model.undo(), "nothing to undo"),
                "redo" => flag(self.model.redo(), "nothing to redo"),
                "validate" => {
                    let errors: Vec<String> = self
                        .model
                        .validate()
                        .iter()
                        .map(|e| format!("{e:?}"))
                        .collect();
                    Ok(json!({ "ok": errors.is_empty(), "errors": errors }).to_string())
                }
                "get_model" => Ok(self.summary().to_string()),
                "save" => store::save(&self.path, &mut self.project, &self.model)
                    .map(|()| "saved".to_owned()),
                _ => return Err((INVALID_PARAMS, format!("unknown tool {name:?}"))),
            }
        };
        let (text, is_error) = match outcome {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
    }

    fn command(&mut self, name: &str, mut arguments: Value) -> Result<String, String> {
        arguments["type"] = name.into();
        let command: Command = serde_json::from_value(arguments).map_err(|e| e.to_string())?;
        self.model.apply(command).map_err(|e| e.to_string())?;
        Ok("applied".into())
    }

    fn summary(&self) -> Value {
        let m = &self.model;
        let names = |t: Vec<&str>| json!({ "count": t.len(), "names": t });
        json!({
            "parameters": names(m.parameters().iter().map(|(_, n, _)| n).collect()),
            "parts": names(m.parts().iter().map(|(_, n, _)| n).collect()),
            "meshes": names(m.meshes().iter().map(|(_, n, _)| n).collect()),
            "warps": m.warps().len(),
            "rotations": m.rotations().len(),
            "motions": m.motions().len(),
            "expressions": m.expressions().len(),
            "can_undo": m.history().can_undo(),
            "can_redo": m.history().can_redo(),
        })
    }
}

fn flag(done: bool, otherwise: &str) -> Result<String, String> {
    if done {
        Ok("done".into())
    } else {
        Err(otherwise.into())
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Serves `input` until it ends. An oversized or malformed line gets an
/// error response and the server keeps going.
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    session: &mut Session,
) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = (&mut input)
            .take(MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        let response = if line.len() > MAX_MESSAGE_BYTES {
            // Drop the rest of the line without keeping it.
            loop {
                let chunk = input.fill_buf()?;
                let Some(end) = chunk.iter().position(|&b| b == b'\n') else {
                    let n = chunk.len();
                    if n == 0 {
                        break;
                    }
                    input.consume(n);
                    continue;
                };
                input.consume(end + 1);
                break;
            }
            Some(error(Value::Null, INVALID_REQUEST, "message is too large"))
        } else if line.iter().all(u8::is_ascii_whitespace) {
            None
        } else {
            session.handle(&line)
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
}
