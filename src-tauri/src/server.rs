//! 内嵌静态服务器：serve 引擎 demo 快照与用户生成的游戏项目。
//! demo 是纯静态 HTML/JS，因此零工具链即可在应用内运行。

use axum::{routing::get, Router};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tower_http::services::ServeDir;

/// 启动 127.0.0.1 随机端口静态服务器，返回端口号。
///
/// - `GET /health` → 探活
/// - `GET /examples/*` → 引擎快照 examples 目录
/// - `GET /projects/*` → 用户项目目录
/// - `GET /__provider.json` → 当前模型配置（同源注入，取代 webview initialization_script）
pub fn start(
    snapshot_examples: PathBuf,
    projects_dir: PathBuf,
    app: AppHandle,
) -> Result<u16, String> {
    let (tx, rx) = std::sync::mpsc::channel::<u16>();

    tauri::async_runtime::spawn(async move {
        let examples = ServeDir::new(&snapshot_examples).append_index_html_on_directories(true);
        let projects = ServeDir::new(&projects_dir).append_index_html_on_directories(true);
        let assets = ServeDir::new(projects_dir.parent().map(|p| p.join("assets")).unwrap_or_default())
            .append_index_html_on_directories(true);
        let app_for_provider = app.clone();
        let app_router = Router::new()
            .route(
                "/health",
                get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
            )
            .route(
                "/__provider.json",
                get(move || async move {
                    // 仅监听 127.0.0.1：与此前注入 localStorage 的信任域一致
                    let cfg = crate::settings::load_provider(&app_for_provider);
                    match cfg {
                        Some(c) => axum::Json(serde_json::json!({
                            "kind": c.kind, "key": c.key, "model": c.model, "base": c.base,
                        })),
                        None => axum::Json(serde_json::json!({"kind": "rule"})),
                    }
                }),
            )
            .nest_service("/examples", examples)
            .nest_service("/projects", projects)
            .nest_service("/assets", assets);

        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[server] bind failed: {e}");
                return;
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let _ = tx.send(port);
        if let Err(e) = axum::serve(listener, app_router).await {
            eprintln!("[server] stopped: {e}");
        }
    });

    rx.recv()
        .map_err(|e| format!("static server failed to start: {e}"))
}

/// 用户项目目录不存在则创建。
pub fn ensure_projects_dir(base: &Path) -> PathBuf {
    let dir = base.join("projects");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// ====================== 自定义协议（子窗口内容服务） ======================
//
// WebView2 运行时（Edge 155 实测）下，运行时创建的子窗口对 http://127.0.0.1:* 的
// 任何导航（wry 初始导航 / navigate() / 页面内 location.href）都会被静默吞掉，
// 唯一在本机验证可渲染的路径是主窗口所走的自定义协议（*.localhost）。
// 因此游戏/Demo 子窗口改走 http://oriera.localhost/...，由这里直接读盘服务。

use std::borrow::Cow;
use tauri::Manager;

fn mime_of(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        "mp4" => "video/mp4",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(status: u16, mime: &str, bytes: Vec<u8>) -> tauri::http::Response<Cow<'static, [u8]>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(Cow::Owned(bytes))
        .unwrap()
}

/// 自定义协议内容服务：/__provider.json、/examples/*（引擎快照）、/projects/*（用户项目）。
pub fn serve_content(
    app: &tauri::AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let state = app.state::<crate::AppState>();
    let path = request.uri().path().to_string();

    if path == "/__provider.json" {
        let cfg = crate::settings::load_provider(app);
        let body = match cfg {
            Some(c) => serde_json::json!({
                "kind": c.kind, "key": c.key, "model": c.model, "base": c.base,
            }),
            None => serde_json::json!({"kind": "rule"}),
        };
        return respond(200, "application/json; charset=utf-8", serde_json::to_vec(&body).unwrap());
    }

    let (root, rel) = if let Some(r) = path.strip_prefix("/examples/") {
        (state.snapshot_examples.clone(), r.to_string())
    } else if let Some(r) = path.strip_prefix("/projects/") {
        (state.projects_dir.clone(), r.to_string())
    } else {
        return respond(404, "text/plain; charset=utf-8", b"not found".to_vec());
    };

    // 防路径穿越（尾斜杠产生的空段无害，保留以支持目录 index.html）
    let full = root.join(&rel);
    if rel.split('/').any(|seg| seg == "..") || full.components().any(|c| c.as_os_str().to_string_lossy() == "..") {
        return respond(404, "text/plain; charset=utf-8", b"not found".to_vec());
    }

    let target = if full.is_dir() { full.join("index.html") } else { full };
    match std::fs::read(&target) {
        Ok(bytes) => respond(200, mime_of(&target), bytes),
        Err(_) => respond(404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}
