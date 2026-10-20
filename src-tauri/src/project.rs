//! 项目生成器：把玩法/剧情/Agent 模板合成为可运行的纯静态游戏原型。
//! 生成物 = runtime/index.html 渲染 + game.json / story.json / agents.json / provider.json / README.md

use crate::llm;
use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

const RUNTIME_TEMPLATE: &str = include_str!("../runtime/index.html");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Mechanic {
    pub name: String,
    pub detail: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EconomyResource {
    pub name: String,
    pub source: String,
    pub sink: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoryBeat {
    pub phase: String,
    pub title: String,
    pub desc: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentPreset {
    pub name: String,
    pub role: String,
    pub persona: String,
    pub mood: String,
    pub first_line: String,
}

/// 前端向导组装好的完整规格。
#[derive(Debug, Clone, Deserialize)]
pub struct GameSpec {
    pub project_name: String,
    pub gameplay: GameplaySpec,
    pub story: StorySpec,
    pub agents: Vec<AgentPreset>,
    #[serde(default)]
    pub user_idea: String,
    /// 场景主题：space|cafe|dungeon|snow（v0.2）
    #[serde(default = "default_theme")]
    pub theme: String,
}
fn default_theme() -> String {
    "space".into()
}
#[derive(Debug, Clone, Deserialize)]
pub struct GameplaySpec {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub core_loop: Vec<String>,
    pub mechanics: Vec<Mechanic>,
    pub economy: Vec<EconomyResource>,
    pub difficulty: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct StorySpec {
    pub framework_name: String,
    pub premise: String,
    pub beats: Vec<StoryBeat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectMeta {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub created: String,
    pub dir: String,
    pub url: String,
}

fn slugify(name: &str) -> String {
    let mut s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "my-game".to_string()
    } else {
        s.chars().take(40).collect()
    }
}

fn now_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs:x}")
}

#[tauri::command]
pub fn create_project(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    spec: GameSpec,
) -> Result<ProjectMeta, String> {
    if spec.agents.is_empty() {
        return Err("至少需要一位 NPC（Agent）".into());
    }
    if spec.story.beats.is_empty() {
        return Err("剧情至少需要一个节拍".into());
    }

    let id = format!("{}-{}", slugify(&spec.project_name), now_compact());
    let dir: PathBuf = state.projects_dir.join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败：{e}"))?;

    // ---- game.json / story.json / agents.json ----
    let game_json = serde_json::json!({
        "id": id,
        "name": spec.project_name,
        "tagline": spec.gameplay.tagline,
        "gameplay_id": spec.gameplay.id,
        "core_loop": spec.gameplay.core_loop,
        "mechanics": spec.gameplay.mechanics,
        "economy": spec.gameplay.economy,
        "difficulty": spec.gameplay.difficulty,
        "user_idea": spec.user_idea,
        "theme": spec.theme,
    });
    let story_json = serde_json::json!({
        "framework": spec.story.framework_name,
        "premise": spec.story.premise,
        "beats": spec.story.beats,
    });
    let agents_json = serde_json::to_value(&spec.agents).map_err(|e| e.to_string())?;

    // ---- provider.json（不含 key；key 由 Studio 注入 webview）----
    let provider = settings::load_provider(&app);
    let provider_json = serde_json::json!({
        "kind": provider.as_ref().map(|p| p.kind.clone()).unwrap_or_else(|| "rule".into()),
        "model": provider.as_ref().map(|p| p.model.clone()).unwrap_or_else(|| "rule-v1".into()),
        "base": provider.as_ref().map(|p| p.base.clone()).unwrap_or_default(),
        "note": "API Key 不落盘，由 OriEra Studio 运行时注入浏览器",
    });

    // ---- runtime/index.html ----
    // JSON 直接注入 <script> 上下文时，serde_json 不会转义 '<'：
    // 项目名/用户想法/AI 节拍里的 "</script>" 会突破内联脚本边界，必须统一转义。
    // "\u003c" 在 JSON 字符串与 JS 字面量中均合法。
    let html = RUNTIME_TEMPLATE
        .replace("__PROJECT_NAME__", &html_escape(&spec.project_name))
        .replace("__GAME_JSON__", &json_for_html(&game_json))
        .replace("__STORY_JSON__", &json_for_html(&story_json))
        .replace("__AGENTS_JSON__", &json_for_html(&agents_json));

    // ---- README.md ----
    let readme = format!(
        "# {name}\n\n> {tagline}\n\n由 **OriEra Studio** 生成的 AI 原生游戏原型。\n\n- 玩法模板：`{gid}`（{gname}）\n- 叙事框架：{framework}\n- NPC：{agents}\n- 在线体验：打开 OriEra Studio → 我的项目 → 运行\n\n## 文件说明\n\n- `index.html` 游戏运行时（Three.js 场景 + AI NPC + 剧情导演）\n- `game.json` 玩法数据（核心循环 / 机制 / 经济）\n- `story.json` 剧情节拍\n- `agents.json` NPC 人设（Persona / 情绪 / 开场白）\n- `provider.json` 模型接入配置（不含密钥）\n\n## 说明\n\n- 原型依赖 OriEra 引擎内置的静态资源（Three.js / kernel.js），请在 Studio 内运行。\n- 未配置模型时自动降级为离线性格引擎；在 Studio「模型中心」配置后，NPC 由大模型驱动并拥有记忆。\n",
        name = spec.project_name,
        tagline = spec.gameplay.tagline,
        gid = spec.gameplay.id,
        gname = spec.gameplay.name,
        framework = spec.story.framework_name,
        agents = spec.agents.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join("、"),
    );

    std::fs::write(dir.join("index.html"), html)
        .map_err(|e| format!("写入 index.html 失败：{e}"))?;
    std::fs::write(
        dir.join("game.json"),
        serde_json::to_vec_pretty(&game_json).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("story.json"),
        serde_json::to_vec_pretty(&story_json).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("agents.json"),
        serde_json::to_vec_pretty(&agents_json).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("provider.json"),
        serde_json::to_vec_pretty(&provider_json).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("README.md"), readme).map_err(|e| e.to_string())?;

    Ok(ProjectMeta {
        id: id.clone(),
        name: spec.project_name,
        tagline: spec.gameplay.tagline,
        created: chrono_now(),
        dir: dir.to_string_lossy().into_owned(),
        url: format!("/projects/{id}/"),
    })
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简易 UTC→本地可读时间（避免引入 chrono）：用 PowerShell 太重，直接存 unix 秒
    secs.to_string()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 序列化为可安全注入 HTML <script> 内联上下文的 JSON 文本。
fn json_for_html(v: &serde_json::Value) -> String {
    serde_json::to_string(v)
        .unwrap_or_else(|_| "{}".into())
        .replace('<', "\\u003c")
}

/// 项目 id 白名单：与 slugify 的产出字符集一致，杜绝 ../ 等路径穿越。
fn valid_project_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[tauri::command]
pub fn list_projects(state: State<'_, crate::AppState>) -> Vec<ProjectMeta> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&state.projects_dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let dir = entry.path();
            if !dir.join("index.html").is_file() {
                continue;
            }
            let meta = (|| {
                let g: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(dir.join("game.json")).ok()?)
                        .ok()?;
                let id = dir.file_name()?.to_string_lossy().into_owned();
                Some(ProjectMeta {
                    url: format!("/projects/{id}/"),
                    id,
                    name: g["name"].as_str()?.to_string(),
                    tagline: g["tagline"].as_str().unwrap_or("").to_string(),
                    created: entry
                        .metadata()
                        .ok()?
                        .modified()
                        .ok()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()?
                        .as_secs()
                        .to_string(),
                    dir: dir.to_string_lossy().into_owned(),
                })
            })();
            if let Some(m) = meta {
                out.push(m);
            }
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

#[tauri::command]
pub fn open_project(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<(), String> {
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    let url: tauri::Url = format!("http://127.0.0.1:{}/projects/{}/", state.port, id)
        .parse()
        .map_err(|e| format!("bad url: {e}"))?;

    // 与 demo 相同的注入键（runtime 复用引擎 kernel.js 的 createProvider）
    let inject = {
        let cfg = settings::load_provider(&app);
        match cfg {
            Some(c) if c.is_usable() => format!(
                "try{{localStorage.setItem('aicafe.llm.v1', JSON.stringify({}));}}catch(e){{}}",
                serde_json::to_string(&c).unwrap_or_default()
            ),
            _ => String::new(),
        }
    };

    let label = format!("project-{id}");
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }
    let name = std::fs::read_to_string(dir.join("game.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|g| g["name"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| id.clone());

    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
        .title(&format!("🎮 {name} · OriEra 原型"))
        .inner_size(1360.0, 900.0)
        .min_inner_size(1024.0, 700.0);
    if !inject.is_empty() {
        // F5 刷新：webview 默认无刷新键，注入键盘监听
        let script = format!(
            "{inject}document.addEventListener('keydown',e=>{{if(e.key==='F5'){{e.preventDefault();location.reload();}}}});"
        );
        builder = builder.initialization_script(&script);
    } else {
        builder = builder.initialization_script(
            "document.addEventListener('keydown',e=>{if(e.key==='F5'){e.preventDefault();location.reload();}});",
        );
    }
    let win = builder.build().map_err(|e| e.to_string())?;
    if crate::settings::load(&app).developer_mode {
        win.open_devtools();
    }
    Ok(())
}

// ====================== v0.2：编辑器与项目维护命令 ======================

#[derive(Serialize)]
pub struct ProjectDetail {
    pub game: serde_json::Value,
    pub story: serde_json::Value,
    pub agents: serde_json::Value,
}

fn read_json_file(dir: &std::path::Path, name: &str) -> Result<serde_json::Value, String> {
    let text =
        std::fs::read_to_string(dir.join(name)).map_err(|e| format!("读取 {name} 失败：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 {name} 失败：{e}"))
}

#[tauri::command]
pub fn read_project(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<ProjectDetail, String> {
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    Ok(ProjectDetail {
        game: read_json_file(&dir, "game.json")?,
        story: read_json_file(&dir, "story.json")?,
        agents: read_json_file(&dir, "agents.json")?,
    })
}

/// 保存剧情编辑（含前提与节拍）。写入前把旧 story.json 备份到 .history/（保留 10 份）。
#[tauri::command]
pub fn update_story(
    state: State<'_, crate::AppState>,
    id: String,
    story: serde_json::Value,
) -> Result<(), String> {
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    if !story["premise"].is_string()
        || !story["beats"].is_array()
        || story["beats"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true)
    {
        return Err("剧情需要非空 premise 与至少一个节拍".into());
    }

    // 备份
    let history = dir.join(".history");
    let _ = std::fs::create_dir_all(&history);
    if let Ok(old) = std::fs::read(dir.join("story.json")) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = std::fs::write(history.join(format!("story-{ts}.json")), old);
        // 只保留最近 10 份
        if let Ok(files) = std::fs::read_dir(&history) {
            let mut list: Vec<_> = files.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            list.sort();
            while list.len() > 10 {
                let _ = std::fs::remove_file(list.remove(0));
            }
        }
    }

    std::fs::write(
        dir.join("story.json"),
        serde_json::to_vec_pretty(&story).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入失败：{e}"))
}

/// 单拍 AI 重写（编辑器核心）。返回重写后的节拍并落盘。
#[tauri::command]
pub async fn regenerate_beat(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
    beat_index: usize,
    instruction: String,
) -> Result<llm::Beat, String> {
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let cfg = crate::settings::load_provider(&app)
        .ok_or_else(|| "AI 重写需要先在模型中心配置模型".to_string())?;

    let dir = state.projects_dir.join(&id);
    let game = read_json_file(&dir, "game.json")?;
    let mut story = read_json_file(&dir, "story.json")?;

    let beats = story["beats"].as_array().cloned().unwrap_or_default();
    if beat_index >= beats.len() {
        return Err(format!("节拍序号越界：{beat_index}").into());
    }
    // serde 宽松解析:choices 字段可缺省(v0.3 分支),缺 phase/title/desc 才算坏
    let parse_beat = |v: &serde_json::Value| -> Option<llm::Beat> {
        serde_json::from_value::<llm::Beat>(v.clone()).ok()
    };
    let cur = parse_beat(&beats[beat_index]).ok_or("节拍字段不完整")?;
    let prev = if beat_index > 0 {
        parse_beat(&beats[beat_index - 1])
    } else {
        None
    };
    let next = if beat_index + 1 < beats.len() {
        parse_beat(&beats[beat_index + 1])
    } else {
        None
    };

    let name = game["name"].as_str().unwrap_or("未命名游戏");
    let premise = story["premise"].as_str().unwrap_or("");
    let rewritten = llm::regenerate_beat(
        &cfg,
        name,
        premise,
        beat_index + 1,
        beats.len(),
        prev.as_ref(),
        &cur,
        next.as_ref(),
        &instruction,
    )
    .await?;

    story["beats"][beat_index] = serde_json::to_value(&rewritten).map_err(|e| e.to_string())?;
    let _ = update_story(state, id.clone(), story);
    Ok(rewritten)
}

/// 用当前运行时模板重写项目 index.html（保留 game/story/agents 数据）。
#[tauri::command]
pub fn upgrade_runtime(state: State<'_, crate::AppState>, id: String) -> Result<String, String> {
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.join("game.json").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    let game = read_json_file(&dir, "game.json")?;
    let story = read_json_file(&dir, "story.json")?;
    let agents = read_json_file(&dir, "agents.json")?;
    let name = game["name"].as_str().unwrap_or(&id).to_string();

    let html = RUNTIME_TEMPLATE
        .replace("__PROJECT_NAME__", &html_escape(&name))
        .replace("__GAME_JSON__", &json_for_html(&game))
        .replace("__STORY_JSON__", &json_for_html(&story))
        .replace("__AGENTS_JSON__", &json_for_html(&agents));
    std::fs::write(dir.join("index.html"), html).map_err(|e| format!("写入失败：{e}"))?;
    Ok("运行时已升级到当前模板".into())
}

/// 导出项目为可分享 zip（对方导入自己的 Studio projects 目录即可玩）。
#[tauri::command]
pub async fn export_project_zip(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    let file_name = format!("{id}.zip");
    let path = app
        .dialog()
        .file()
        .add_filter("zip", &["zip"])
        .set_file_name(&file_name)
        .blocking_save_file();
    let Some(path) = path else {
        return Ok(String::new()); // 用户取消
    };
    let out = path.into_path().map_err(|e| format!("路径无效：{e}"))?;

    let file = std::fs::File::create(&out).map_err(|e| format!("创建文件失败：{e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    use std::io::Write as _;
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut stack = vec![dir.clone()];
    let base = dir.parent().unwrap_or(&dir).to_path_buf();
    while let Some(p) = stack.pop() {
        if p.file_name().map(|n| n == ".history").unwrap_or(false) {
            continue; // 备份目录不打进分享包
        }
        for entry in std::fs::read_dir(&p)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path.strip_prefix(&base).map_err(|e| e.to_string())?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            zw.start_file(rel, options).map_err(|e| e.to_string())?;
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            zw.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    zw.finish().map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn delete_project(state: State<'_, crate::AppState>, id: String) -> Result<(), String> {
    let dir = state.projects_dir.join(&id);
    // 只允许删除 projects 目录下的直接子目录，防路径穿越
    let canonical = dir.canonicalize().map_err(|_| "项目不存在".to_string())?;
    let base = state
        .projects_dir
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if canonical.parent() != Some(base.as_path()) {
        return Err("非法路径".into());
    }
    std::fs::remove_dir_all(&canonical).map_err(|e| format!("删除失败：{e}"))
}

#[tauri::command]
pub fn reveal_project_dir(
    state: State<'_, crate::AppState>,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !valid_project_id(&id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = state.projects_dir.join(&id);
    if !dir.is_dir() {
        return Err(format!("项目不存在：{id}"));
    }
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("打开目录失败：{e}"))
}
