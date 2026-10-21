//! OriEra Studio —— 开箱即用的 OriEra 世界引擎桌面工作台。

mod demos;
mod feedback;
mod hardware;
mod llm;
mod ollama;
mod project;
mod recommend;
mod server;
mod settings;
mod sse;
mod templates;

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::Manager;

/// 引擎快照版本（随安装包内置；更新走 gitee 镜像，M5 实现）。
const SNAPSHOT_VERSION: &str = "oriera-engine v0.3.0 (examples 快照)";

pub struct AppState {
    pub port: u16,
    pub snapshot_examples: std::path::PathBuf,
    pub projects_dir: std::path::PathBuf,
}

#[derive(Serialize)]
pub struct ServerInfo {
    pub port: u16,
    pub snapshot_version: String,
    pub snapshot_dir: String,
    pub projects_dir: String,
}

#[derive(Serialize)]
pub struct AppStatus {
    pub server: ServerInfo,
    pub provider_configured: bool,
    pub provider: Option<settings::ProviderCfg>,
    pub project_count: usize,
}

/// 解析引擎快照目录：安装态取资源目录，开发态取仓库内 engine-snapshot/examples。
fn resolve_snapshot_examples(app: &tauri::App) -> std::path::PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let packaged = resource_dir.join("engine-snapshot").join("examples");
        if packaged.is_dir() {
            return packaged;
        }
    }
    let dev = std::env::current_dir()
        .map(|d| d.join("engine-snapshot").join("examples"))
        .unwrap_or_default();
    if dev.is_dir() {
        return dev;
    }
    std::path::PathBuf::new()
}

#[tauri::command]
fn get_status(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> AppStatus {
    let provider = settings::load_provider(&app);
    let project_count = std::fs::read_dir(&state.projects_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("index.html").is_file())
                .count()
        })
        .unwrap_or(0);
    AppStatus {
        server: ServerInfo {
            port: state.port,
            snapshot_version: SNAPSHOT_VERSION.to_string(),
            snapshot_dir: state.snapshot_examples.to_string_lossy().into_owned(),
            projects_dir: state.projects_dir.to_string_lossy().into_owned(),
        },
        provider_configured: provider.is_some(),
        provider,
        project_count,
    }
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> settings::Settings {
    settings::load(&app)
}

#[tauri::command]
fn save_provider(app: tauri::AppHandle, cfg: settings::ProviderCfg) -> Result<(), String> {
    settings::save_provider(&app, &cfg)?;
    // 模型热切换：重载所有打开的 demo/项目窗口（初始化脚本随导航重新注入）
    reload_runtime_windows(&app);
    Ok(())
}

#[tauri::command]
fn clear_provider(app: tauri::AppHandle) -> Result<(), String> {
    settings::clear_provider(&app)?;
    reload_runtime_windows(&app);
    Ok(())
}

#[tauri::command]
fn set_developer_mode(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let mut s = settings::load(&app);
    s.developer_mode = enabled;
    settings::save(&app, &s)
}

fn reload_runtime_windows(app: &tauri::AppHandle) {
    for (label, win) in app.webview_windows() {
        if label.starts_with("demo-") || label.starts_with("project-") {
            let _ = win.eval("location.reload()");
        }
    }
}

#[tauri::command]
fn detect_hardware() -> hardware::HardwareInfo {
    hardware::detect()
}

#[tauri::command]
fn get_recommendations(hw: hardware::HardwareInfo) -> recommend::Recommendation {
    recommend::recommend(&hw)
}

/// 进行中的 AI 流式任务（task id → 取消旗标）。前端 cancel_ai 置位，流在下个增量边界退出。
#[derive(Default)]
pub struct AiTasks(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

fn next_task_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("ai-{n:x}")
}

/// 推送给前端的 AI 流事件（tauri Channel）。
#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AiStreamEvent {
    Delta { text: String },
    Done { result: serde_json::Value },
    Error { message: String },
}

