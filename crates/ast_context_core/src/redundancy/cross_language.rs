use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 85: FFI boundary
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_ffi_boundary(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Check function source for FFI patterns
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        let ffi_type = if src.contains("extern \"C\"") || src.contains("extern \"c\"") {
            Some("extern C")
        } else if src.contains("ctypes") || src.contains("cffi") {
            Some("ctypes/cffi")
        } else if src.contains("wasm_bindgen") || src.contains("wasm-bindgen") {
            Some("wasm_bindgen")
        } else if src.contains("JNIEnv") || src.contains("jni::") {
            Some("JNI")
        } else if src.contains("napi") || src.contains("N-API") || src.contains("node_api") {
            Some("N-API")
        } else if src.contains("cgo") || src.contains("// #cgo") {
            Some("cgo")
        } else if src.contains("pybind11") || src.contains("PyO3") || src.contains("pyo3") {
            Some("PyO3/pybind11")
        } else {
            None
        };

        if let Some(ffi) = ffi_type {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::FfiBoundary {
                    function_name: func.name.clone(),
                    ffi_type: ffi.to_string(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}` uses {} — this is a cross-language boundary. Changes here affect both sides of the FFI.",
                    func.name, ffi
                ),
            });
        }
    }

    // Also check decorators for FFI markers
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        for dec in &func.decorators {
            let ffi_type = if dec.contains("wasm_bindgen") {
                Some("wasm_bindgen")
            } else if dec.contains("no_mangle") {
                Some("extern (no_mangle)")
            } else if dec.contains("pyo3") || dec.contains("pyfunction") || dec.contains("pyclass") {
                Some("PyO3")
            } else {
                None
            };

            if let Some(ffi) = ffi_type {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::FfiBoundary {
                        function_name: func.name.clone(),
                        ffi_type: ffi.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` is decorated with {} — cross-language boundary.",
                        func.name, ffi
                    ),
                });
            }
        }
    }

    // Check imports for FFI-related modules
    for &(idx, node) in &ctx.modules {
        let _m = if let GraphNode::Module(m) = node { m } else { continue };
        if let GraphNode::Module(m) = node {
            let ffi_type = if m.name == "ctypes" || m.name == "cffi" {
                Some("ctypes/cffi")
            } else if m.name == "subprocess" || m.name == "child_process" {
                // handled in subprocess check
                None
            } else if m.name.contains("grpc") || m.name.contains("protobuf") || m.name.contains("proto") {
                // handled in IPC check
                None
            } else if m.name == "jni" || m.name.starts_with("jni::") {
                Some("JNI")
            } else if m.name == "napi" || m.name.starts_with("napi::") || m.name == "node-addon-api" {
                Some("N-API")
            } else if m.name == "pyo3" || m.name.starts_with("pyo3::") {
                Some("PyO3")
            } else if m.name == "wasm_bindgen" || m.name.starts_with("wasm_bindgen::") {
                Some("wasm_bindgen")
            } else {
                None
            };

            if let Some(ffi) = ffi_type {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::FfiBoundary {
                        function_name: format!("import:{}", m.name),
                        ffi_type: ffi.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "Module `{}` is an FFI dependency ({}) — this codebase has a cross-language boundary.",
                        m.name, ffi
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 86: Subprocess/exec calls
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_subprocess_calls(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let patterns = [
        ("subprocess.run", "subprocess.run"),
        ("subprocess.Popen", "subprocess.Popen"),
        ("subprocess.call", "subprocess.call"),
        ("os.system", "os.system"),
        ("os.exec", "os.exec"),
        ("os.popen", "os.popen"),
        ("Command::new", "std::process::Command"),
        ("child_process.exec", "child_process.exec"),
        ("child_process.spawn", "child_process.spawn"),
        ("exec.Command", "os/exec.Command"),
        ("Runtime.exec", "Runtime.exec"),
        ("ProcessBuilder", "ProcessBuilder"),
        ("system(", "system()"),
        ("popen(", "popen()"),
        ("execve(", "execve()"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        for (pattern, label) in &patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::SubprocessCall {
                        function_name: func.name.clone(),
                        call_pattern: label.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` spawns an external process via `{}` — cross-process boundary, may have different error/lifecycle semantics.",
                        func.name, label
                    ),
                });
                break; // one finding per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 87: IPC/RPC boundary
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_ipc_boundary(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Check imports for RPC/IPC frameworks
    for &(idx, node) in &ctx.modules {
        let _m = if let GraphNode::Module(m) = node { m } else { continue };
        if let GraphNode::Module(m) = node {
            let protocol = if m.name.contains("grpc") || m.name.contains("tonic") {
                Some("gRPC")
            } else if m.name.contains("protobuf") || m.name.contains("proto") || m.name.contains("prost") {
                Some("protobuf")
            } else if m.name.contains("thrift") {
                Some("Thrift")
            } else if m.name.contains("amqp") || m.name.contains("rabbitmq") || m.name.contains("celery") {
                Some("message queue")
            } else if m.name.contains("kafka") {
                Some("Kafka")
            } else if m.name.contains("redis") {
                Some("Redis")
            } else if m.name.contains("zmq") || m.name.contains("zeromq") {
                Some("ZeroMQ")
            } else if m.name.contains("websocket") || m.name == "ws" || m.name == "tokio_tungstenite" {
                Some("WebSocket")
            } else {
                None
            };

            if let Some(proto) = protocol {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::IpcBoundary {
                        file_name: m.name.clone(),
                        protocol: proto.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "Module `{}` indicates a {} boundary — data crosses process/network boundaries here.",
                        m.name, proto
                    ),
                });
            }
        }
    }

    // Check for REST endpoint decorators
    let rest_decorators = ["app.route", "app.get", "app.post", "app.put", "app.delete",
        "router.get", "router.post", "router.put", "router.delete",
        "GetMapping", "PostMapping", "RequestMapping",
        "api_view", "action", "HttpGet", "HttpPost"];

    for &(idx, node) in &ctx.functions {
        let _f = if let GraphNode::Function(f) = node { f } else { continue };
        if let GraphNode::Function(f) = node {
            if f.is_dependency {
                continue;
            }
            for dec in &f.decorators {
                if rest_decorators.iter().any(|rd| dec.contains(rd)) {
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::IpcBoundary {
                            file_name: f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                            protocol: "REST endpoint".to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}` is a REST endpoint (`{}`) — this is a network API boundary.",
                            f.name, dec
                        ),
                    });
                    break;
                }
            }
        }
    }
}
