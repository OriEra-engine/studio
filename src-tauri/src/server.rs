//! 内嵌静态服务器：serve 引擎 demo 快照与用户生成的游戏项目。
//! demo 是纯静态 HTML/JS，因此零工具链即可在应用内运行。
//!
//! v0.7.1：`/__provider.json`（同源注入，取代 webview initialization_script）
//! + 自定义协议内容服务（oriera.localhost，新版 WebView2 子窗口唯一可渲染路径）。
//!
//! v0.8：新增 `/proxy/{alias}/*` 白名单网关透传——浏览器/WebView 里的游戏
//! 跨域调用云端网关会被 CORS 拦截（如 XLite 未返回 ACAO 头），本机同源代理
//! 一次性解决；`localize_base()` 在 `/__provider.json` 两个出口（axum 路由
//! 与自定义协议）统一改写 base，游戏侧零感知。

use axum::body::Body;
use axum::extract::{OriginalUri, Path as AxumPath};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::AppHandle;
use tower_http::services::ServeDir;

/// 代理白名单：alias → 上游网关源站。未知 alias 一律 404（不开放任意转发）。
const PROXY_UPSTREAMS: &[(&str, &str)] = &[("xlite", "https://xlite.dezai.com")];

/// 若 base 指向白名单内的网关主机，改写为本机同源代理地址（游戏 WebView 免
/// CORS）；其他端点原样返回（智谱/DeepSeek 等自带 CORS 头的厂商不需要代理）。
pub fn localize_base(base: &str, port: u16) -> String {
    for (alias, upstream) in PROXY_UPSTREAMS {
        let host = upstream
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        if let Some((_, path)) = base.split_once(host) {
            return format!("http://127.0.0.1:{port}/proxy/{alias}{path}");
        }
    }
    base.to_string()
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder().build().unwrap_or_default())
}

/// 透传处理器：转发方法/查询串/鉴权与内容头/请求体，回传状态码、
/// Content-Type 与（流式）响应体——SSE 逐块转发，不缓冲。
/// 响应带 `Access-Control-Allow-Origin: *` 并显式应答 OPTIONS 预检：自定义
/// 协议子窗口（oriera.localhost）访问本代理是跨源调用，网关密钥由调用方
/// 自带（代理不存任何凭据），放开 Origin 与直连网关等价。
async fn proxy_passthrough(
    AxumPath((alias, path)): AxumPath<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if method == Method::OPTIONS {
        return Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header("Access-Control-Allow-Origin", "*")
            .header(
                "Access-Control-Allow-Methods",
                "GET, POST, PUT, DELETE, OPTIONS",
            )
            .header(
                "Access-Control-Allow-Headers",
                "authorization, content-type, accept, x-api-key, anthropic-version",
            )
            .header("Access-Control-Max-Age", "86400")
            .body(Body::empty())
            .unwrap_or_else(|_| (StatusCode::BAD_GATEWAY, "preflight error").into_response());
    }
    let Some((_, upstream)) = PROXY_UPSTREAMS.iter().find(|(a, _)| *a == alias) else {
        return (StatusCode::NOT_FOUND, "unknown proxy alias").into_response();
    };
    let url = format!("{upstream}/{path}{}", uri.query().map(|q| format!("?{q}")).unwrap_or_default());

    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::POST);
    let mut req = http_client().request(method, &url).body(body);
    // 只转发语义头；Host/Cookie 等不转（上游按 Bearer 鉴权）
    for name in ["authorization", "content-type", "accept", "x-api-key", "anthropic-version"] {
        if let Some(v) = headers.get(name) {
            req = req.header(name, v.as_bytes());
        }
    }

    match req.send().await {
        Ok(up) => {
            let status =
                StatusCode::from_u16(up.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut resp = Response::builder()
                .status(status)
                .header("Access-Control-Allow-Origin", "*");
            if let Some(ct) = up.headers().get(reqwest::header::CONTENT_TYPE).cloned() {
                resp = resp.header("content-type", ct.as_bytes());
            }
            resp.body(Body::from_stream(up.bytes_stream()))
                .unwrap_or_else(|_| (StatusCode::BAD_GATEWAY, "proxy body error").into_response())
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("upstream error: {e}"),
        )
            .into_response(),
    }
}

/// 组装完整路由（静态服务 + 网关代理）。port 供 `__provider.json` 出口改写
/// 白名单网关 base（先绑监听拿端口再建路由，见 `start`）。
fn build_router(
    snapshot_examples: PathBuf,
    projects_dir: PathBuf,
    port: u16,
    app: AppHandle,
) -> Router {
    let examples = ServeDir::new(snapshot_examples).append_index_html_on_directories(true);
    let assets = ServeDir::new(projects_dir.parent().map(|p| p.join("assets")).unwrap_or_default())
        .append_index_html_on_directories(true);
    let projects = ServeDir::new(projects_dir).append_index_html_on_directories(true);
    let app_for_provider = app.clone();
    Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
        )
        .route(
            "/__provider.json",
            get(move || async move {
                // 仅监听 127.0.0.1：与此前注入 localStorage 的信任域一致。
                // 白名单网关 base 在出口改写为同源代理（游戏免 CORS）。
                let cfg = crate::settings::load_provider(&app_for_provider);
                match cfg {
                    Some(c) => axum::Json(serde_json::json!({
                        "kind": c.kind, "key": c.key, "model": c.model,
                        "base": localize_base(&c.base, port),
                    })),
                    None => axum::Json(serde_json::json!({"kind": "rule"})),
                }
            }),
        )
        .nest_service("/examples", examples)
        .nest_service("/assets", assets)
        .nest_service("/projects", projects)
        .route("/proxy/{alias}/{*path}", any(proxy_passthrough))
}

/// 启动 127.0.0.1 随机端口静态服务器，返回端口号。
///
/// - `GET /health` → 探活
/// - `GET /examples/*` → 引擎快照 examples 目录
/// - `GET /projects/*` → 用户项目目录
/// - `GET /__provider.json` → 当前模型配置（同源注入；白名单网关 base 已本地化）
/// - `ANY /proxy/{alias}/*` → 白名单网关透传（免游戏 WebView CORS）
pub fn start(
    snapshot_examples: PathBuf,
    projects_dir: PathBuf,
    app: AppHandle,
) -> Result<u16, String> {
    let (tx, rx) = std::sync::mpsc::channel::<u16>();

    tauri::async_runtime::spawn(async move {
        // 先绑监听拿端口，再建路由：__provider.json 出口改写 base 需要端口。
        let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[server] bind failed: {e}");
                return;
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let _ = tx.send(port);
        let app_router = build_router(snapshot_examples, projects_dir, port, app);
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
                "kind": c.kind, "key": c.key, "model": c.model,
                // 与 axum 出口一致：白名单网关 base 改写为同源代理。
                // 自定义协议域（oriera.localhost）跨源访问 127.0.0.1 代理不受
                // 浏览器 CORS 限制（代理响应不校验 Origin），指向即可。
                "base": localize_base(&c.base, state.port),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localize_base_rewrites_whitelisted_only() {
        let port = 12345u16;
        // 白名单主机：https 与带路径都改写为同源代理
        assert_eq!(
            localize_base("https://xlite.dezai.com/v1", port),
            "http://127.0.0.1:12345/proxy/xlite/v1"
        );
        // 非白名单端点原样保留
        assert_eq!(
            localize_base("https://open.bigmodel.cn/api/paas/v4", port),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            localize_base("http://127.0.0.1:11434/v1", port),
            "http://127.0.0.1:11434/v1"
        );
    }
}