/// 用当前 provider 发起一次最小流式对话，验证 key/端点可用（流式顺便验证 SSE 通道）。
#[tauri::command]
async fn test_provider(app: tauri::AppHandle) -> Result<String, String> {
    let cfg = settings::load_provider(&app)
        .ok_or_else(|| "请先填写 API Key（或选择 Ollama 本地方案）".to_string())?;
    let mut seen = 0usize;
    let mut on_delta = |d: &str| {
        seen += d.chars().count();
        seen < 60
    };
    let reply = llm::chat_stream(
        &cfg,
        vec![
            serde_json::json!({"role": "system", "content": "你是连通性测试助手。"}),
            serde_json::json!({"role": "user", "content": "请回复：连接成功"}),
        ],
        40,
        0.7,
        &mut on_delta,
        &AtomicBool::new(false),
    )
    .await?;
    Ok(reply.chars().take(80).collect())
}

/// AI 剧情定制（M4 向导第二步「AI 生成」按钮）。framework_rhythm 传叙事框架节奏，None 回退三幕式。
#[tauri::command]
async fn generate_story(
    app: tauri::AppHandle,
    gameplay_name: String,
    tagline: String,
    user_idea: String,
    beat_count: usize,
    framework_rhythm: Option<String>,
    // 玩法模板的经济资源名（v0.3 分支效果提示词用；旧前端不传也能跑）
    economy: Option<Vec<serde_json::Value>>,
) -> Result<llm::GeneratedStory, String> {
    let cfg = settings::load_provider(&app)
        .ok_or_else(|| "AI 定制需要先在模型中心配置模型".to_string())?;
    let economy_names: Vec<String> = economy
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v["name"].as_str().map(String::from))
        .collect();
    llm::generate_story(
        &cfg,
        &gameplay_name,
        &tagline,
        &user_idea,
        beat_count,
        framework_rhythm.as_deref(),
        &economy_names,
    )
    .await
}

/// Studio 助手「奥粒」多轮对话（前端 CopilotPanel 调用）。
#[tauri::command]
async fn copilot_chat(
    app: tauri::AppHandle,
    system: String,
    history: Vec<llm::ChatTurn>,
    user: String,
) -> Result<String, String> {
    let cfg = settings::load_provider(&app).ok_or_else(|| {
        "小奥离线中：先在「模型中心」配置模型（云端或 Ollama 本地），配置好我就能真正对话啦"
            .to_string()
    })?;
    llm::copilot_chat(&cfg, &system, &history, &user).await
}

/// 流式 AI 剧情定制：立即返回 task id，增量经 Channel 推送（delta/done/error）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn generate_story_stream(
    app: tauri::AppHandle,
    tasks: tauri::State<'_, AiTasks>,
    gameplay_name: String,
    tagline: String,
    user_idea: String,
    beat_count: usize,
    framework_rhythm: Option<String>,
    economy: Option<Vec<serde_json::Value>>,
    channel: Channel<AiStreamEvent>,
) -> Result<String, String> {
    let cfg = settings::load_provider(&app)
        .ok_or_else(|| "AI 生成需要先配置模型（第 3 步内即可配置）".to_string())?;
    let economy_names: Vec<String> = economy
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v["name"].as_str().map(String::from))
        .collect();

    let flag = Arc::new(AtomicBool::new(false));
    let id = next_task_id();
    tasks.0.lock().unwrap().insert(id.clone(), flag.clone());

    let ch = channel.clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let mut on_delta = |d: &str| {
            let _ = ch.send(AiStreamEvent::Delta { text: d.to_string() });
            true
        };
        let result = llm::generate_story_stream(
            &cfg,
            &gameplay_name,
            &tagline,
            &user_idea,
            beat_count,
            framework_rhythm.as_deref(),
            &economy_names,
            &mut on_delta,
            &flag,
        )
        .await;
        if flag.load(Ordering::Relaxed) {
            let _ = channel.send(AiStreamEvent::Error { message: "已取消".into() });
        } else {
            match result {
                Ok(story) => {
                    let _ = channel.send(AiStreamEvent::Done {
                        result: serde_json::to_value(&story).unwrap_or_default(),
                    });
                }
                Err(e) => {
                    let _ = channel.send(AiStreamEvent::Error { message: e });
                }
            }
        }
        if let Some(t) = app.try_state::<AiTasks>() {
            t.0.lock().unwrap().remove(&task_id);
        }
    });
    Ok(id)
}

