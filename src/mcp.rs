//! Minimal MCP (Model Context Protocol) stdio server.
//!
//! MCP uses newline-delimited JSON-RPC 2.0 over stdio. We implement the small
//! subset needed by desktop AI clients (initialize, tools/list, tools/call,
//! ping) without pulling in an async runtime or a large SDK.
//!
//! Capture and window operations are forwarded to the UI thread because the
//! screenshot must not contain PinSight's own window. Long screenshots and
//! recordings run on worker threads.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use crate::annotate::ImgRect;
use crate::util::PhysRect;

pub struct McpHandle {
    pub requests: Arc<AtomicUsize>,
}

#[derive(Debug)]
pub struct CapturedImage {
    pub path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub base64: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct PinSummary {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug)]
pub struct StatusInfo {
    pub version: String,
    pub pins: usize,
    pub recording: bool,
    pub save_dir: String,
    pub mcp_requests: usize,
}

pub enum UiCommand {
    Capture {
        monitor_id: Option<u32>,
        region: Option<PhysRect>,
        save: bool,
        return_base64: bool,
        reply: Sender<Result<CapturedImage, String>>,
    },
    Pin {
        path: Option<PathBuf>,
        base64: Option<String>,
        reply: Sender<Result<u64, String>>,
    },
    ListPins {
        reply: Sender<Result<Vec<PinSummary>, String>>,
    },
    ClosePin {
        id: u64,
        reply: Sender<Result<(), String>>,
    },
    StartRecording {
        monitor_id: Option<u32>,
        region: Option<PhysRect>,
        fps: u32,
        reply: Sender<Result<String, String>>,
    },
    StopRecording {
        reply: Sender<Result<String, String>>,
    },
    Status {
        reply: Sender<Result<StatusInfo, String>>,
    },
}

pub fn start(ui_tx: Sender<UiCommand>, save_dir: PathBuf) -> McpHandle {
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    std::thread::Builder::new()
        .name("pinsight-mcp".into())
        .spawn(move || serve_stdio(ui_tx, save_dir, counter))
        .expect("spawn MCP thread");
    McpHandle { requests }
}

fn serve_stdio(ui_tx: Sender<UiCommand>, save_dir: PathBuf, requests: Arc<AtomicUsize>) {
    tracing::info!("MCP stdio server listening");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            let err = json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {"code": -32700, "message": "parse error"}
            });
            write_message(&mut stdout, &err);
            continue;
        };

        let is_notification = message.get("id").is_none();
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if is_notification {
            continue;
        }

        requests.fetch_add(1, Ordering::Relaxed);
        let response = match method {
            "initialize" => {
                let version = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("2025-06-18");
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": version,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {
                            "name": "pinsight",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "PinSight exposes native screenshot, pinboard, long-screenshot and recording tools."
                    }
                })
            }
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": tool_definitions()}
            }),
            "resources/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            }),
            "prompts/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call_tool(name, &args, &ui_tx, &save_dir) {
                    Ok(text) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": text}],
                            "isError": false
                        }
                    }),
                    Err(err) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{"type": "text", "text": err}],
                            "isError": true
                        }
                    }),
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("method not found: {method}")}
            }),
        };

        write_message(&mut stdout, &response);
    }
}

fn write_message(stdout: &mut std::io::Stdout, value: &Value) {
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}

