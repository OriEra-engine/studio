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
            .nest_service("/projects", projects);

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
