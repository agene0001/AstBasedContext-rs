mod protocol;
mod tools;

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use log::info;
use serde_json::json;

use protocol::{JsonRpcRequest, JsonRpcResponse};
use tools::{ServerState, SharedState};

const SERVER_NAME: &str = "ast-context-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

    info!("{SERVER_NAME} v{SERVER_VERSION} starting");

    let state: SharedState = Arc::new(Mutex::new(ServerState::new()));
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Read error: {e}");
                break;
            }
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                send_response(&stdout, &resp);
                continue;
            }
        };

        let response = handle_request(&state, &request);
        send_response(&stdout, &response);
    }

    info!("{SERVER_NAME} shutting down");
}

fn handle_request(state: &SharedState, req: &JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            });
            JsonRpcResponse::success(req.id.clone(), result)
        }

        "notifications/initialized" => {
            // No response needed for notifications, but if there's an id, respond
            if req.id.is_some() {
                JsonRpcResponse::success(req.id.clone(), json!({}))
            } else {
                // Notification — no response
                JsonRpcResponse::success(None, json!({}))
            }
        }

        "tools/list" => {
            let tools: Vec<_> = tools::list_tools()
                .into_iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                    })
                })
                .collect();
            JsonRpcResponse::success(req.id.clone(), json!({ "tools": tools }))
        }

        "tools/call" => {
            let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));

            let result = tools::handle_tool(state, tool_name, &arguments);
            let content: Vec<_> = result
                .content
                .into_iter()
                .map(|c| {
                    json!({
                        "type": c.content_type,
                        "text": c.text,
                    })
                })
                .collect();

            let mut resp = json!({ "content": content });
            if let Some(true) = result.is_error {
                resp["isError"] = json!(true);
            }

            JsonRpcResponse::success(req.id.clone(), resp)
        }

        "ping" => JsonRpcResponse::success(req.id.clone(), json!({})),

        _ => JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("Method not found: {}", req.method),
        ),
    }
}

fn send_response(stdout: &io::Stdout, resp: &JsonRpcResponse) {
    // Skip sending responses for notifications (no id)
    if resp.id.is_none() && resp.error.is_none() {
        return;
    }
    let json = serde_json::to_string(resp).unwrap();
    let mut out = stdout.lock();
    let _ = writeln!(out, "{json}");
    let _ = out.flush();
}