fn call_tool(
    name: &str,
    args: &Value,
    ui_tx: &Sender<UiCommand>,
    save_dir: &std::path::Path,
) -> Result<String, String> {
    match name {
        "capture_screenshot" => {
            let monitor_id = args
                .get("monitor_id")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            let save = args.get("save").and_then(Value::as_bool).unwrap_or(true);
            let return_base64 = args
                .get("return_base64")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let reply = capture(ui_tx, monitor_id, None, save, return_base64)?;
            Ok(serde_json::to_string_pretty(&reply).unwrap_or_default())
        }
        "capture_region" => {
            let region =
                parse_region(args).ok_or("capture_region requires x, y, width and height")?;
            let save = args.get("save").and_then(Value::as_bool).unwrap_or(true);
            let return_base64 = args
                .get("return_base64")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let reply = capture(ui_tx, None, Some(region), save, return_base64)?;
            Ok(serde_json::to_string_pretty(&reply).unwrap_or_default())
        }
        "pin_image" => {
            let path = args.get("path").and_then(Value::as_str).map(PathBuf::from);
            let base64 = args
                .get("base64")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if path.is_none() && base64.is_none() {
                return Err("pin_image requires either `path` or `base64`".into());
            }
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::Pin {
                    path,
                    base64,
                    reply: tx,
                })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            let id = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(json!({"pinned": true, "id": id}).to_string())
        }
        "list_pins" => {
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::ListPins { reply: tx })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            let pins = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(serde_json::to_string_pretty(&json!({"pins": pins})).unwrap_or_default())
        }
        "close_pin" => {
            let id = args
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("close_pin requires `id`")?;
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::ClosePin { id, reply: tx })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            rx.recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(json!({"closed": true, "id": id}).to_string())
        }
        "start_recording" => {
            let region = parse_region(args);
            let monitor_id = args
                .get("monitor_id")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            let fps = args.get("fps").and_then(Value::as_u64).unwrap_or(12) as u32;
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::StartRecording {
                    monitor_id,
                    region,
                    fps,
                    reply: tx,
                })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            let path = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(json!({"recording": true, "path": path}).to_string())
        }
        "stop_recording" => {
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::StopRecording { reply: tx })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            let path = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(json!({"recording": false, "path": path}).to_string())
        }
        "long_screenshot" => {
            let region = parse_region(args);
            let max_height = args
                .get("max_height")
                .and_then(Value::as_u64)
                .unwrap_or(20000) as u32;
            let monitor = match args.get("monitor_id").and_then(Value::as_u64) {
                Some(id) => crate::capture::find_monitor(id as u32).map_err(|e| e.to_string())?,
                None => crate::capture::monitor_at_cursor().map_err(|e| e.to_string())?,
            };
            let region = region
                .map(|r| {
                    ImgRect::from_min_size(
                        (r.x - monitor.rect.x) as f32,
                        (r.y - monitor.rect.y) as f32,
                        r.w as f32,
                        r.h as f32,
                    )
                    .clamped(monitor.rect.w, monitor.rect.h)
                })
                .unwrap_or_else(|| {
                    ImgRect::from_min_size(0.0, 0.0, monitor.rect.w as f32, monitor.rect.h as f32)
                });
            let job = crate::longshot::start(
                monitor.id,
                region,
                crate::longshot::LongShotOptions {
                    max_height,
                    ..Default::default()
                },
            );
            loop {
                match job.rx.recv_timeout(Duration::from_secs(90)) {
                    Ok(crate::longshot::LongShotUpdate::Finished(image)) => {
                        let options = crate::util::SaveOptions {
                            template: crate::naming::DEFAULT_TEMPLATE.to_string(),
                            quality: 90,
                            format: crate::util::ImageFormat::Png,
                            title: String::new(),
                            decorations: crate::decorate::Decorations::default(),
                        };
                        let path = crate::longshot::save(&image, save_dir, &options)
                            .map_err(|e| e.to_string())?;
                        return Ok(json!({
                            "path": path.display().to_string(),
                            "width": image.width(),
                            "height": image.height()
                        })
                        .to_string());
                    }
                    Ok(crate::longshot::LongShotUpdate::Failed(err)) => return Err(err),
                    Ok(_) => continue,
                    Err(_) => return Err("long screenshot timed out".into()),
                }
            }
        }
        "get_status" => {
            let (tx, rx) = std::sync::mpsc::channel();
            ui_tx
                .send(UiCommand::Status { reply: tx })
                .map_err(|_| "PinSight UI is not running".to_string())?;
            let status = rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "timed out waiting for the UI".to_string())??;
            Ok(json!({
                "version": status.version,
                "pins": status.pins,
                "recording": status.recording,
                "save_dir": status.save_dir,
                "mcp_requests": status.mcp_requests
            })
            .to_string())
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}

fn capture(
    ui_tx: &Sender<UiCommand>,
    monitor_id: Option<u32>,
    region: Option<PhysRect>,
    save: bool,
    return_base64: bool,
) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    ui_tx
        .send(UiCommand::Capture {
            monitor_id,
            region,
            save,
            return_base64,
            reply: tx,
        })
        .map_err(|_| "PinSight UI is not running".to_string())?;
    let captured = rx
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "timed out waiting for the UI to capture".to_string())??;
    Ok(json!({
        "path": captured.path.map(|p| p.display().to_string()),
        "width": captured.width,
        "height": captured.height,
        "base64": captured.base64
    }))
}

fn parse_region(args: &Value) -> Option<PhysRect> {
    Some(PhysRect {
        x: args.get("x")?.as_i64()? as i32,
        y: args.get("y")?.as_i64()? as i32,
        w: args.get("width")?.as_u64()? as u32,
        h: args.get("height")?.as_u64()? as u32,
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "capture_screenshot",
            "description": "Capture a full monitor. The PinSight window is moved out of the way first. Saves a PNG by default.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "monitor_id": {"type": "integer", "description": "xcap monitor id; defaults to the monitor under the cursor"},
                    "save": {"type": "boolean", "description": "Save a PNG to the PinSight output folder (default true)"},
                    "return_base64": {"type": "boolean", "description": "Include a base64 PNG in the result (default false)"}
                }
            }
        },
        {
            "name": "capture_region",
            "description": "Capture an absolute physical desktop rectangle. The PinSight window is moved out of the way first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "save": {"type": "boolean"},
                    "return_base64": {"type": "boolean"}
                },
                "required": ["x", "y", "width", "height"]
            }
        },
        {
            "name": "pin_image",
            "description": "Pin an image from a file path or a base64 PNG/JPEG on top of all windows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "base64": {"type": "string"}
                }
            }
        },
        {
            "name": "list_pins",
            "description": "List currently pinned images.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "close_pin",
            "description": "Close a pinned image by id.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "integer"}},
                "required": ["id"]
            }
        },
        {
            "name": "start_recording",
            "description": "Start recording a region to MP4 (when ffmpeg is installed) or GIF.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "monitor_id": {"type": "integer"},
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "fps": {"type": "integer", "minimum": 4, "maximum": 30}
                }
            }
        },
        {
            "name": "stop_recording",
            "description": "Stop the active recording and return the output path.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "long_screenshot",
            "description": "Capture a scrolling region and stitch the frames into one long image. Scrolls the content under the region.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "monitor_id": {"type": "integer"},
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "max_height": {"type": "integer"}
                }
            }
        },
        {
            "name": "get_status",
            "description": "Return PinSight version, pin count, recording state and output folder.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

#[allow(dead_code)]
fn _assert_send<T: Send>() {}
#[allow(dead_code)]
fn _assert_sync<T: Sync>() {}
const _: fn() = || {
    _assert_send::<UiCommand>();
    _assert_sync::<McpHandle>();
};
