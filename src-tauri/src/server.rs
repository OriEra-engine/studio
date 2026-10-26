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

/// Host 白名单（v0.7.5 安全加固）：本机服务只应被本机源（127.0.0.1/localhost，
/// 去端口比较）与自定义协议域（oriera.localhost）访问。伪造 Host 的远程网页
/// （DNS rebinding）会被 403；无 Host 头的非浏览器客户端（curl 等本机进程）
/// 放行——CORS/Host 防线只对浏览器有意义。
pub fn host_in_allowlist(host: &str) -> bool {
    let bare = host.trim().to_ascii_lowercase();
    let bare = bare.split(':').next().unwrap_or("");
    matches!(bare, "127.0.0.1" | "localhost" | "oriera.localhost")
}

/// CORS 回显白名单（v0.7.5 安全加固）：代理响应不再 `Access-Control-Allow-Origin: *`
///（任意网页可跨源读本机代理），只对同机源回显 Origin；带 Host 白名单外的
/// Origin 一律不给 ACAO 头，浏览器将拦截响应体读取（curl 等无 Origin 不受影响）。
pub fn cors_allow_origin(origin: Option<&str>) -> Option<String> {
    let origin = origin?;
    let o = origin.trim().to_ascii_lowercase();
    let allowed = o.starts_with("http://127.0.0.1:")
        || o.starts_with("http://localhost:")
        || o == "http://oriera.localhost";
    allowed.then(|| origin.to_string())
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
    // v0.7.5：伪造 Host（DNS rebinding）直接 403。
    if let Some(h) = headers.get("host").and_then(|v| v.to_str().ok()) {
        if !host_in_allowlist(h) {
            return (StatusCode::FORBIDDEN, "forbidden host").into_response();
        }
    }
    let cors = cors_allow_origin(headers.get("origin").and_then(|v| v.to_str().ok()));
    if method == Method::OPTIONS {
        let mut pre = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(
                "Access-Control-Allow-Methods",
                "GET, POST, PUT, DELETE, OPTIONS",
            )
            .header(
                "Access-Control-Allow-Headers",
                "authorization, content-type, accept, x-api-key, anthropic-version",
            )
            .header("Access-Control-Max-Age", "86400")
            .header("Vary", "Origin");
        if let Some(o) = &cors {
            pre = pre.header("Access-Control-Allow-Origin", o);
        }
        return pre
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
            let mut resp = Response::builder().status(status).header("Vary", "Origin");
            if let Some(o) = &cors {
                resp = resp.header("Access-Control-Allow-Origin", o);
            }
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
    let demos = ServeDir::new(projects_dir.parent().map(|p| p.join("demos")).unwrap_or_default())
        .append_index_html_on_directories(true);
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
            get(move |headers: HeaderMap| async move {
                // v0.7.5：Host 白名单校验——伪造 Host 的远程网页（DNS rebinding）
                // 扫不到 key。无 Host 的本机非浏览器客户端不受影响。
                if let Some(h) = headers.get("host").and_then(|v| v.to_str().ok()) {
                    if !host_in_allowlist(h) {
                        return (
                            StatusCode::FORBIDDEN,
                            axum::Json(serde_json::json!({"error": "forbidden host"})),
                        )
                            .into_response();
                    }
                }
                // 仅监听 127.0.0.1：与此前注入 localStorage 的信任域一致。
                // 白名单网关 base 在出口改写为同源代理（游戏免 CORS）。
                let cfg = crate::settings::load_provider(&app_for_provider);
                // 含 key 的响应禁止进任何缓存（2026-10-26 安全测评 P1）：
                // axum Json 出口此前没设 no-store，响应可能落 WebView2
                // 磁盘缓存，本机任意读缓存进程即可拾取 key。
                let mut resp = match cfg {
                    Some(c) => axum::Json(serde_json::json!({
                        "kind": c.kind, "key": c.key, "model": c.model,
                        "base": localize_base(&c.base, port),
                    }))
                    .into_response(),
                    None => axum::Json(serde_json::json!({"kind": "rule"})).into_response(),
                };
                resp.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("no-store"),
                );
                resp
            }),
        )
        .nest_service("/examples", examples)
        .nest_service("/demos", demos)
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

