//! OriEra Studio —— 开箱即用的 OriEra 世界引擎桌面工作台。

mod assets;
mod canvas;
mod demos;
mod feedback;
mod film_flow;
mod hardware;
mod llm;
mod ollama;
mod pet;
mod project;
mod recommend;
mod server;
mod settings;
mod sim;
mod singlefile;
mod sse;
mod templates;

use serde::Serialize;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::Manager;

/// 引擎快照版本：从同步器生成的 VERSION.json 读取（engine commit + 同步日期）。
fn snapshot_version(snapshot_examples: &std::path::Path) -> String {
    snapshot_examples
        .parent()
        .and_then(|p| std::fs::read_to_string(p.join("VERSION.json")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| {
            let engine = v["engine"].as_str().unwrap_or("?");
            let synced = v["syncedAt"]
                .as_str()
                .map(|s| s.chars().take(10).collect::<String>())
                .unwrap_or_default();
            format!("oriera-engine {engine} (快照同步于 {synced})")
        })
        .unwrap_or_else(|| "oriera-engine (快照)".into())
}

pub struct AppState {
    pub port: u16,
    pub snapshot_examples: std::path::PathBuf,
    pub projects_dir: std::path::PathBuf,
    pub snapshot_version: String,
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

/// 当月 AI 调用账本（copilot/story 次数）——2026-10-26 付费用户测评：
/// 消耗必须可见。只记次数不折价，价目在供应商侧。
#[tauri::command]
fn chat_ledger_get() -> serde_json::Map<String, serde_json::Value> {
    llm::month_calls()
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
            snapshot_version: state.snapshot_version.clone(),
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

/// 硬件体检走异步线程：PowerShell CIM 冷启动 2-5 秒，同步跑在主线程会把整个
/// UI 冻住（0.9.0 实测模型中心进页即卡）。
#[tauri::command]
async fn detect_hardware() -> hardware::HardwareInfo {
    tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .unwrap_or_default()
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
/// 测试路径=运行路径：白名单网关（XLite）经本机代理测试——直连测试通过
/// 不等于浏览器游戏可用（CORS），别让「测试连接」骗用户。
#[tauri::command]
async fn test_provider(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let mut cfg = settings::load_provider(&app)
        .ok_or_else(|| "请先填写 API Key（或选择 Ollama 本地方案）".to_string())?;
    cfg.base = server::localize_base(&cfg.base, state.port);
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
    let out = llm::copilot_chat(&cfg, &system, &history, &user).await;
    if out.is_ok() {
        llm::record_call("copilot");
    }
    out
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
            let _ = ch.send(AiStreamEvent::Delta {
                text: d.to_string(),
            });
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
            let _ = channel.send(AiStreamEvent::Error {
                message: "已取消".into(),
            });
        } else {
            match result {
                Ok(story) => {
                    llm::record_call("story");
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
            let _ = ch.send(AiStreamEvent::Delta {
                text: d.to_string(),
            });
            true
        };
        let result =
            llm::copilot_chat_stream(&cfg, &system, &history, &user, &mut on_delta, &flag).await;
        if flag.load(Ordering::Relaxed) {
            let _ = channel.send(AiStreamEvent::Error {
                message: "已取消".into(),
            });
        } else {
            match result {
                Ok(text) => {
                    llm::record_call("copilot");
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
        .register_uri_scheme_protocol("oriera", |ctx, request| {
            #[cfg(debug_assertions)]
            println!("[oriera-proto] {}", request.uri());
            crate::server::serve_content(ctx.app_handle(), request)
        })
        .setup(|app| {
            let snapshot_examples = resolve_snapshot_examples(app);
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("cannot resolve app data dir");
            let projects_dir = server::ensure_projects_dir(&data_dir);
            llm::init_ledger(data_dir.join("chat_ledger.json"));

            let port = server::start(
                snapshot_examples.clone(),
                projects_dir.clone(),
                app.handle().clone(),
            )
            .unwrap_or_else(|e| panic!("static server: {e}"));

            let snapshot_version = snapshot_version(&snapshot_examples);
            app.manage(AppState {
                port,
                snapshot_examples,
                projects_dir,
                snapshot_version,
            });
            app.manage(AiTasks::default());

            // 火绒兼容提醒（只弹一次）：其浏览器防护可能拦截游戏子窗口
            {
                let app2 = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let mut s = crate::settings::load(&app2);
                    if s.huorong_hint_shown {
                        return;
                    }
                    let out = std::process::Command::new("tasklist")
                        .args(["/FI", "IMAGENAME eq usysdiag.exe", "/NH"])
                        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                        .output();
                    let huorong = out
                        .map(|o| String::from_utf8_lossy(&o.stdout).contains("usysdiag"))
                        .unwrap_or(false);
                    if !huorong {
                        return;
                    }
                    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
                    app2.dialog()
                        .message(
                            "检测到火绒安全软件正在运行。

火绒的浏览器防护可能拦截游戏窗口（表现为白屏）。
建议：打开火绒 → 信任区 → 添加本程序；
或直接使用项目卡片上的「🌐 浏览器」游玩（不受影响）。",
                        )
                        .title("OriEra Studio · 火绒兼容提示")
                        .kind(MessageDialogKind::Warning)
                        .buttons(MessageDialogButtons::Ok)
                        .blocking_show();
                    s.huorong_hint_shown = true;
                    let _ = crate::settings::save(&app2, &s);
                });
            }

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
            pet::open_pet,
            pet::pet_start_drag,
            pet::pet_close,
            pet::pet_chat,
            settings::secret_save,
            settings::secret_has,
            settings::secret_delete,
            chat_ledger_get,
            settings::set_local_voice,
            settings::get_local_voice,
            assets::generate_asset_command,
            assets::generate_mesh_command,
            assets::generate_video_command,
            film_flow::film_flow_load,
            film_flow::film_flow_save,
            film_flow::tts_doubao,
            film_flow::tts_local,
            film_flow::export_film,
            canvas::canvas_load,
            canvas::canvas_save,
            canvas::canvas_preview_html,
            assets::list_assets,
            assets::delete_asset,
            assets::add_asset_to_project,
            assets::set_video_as_clip,
            demos::open_demo_in_browser,
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
            project::export_project_singlefile,
            project::import_project_zip,
            project::running_projects,
            project::open_project_in_browser,
            templates::list_templates,
            templates::import_gameplay_pack,
            feedback::submit_feedback,
            feedback::has_feedback_pat,
            feedback::save_feedback_pat,
            feedback::clear_feedback_pat,
            sim::sim_export_lerobot,
            sim::sim_reveal_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OriEra Studio");
}