/// 流式奥粒对话：同 generate_story_stream 的事件协议，Done.result = {"text": ...}。
#[tauri::command]
async fn copilot_chat_stream(
    app: tauri::AppHandle,
    tasks: tauri::State<'_, AiTasks>,
    system: String,
    history: Vec<llm::ChatTurn>,
    user: String,
    channel: Channel<AiStreamEvent>,
) -> Result<String, String> {
    let cfg = settings::load_provider(&app).ok_or_else(|| {
        "小奥离线中：先在「模型中心」配置模型（云端或 Ollama 本地），配置好我就能真正对话啦"
            .to_string()
    })?;

    let flag = Arc::new(AtomicBool::new(false));
    let id = next_task_id();
    tasks.0.lock().unwrap().insert(id.clone(), flag.clone());

    let ch = channel.clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let mut on_delta = |d: &str| {
            let _ = ch.send(AiStreamEvent::Delta { text: d.to_string() });
            true
        };
        let result = llm::copilot_chat_stream(&cfg, &system, &history, &user, &mut on_delta, &flag)
            .await;
        if flag.load(Ordering::Relaxed) {
            let _ = channel.send(AiStreamEvent::Error { message: "已取消".into() });
        } else {
            match result {
                Ok(text) => {
                    let _ = channel.send(AiStreamEvent::Done {
                        result: serde_json::json!({ "text": text }),
                    });
                }
                Err(e) => {
                    let _ = channel.send(AiStreamEvent::Error { message: e });
                }
            }
        }
        if let Some(t) = app.try_state::<AiTasks>() {
            t.0.lock().unwrap().remove(&task_id);
        }
    });
    Ok(id)
}

/// 取消进行中的 AI 流式任务。
#[tauri::command]
fn cancel_ai(tasks: tauri::State<'_, AiTasks>, id: String) -> Result<(), String> {
    if let Some(f) = tasks.0.lock().unwrap().get(&id) {
        f.store(true, Ordering::Relaxed);
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let snapshot_examples = resolve_snapshot_examples(app);
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("cannot resolve app data dir");
            let projects_dir = server::ensure_projects_dir(&data_dir);

            let port = server::start(snapshot_examples.clone(), projects_dir.clone())
                .unwrap_or_else(|e| panic!("static server: {e}"));

            app.manage(AppState {
                port,
                snapshot_examples,
                projects_dir,
            });
            app.manage(AiTasks::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_settings,
            save_provider,
            clear_provider,
            set_developer_mode,
            detect_hardware,
            get_recommendations,
            test_provider,
            generate_story,
            generate_story_stream,
            copilot_chat,
            copilot_chat_stream,
            cancel_ai,
            demos::list_demos,
            demos::open_demo,
            ollama::ollama_status,
            ollama::ollama_serve,
            ollama::ollama_pull,
            ollama::ollama_delete_model,
            ollama::gpu_vram_usage,
            project::create_project,
            project::list_projects,
            project::open_project,
            project::delete_project,
            project::reveal_project_dir,
            project::read_project,
            project::update_story,
            project::preview_regenerate_beat,
            project::apply_beat,
            project::upgrade_runtime,
            project::export_project_zip,
            project::running_projects,
            templates::list_templates,
            templates::import_gameplay_pack,
            feedback::submit_feedback,
            feedback::has_feedback_pat,
            feedback::save_feedback_pat,
            feedback::clear_feedback_pat,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OriEra Studio");
}