fn respond(
    status: u16,
    mime: &str,
    cache: &str,
    bytes: Vec<u8>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", cache)
        .body(Cow::Owned(bytes))
        .unwrap()
}

/// 缓存分型（v0.7.5）：大体积不可变资产（模型/贴图/音频/字体）允许浏览器
/// 缓存 1 小时，避免每次刷新重拉 1.3MB three.js 与 GLB；html/js/css/json
/// 迭代频繁，保持 no-store 保证改动即时可见。
fn cache_control_of(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "glb" | "gltf" | "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "wasm" | "mp3"
        | "ogg" | "wav" | "ttf" | "woff2" | "mp4" => "public, max-age=3600",
        _ => "no-store",
    }
}

fn not_found() -> tauri::http::Response<Cow<'static, [u8]>> {
    respond(404, "text/plain; charset=utf-8", "no-store", b"not found".to_vec())
}

/// 把 URL 相对路径安全解析到 root 之下（v0.7.5 安全加固）。
///
/// Windows 上 `root.join("C:/Users/...")` 会用盘符绝对路径**整体替换** root，
/// 原有两道 `..` 检查都拦不住这种无 `..` 的形态（任意文件读）。这里在
/// canonicalize 终检：解析后的真实路径必须仍在 root 之下才放行。
fn resolve_under_root(root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.split('/').any(|seg| seg == "..") {
        return None;
    }
    let full = root.join(rel);
    if full
        .components()
        .any(|c| c.as_os_str().to_string_lossy() == "..")
    {
        return None;
    }
    let target = if full.is_dir() { full.join("index.html") } else { full };
    let canonical = target.canonicalize().ok()?;
    let root_canonical = root.canonicalize().ok()?;
    if canonical.starts_with(&root_canonical) {
        Some(canonical)
    } else {
        None
    }
}

/// 自定义协议内容服务：/__provider.json、/examples/*（引擎快照）、/projects/*（用户项目）。
pub fn serve_content(
    app: &tauri::AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let state = app.state::<crate::AppState>();
    let path = request.uri().path().to_string();

    if path == "/__provider.json" {
        // v0.7.5：与 axum 出口同规则——Host 白名单外（伪造 Host 的远程网页）
        // 一律 403，防止扫描/枚举端口读走明文 key。
        if let Some(h) = request.uri().host().or_else(|| {
            request
                .headers()
                .get("host")
                .and_then(|v| v.to_str().ok())
        }) {
            if !host_in_allowlist(h) {
                return respond(
                    403,
                    "application/json; charset=utf-8",
                    "no-store",
                    br#"{"error":"forbidden host"}"#.to_vec(),
                );
            }
        }
        let cfg = crate::settings::load_provider(app);
        let body = match cfg {
            Some(c) => serde_json::json!({
                "kind": c.kind, "key": c.key, "model": c.model,
                // 与 axum 出口一致：白名单网关 base 改写为同源代理。
                // 自定义协议域（oriera.localhost）跨源访问 127.0.0.1 代理不受
                // 浏览器 CORS 限制（代理响应回显 oriera.localhost 源），指向即可。
                "base": localize_base(&c.base, state.port),
            }),
            None => serde_json::json!({"kind": "rule"}),
        };
        return respond(
            200,
            "application/json; charset=utf-8",
            "no-store",
            serde_json::to_vec(&body).unwrap(),
        );
    }

    let (root, rel) = if let Some(r) = path.strip_prefix("/examples/") {
        (state.snapshot_examples.clone(), r.to_string())
    } else if let Some(r) = path.strip_prefix("/projects/") {
        (state.projects_dir.clone(), r.to_string())
    } else if let Some(r) = path.strip_prefix("/demos/") {
        // 用户上传 demo（app_data/demos，与 projects 平级）
        (
            state
                .projects_dir
                .parent()
                .map(|p| p.join("demos"))
                .unwrap_or_default(),
            r.to_string(),
        )
    } else {
        return not_found();
    };

    let Some(target) = resolve_under_root(&root, &rel) else {
        return not_found();
    };
    match std::fs::read(&target) {
        Ok(bytes) => respond(200, mime_of(&target), cache_control_of(&target), bytes),
        Err(_) => not_found(),
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

    #[test]
    fn host_allowlist_blocks_remote_and_rebinding() {
        assert!(host_in_allowlist("127.0.0.1:52340"));
        assert!(host_in_allowlist("localhost:8080"));
        assert!(host_in_allowlist("oriera.localhost"));
        assert!(host_in_allowlist("oriera.localhost:80"));
        // DNS rebinding / 伪造 Host / 大小写混淆
        assert!(!host_in_allowlist("evil.com"));
        assert!(!host_in_allowlist("evil.com:52340"));
        assert!(!host_in_allowlist("ORIERA.LOCALHOST.evil.com"));
        assert!(!host_in_allowlist("oriera.localhost@evil.com"));
        assert!(!host_in_allowlist("[::1]:52340"));
    }

    #[test]
    fn cors_only_echoes_local_origins() {
        assert_eq!(
            cors_allow_origin(Some("http://oriera.localhost")).as_deref(),
            Some("http://oriera.localhost")
        );
        assert_eq!(
            cors_allow_origin(Some("http://127.0.0.1:52340")).as_deref(),
            Some("http://127.0.0.1:52340")
        );
        assert_eq!(cors_allow_origin(Some("https://evil.com")), None);
        assert_eq!(cors_allow_origin(None), None);
    }

    /// 在系统临时目录搭一个 root/sub/index.html 骨架，跑穿越探针矩阵。
    #[test]
    fn resolve_under_root_blocks_traversal_and_absolute() {
        let root = std::env::temp_dir().join(format!(
            "oriera-server-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("index.html"), b"ok").unwrap();

        // 正常相对路径（含目录 index 展开）放行
        let ok = resolve_under_root(&root, "sub/index.html").expect("relative ok");
        assert!(ok.ends_with("index.html"));
        assert!(resolve_under_root(&root, "sub").is_some(), "dir -> index.html");

        // Windows 盘符绝对路径整体替换 root —— v0.7.5 修复的核心探针
        assert!(resolve_under_root(&root, "C:/Windows/win.ini").is_none());
        assert!(resolve_under_root(&root, "C:\\Windows\\win.ini").is_none());
        assert!(
            resolve_under_root(&root, "//etc/passwd").is_none(),
            "UNC/根相对形态"
        );
        // 经典 .. 穿越（正反斜杠）
        assert!(resolve_under_root(&root, "sub/../../secret").is_none());
        assert!(resolve_under_root(&root, "sub\\..\\..\\secret").is_none());
        // root 外真实存在的文件（canonicalize 终检兜底）
        let outside = std::env::temp_dir().join("oriera-server-test-outside.txt");
        std::fs::write(&outside, b"x").unwrap();
        let outside_str = outside.to_string_lossy().replace('\\', "/");
        assert!(
            resolve_under_root(&root, &outside_str).is_none(),
            "root 外真实文件必须 404"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn cache_control_splits_immutable_assets() {
        use std::path::PathBuf as P;
        assert_eq!(cache_control_of(&P::from("a/b.glb")), "public, max-age=3600");
        assert_eq!(cache_control_of(&P::from("a/b.PNG")), "public, max-age=3600");
        assert_eq!(cache_control_of(&P::from("index.html")), "no-store");
        assert_eq!(cache_control_of(&P::from("engine/kernel.js")), "no-store");
        assert_eq!(cache_control_of(&P::from("data.json")), "no-store");
    }
}
