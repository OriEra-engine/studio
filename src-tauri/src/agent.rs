//! Agent harness（v0.13.0 Agent 工作台）：「奥粒」真正的工具调用循环。
//! 循环：模型 ↔ 工具（读/写项目数据、查状态）↔ 审批门（写操作需用户放行）。
//! 会话落盘 appdata/agent_sessions/*.json：messages（OpenAI 协议，上下文真源）
//! + turns（展示块，前端会话恢复用）。
//! 快速响应批：provider 走 settings 内存缓存；delta 经 30ms 攒批再过 IPC。

use crate::agent_state;
use crate::llm::{self, TurnOutcome};
use crate::settings;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

/// 回灌模型的工具结果上限（字符）；完整内容只进 UI 不进上下文。
const MAX_TOOL_RESULT_CHARS: usize = 6000;
/// 读文件类工具回灌上限。
const MAX_FILE_CHARS: usize = 24_000;
/// diff 卡最多行数。
const DIFF_MAX_LINES: usize = 400;

// ====================== 会话持久化 ======================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSession {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub model: String,
    /// OpenAI 协议消息（不含 system，重建上下文的真源）。
    #[serde(default)]
    pub messages: Vec<Value>,
    /// 展示 turns：{role:"user",text} | {role:"assistant",blocks:[…]}。
    #[serde(default)]
    pub turns: Vec<Value>,
}

#[derive(Serialize)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    pub model: String,
    pub turns: usize,
}

fn sessions_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("agent_sessions");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn session_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn session_path(app: &AppHandle, id: &str) -> Result<std::path::PathBuf, String> {
    if !session_id_ok(id) {
        return Err(format!("非法会话 id：{id}"));
    }
    Ok(sessions_dir(app).join(format!("{id}.json")))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn save_session(app: &AppHandle, s: &AgentSession) -> Result<(), String> {
    let path = session_path(app, &s.id)?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    // 原子写：temp+rename
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入会话失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("落盘会话失败：{e}"))
}

fn load_session(app: &AppHandle, id: &str) -> Result<AgentSession, String> {
    let path = session_path(app, id)?;
    let text = std::fs::read_to_string(&path).map_err(|_| format!("会话不存在：{id}"))?;
    serde_json::from_str(&text).map_err(|e| format!("会话损坏（{e}）"))
}

fn new_session_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("s-{n:x}")
}

// ====================== 会话命令 ======================

#[tauri::command]
pub fn agent_session_new(app: AppHandle, first_hint: Option<String>) -> AgentSession {
    let now = now_secs();
    let title = first_hint
        .map(|h| h.chars().take(24).collect::<String>())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "新会话".into());
    let s = AgentSession {
        id: new_session_id(),
        title,
        created: now,
        updated: now,
        model: String::new(),
        messages: vec![],
        turns: vec![],
    };
    let _ = save_session(&app, &s);
    s
}

#[tauri::command]
pub fn agent_session_list(app: AppHandle) -> Vec<SessionMeta> {
    let dir = sessions_dir(&app);
    let mut out: Vec<SessionMeta> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<AgentSession>(&text) {
                    out.push(SessionMeta {
                        turns: s.turns.len(),
                        id: s.id,
                        title: s.title,
                        created: s.created,
                        updated: s.updated,
                        model: s.model,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out.truncate(50);
    out
}

#[tauri::command]
pub fn agent_session_load(app: AppHandle, id: String) -> Result<AgentSession, String> {
    let mut s = load_session(&app, &id)?;
    s.messages = vec![]; // 协议消息不回传前端（省 IPC，也是上下文内部细节）
    Ok(s)
}

#[tauri::command]
pub fn agent_session_delete(app: AppHandle, id: String) -> Result<(), String> {
    let path = session_path(&app, &id)?;
    std::fs::remove_file(path).map_err(|e| format!("删除会话失败：{e}"))
}

#[tauri::command]
pub fn agent_session_rename(app: AppHandle, id: String, title: String) -> Result<(), String> {
    let mut s = load_session(&app, &id)?;
    let title = title.trim().chars().take(40).collect::<String>();
    if !title.is_empty() {
        s.title = title;
    }
    s.updated = now_secs();
    save_session(&app, &s)
}

// ====================== 审批门 ======================

/// 审批中枢：ApprovalRequest id → 裁决通道。前端 agent_approve 命令投递，
/// agent 循环等待；取消任务时 drop sender（接收端得到 Err = 视为拒绝）。
#[derive(Default)]
pub struct ApprovalHub(pub Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>);

impl ApprovalHub {
    fn register(&self, id: &str) -> tokio::sync::oneshot::Receiver<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.0.lock().unwrap().insert(id.to_string(), tx);
        rx
    }
    fn drop_pending(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

/// 裁决一个待审批请求（lib.rs 的 agent_approve 命令转发至此）。
pub fn resolve_approval(hub: &ApprovalHub, id: &str, approved: bool) -> bool {
    let tx = hub.0.lock().unwrap().remove(id);
    match tx {
        Some(tx) => tx.send(approved).is_ok(),
        None => false,
    }
}

// ====================== 工具上下文与注册表 ======================

/// 工具执行环境（命令层装配后传入循环，避免 State 跨 spawn）。
#[derive(Clone)]
pub struct ToolCtx {
    pub app: AppHandle,
    pub projects_dir: std::path::PathBuf,
    pub snapshot_version: String,
    pub port: u16,
    pub app_version: String,
}

fn project_dir_ok(ctx: &ToolCtx, id: &str) -> Result<std::path::PathBuf, String> {
    if !crate::project::valid_project_id(id) {
        return Err(format!("非法项目 id：{id}"));
    }
    let dir = ctx.projects_dir.join(id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{id}"));
    }
    Ok(dir)
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn json_compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

fn read_json(dir: &std::path::Path, name: &str) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(dir.join(name)).map_err(|e| format!("读取 {name} 失败：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 {name} 失败：{e}"))
}

pub fn tools_schema() -> Vec<Value> {
    vec![
        json!({"type":"function","function":{"name":"list_projects","description":"列出工作台里的全部游戏项目（id/名称/简介）。需要知道用户有哪些项目时调用。","parameters":{"type":"object","properties":{},"required":[]}}}),
        json!({"type":"function","function":{"name":"read_project","description":"读取一个项目的完整设计数据：game（玩法/命名）、story（前提+节拍）、agents（NPC 配置）。","parameters":{"type":"object","properties":{"id":{"type":"string","description":"项目 id（来自 list_projects）"}},"required":["id"]}}}),
        json!({"type":"function","function":{"name":"read_project_file","description":"读取项目内单个文件原文（story.json / game.json / agents.json）。任何修改前必须先读当前内容。","parameters":{"type":"object","properties":{"id":{"type":"string"},"file":{"type":"string","enum":["story.json","game.json","agents.json"]}},"required":["id","file"]}}}),
        json!({"type":"function","function":{"name":"write_project_file","description":"整体重写项目的 story.json 或 agents.json（用户会先看到 diff 并审批；被拒绝不要重试同一操作）。story.json 必须含非空 premise 与 beats 数组。改剧情时：先 read_project_file 拿现内容，改好后把完整新 JSON 文本传入。","parameters":{"type":"object","properties":{"id":{"type":"string"},"file":{"type":"string","enum":["story.json","agents.json"]},"content":{"type":"string","description":"文件完整新内容（JSON 文本）"}},"required":["id","file","content"]}}}),
        json!({"type":"function","function":{"name":"list_templates","description":"列出玩法模板与叙事框架（名称+简介），给新建游戏做推荐时调用。","parameters":{"type":"object","properties":{},"required":[]}}}),
        json!({"type":"function","function":{"name":"list_assets","description":"列出素材中心已生成的资产（插画/3D 模型/视频）。","parameters":{"type":"object","properties":{},"required":[]}}}),
        json!({"type":"function","function":{"name":"get_workbench_status","description":"查询工作台状态：当前模型、项目数、引擎快照版本、内置服务端口。","parameters":{"type":"object","properties":{},"required":[]}}}),
        json!({"type":"function","function":{"name":"update_plan","description":"展示/更新当前任务的计划清单（3-6 步）。多步任务开始时先调用；每完成一步把对应步骤 status 更新为 done。","parameters":{"type":"object","properties":{"steps":{"type":"array","items":{"type":"object","properties":{"step":{"type":"string","description":"步骤描述（一句话）"},"status":{"type":"string","enum":["pending","running","done"]}},"required":["step","status"]}}},"required":["steps"]}}}),
        json!({"type":"function","function":{"name":"propose_choices","description":"当你给出多个可行方案或方向、需要用户拍板时调用：展示选项卡让用户点选下一步。2-3 个选项，每个 label 不超过 12 字，detail 一句话讲清取舍。","parameters":{"type":"object","properties":{"options":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string","description":"选项短语（不超过12字）"},"detail":{"type":"string","description":"一句话讲清取舍"}},"required":["label","detail"]}}},"required":["options"]}}}),
        json!({"type":"function","function":{"name":"read_skill","description":"读取一个已安装技能(SKILL.md)的全文。system prompt 的技能索引里列出了可用技能；执行相关任务前先读全文并遵循它。","parameters":{"type":"object","properties":{"name":{"type":"string","description":"技能名（来自技能索引）"}},"required":["name"]}}}),
    ]
}

fn needs_approval(name: &str) -> bool {
    name == "write_project_file"
}

fn tool_summary(name: &str, args: &Value) -> String {
    match name {
        "read_project_file" | "write_project_file" => {
            let id = args["id"].as_str().unwrap_or("?");
            let file = args["file"].as_str().unwrap_or("?");
            format!("{file} · {id}")
        }
        "read_project" => args["id"].as_str().unwrap_or("?").to_string(),
        "update_plan" => format!(
            "{} 步",
            args["steps"].as_array().map(|a| a.len()).unwrap_or(0)
        ),
        "propose_choices" => format!(
            "{} 个选项",
            args["options"].as_array().map(|a| a.len()).unwrap_or(0)
        ),
        "read_skill" => format!("读取技能 {}", args["name"].as_str().unwrap_or("?")),
        _ => name.to_string(),
    }
}

// ---------------- diff（审批卡预览） ----------------

#[derive(Serialize, Clone)]
pub struct DiffLine {
    pub kind: String, // same | del | add | note
    pub text: String,
}

/// 简易行级 diff：公共前后缀对齐，中间标 −/+。够审批卡一眼看懂，
/// 不引依赖不做完整 LCS。
pub fn simple_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut p = 0usize;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0usize;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let clip = |l: &str| -> String { truncate_chars(l, 240) };
    let mut out: Vec<DiffLine> = Vec::new();
    // 变更点前最多 2 行上下文
    let ctx_start = p.saturating_sub(2);
    for l in &a[ctx_start..p] {
        out.push(DiffLine {
            kind: "same".into(),
            text: clip(l),
        });
    }
    let mid_del = &a[p..a.len() - s];
    let mid_add = &b[p..b.len() - s];
    for l in mid_del {
        out.push(DiffLine {
            kind: "del".into(),
            text: clip(l),
        });
    }
    for l in mid_add {
        out.push(DiffLine {
            kind: "add".into(),
            text: clip(l),
        });
    }
    if mid_del.is_empty() && mid_add.is_empty() {
        out.push(DiffLine {
            kind: "note".into(),
            text: "（内容无变化）".into(),
        });
    }
    // 变更点后最多 2 行上下文
    let ctx_end = (b.len() - s + 2).min(b.len());
    for l in &b[b.len() - s..ctx_end] {
        out.push(DiffLine {
            kind: "same".into(),
            text: clip(l),
        });
    }
    // 超长截断：保头去尾
    if out.len() > DIFF_MAX_LINES {
        let kept_head = 240;
        let kept_tail = 120;
        let omitted = out.len() - kept_head - kept_tail;
        let mut trimmed: Vec<DiffLine> = out[..kept_head].to_vec();
        trimmed.push(DiffLine {
            kind: "note".into(),
            text: format!("……中间省略 {omitted} 行，完整内容以文件为准……"),
        });
        trimmed.extend(out[out.len() - kept_tail..].iter().cloned());
        out = trimmed;
    }
    out
}

// ---------------- 工具执行 ----------------

struct ExecResult {
    ok: bool,
    summary: String,
    /// 回灌模型的结果（已截断）
    for_model: String,
    /// UI 展示用的结果预览（可更长）
    detail: Value,
}

fn exec_tool(ctx: &ToolCtx, call: &llm::CompletedToolCall) -> ExecResult {
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    let fail = |msg: String| ExecResult {
        ok: false,
        summary: msg.chars().take(60).collect(),
        for_model: json!({"ok": false, "error": msg}).to_string(),
        detail: json!({"error": msg}),
    };
    match call.name.as_str() {
        "list_projects" => {
            let mut items = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&ctx.projects_dir) {
                for entry in rd.filter_map(|e| e.ok()) {
                    let dir = entry.path();
                    if !dir.join("index.html").is_file() {
                        continue;
                    }
                    if let Ok(g) = read_json(&dir, "game.json") {
                        items.push(json!({
                            "id": dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            "name": g["name"].as_str().unwrap_or(""),
                            "tagline": g["tagline"].as_str().unwrap_or(""),
                        }));
                    }
                }
            }
            let n = items.len();
            ExecResult {
                ok: true,
                summary: format!("共 {n} 个项目"),
                for_model: truncate_chars(&json_compact(&json!(items)), MAX_TOOL_RESULT_CHARS),
                detail: json!(items),
            }
        }
        "read_project" => {
            let Some(id) = args["id"].as_str() else {
                return fail("缺少 id".into());
            };
            let dir = match project_dir_ok(ctx, id) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            let game = read_json(&dir, "game.json");
            let story = read_json(&dir, "story.json");
            let agents = read_json(&dir, "agents.json");
            let (game, story, agents) = match (game, story, agents) {
                (Ok(g), Ok(s), Ok(a)) => (g, s, a),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return fail(e),
            };
            let name = game["name"].as_str().unwrap_or(id);
            ExecResult {
                ok: true,
                summary: format!("「{name}」设计数据"),
                for_model: truncate_chars(
                    &json_compact(&json!({"game": game, "story": story, "agents": agents})),
                    MAX_TOOL_RESULT_CHARS,
                ),
                detail: json!({"game": game, "story": story, "agents": agents}),
            }
        }
        "read_project_file" => {
            let (Some(id), Some(file)) = (args["id"].as_str(), args["file"].as_str()) else {
                return fail("缺少 id 或 file".into());
            };
            if !["story.json", "game.json", "agents.json"].contains(&file) {
                return fail(format!("不允许读取 {file}"));
            }
            let dir = match project_dir_ok(ctx, id) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            let text = match std::fs::read_to_string(dir.join(file)) {
                Ok(t) => t,
                Err(e) => return fail(format!("读取 {file} 失败：{e}")),
            };
            let truncated = text.chars().count() > MAX_FILE_CHARS;
            ExecResult {
                ok: true,
                summary: format!("{file}（{} 字）", text.chars().count()),
                for_model: truncate_chars(&text, MAX_FILE_CHARS),
                detail: json!({"file": file, "chars": text.chars().count(), "truncated": truncated}),
            }
        }
        "write_project_file" => {
            // 审批已在循环层完成，到达这里 = 用户已放行
            let (Some(id), Some(file)) = (args["id"].as_str(), args["file"].as_str()) else {
                return fail("缺少 id 或 file".into());
            };
            let Some(content) = args["content"].as_str() else {
                return fail("缺少 content".into());
            };
            if !["story.json", "agents.json"].contains(&file) {
                return fail(format!("不允许写入 {file}"));
            }
            let dir = match project_dir_ok(ctx, id) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            let parsed: Value = match serde_json::from_str(content) {
                Ok(v) => v,
                Err(e) => return fail(format!("新内容不是合法 JSON：{e}")),
            };
            if file == "story.json"
                && (!parsed["premise"].is_string()
                    || !parsed["beats"].is_array()
                    || parsed["beats"]
                        .as_array()
                        .map(|a| a.is_empty())
                        .unwrap_or(true))
            {
                return fail("story.json 需要非空 premise 与至少一个节拍".into());
            }
            // 备份旧版（保留 10 份，与剧情编辑器同约定）
            let history = dir.join(".history");
            let _ = std::fs::create_dir_all(&history);
            if let Ok(old) = std::fs::read(dir.join(file)) {
                let stem = file.trim_end_matches(".json");
                let _ = std::fs::write(history.join(format!("{stem}-{}.json", now_secs())), old);
                if let Ok(files) = std::fs::read_dir(&history) {
                    let mut list: Vec<_> = files.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                    list.sort();
                    while list.len() > 10 {
                        let _ = std::fs::remove_file(list.remove(0));
                    }
                }
            }
            let pretty = match serde_json::to_string_pretty(&parsed) {
                Ok(t) => t,
                Err(e) => return fail(format!("序列化失败：{e}")),
            };
            if let Err(e) = std::fs::write(dir.join(file), &pretty) {
                return fail(format!("写入失败：{e}"));
            }
            crate::project::reload_project_window(&ctx.app, id);
            ExecResult {
                ok: true,
                summary: format!("{file} 已写入并热重载"),
                for_model: json!({"ok": true, "written": file, "chars": pretty.chars().count()})
                    .to_string(),
                detail: json!({"written": file, "chars": pretty.chars().count()}),
            }
        }
        "list_templates" => {
            let lib = crate::templates::list_templates(ctx.app.clone());
            let gameplay: Vec<Value> = lib
                .gameplay
                .iter()
                .map(|g| {
                    json!({
                        "name": g["name"].as_str().unwrap_or(""),
                        "tagline": g["tagline"].as_str().unwrap_or(""),
                    })
                })
                .collect();
            let frameworks: Vec<Value> = lib
                .story_frameworks
                .iter()
                .map(|f| json!({"name": f["name"].as_str().unwrap_or("")}))
                .collect();
            ExecResult {
                ok: true,
                summary: format!(
                    "{} 个玩法模板 · {} 个叙事框架",
                    gameplay.len(),
                    frameworks.len()
                ),
                for_model: truncate_chars(
                    &json_compact(&json!({"gameplay": gameplay, "frameworks": frameworks})),
                    MAX_TOOL_RESULT_CHARS,
                ),
                detail: json!({"gameplay": gameplay, "frameworks": frameworks}),
            }
        }
        "list_assets" => {
            let meta_path = ctx
                .projects_dir
                .parent()
                .map(|p| p.join("assets").join("meta.json"))
                .unwrap_or_default();
            let list: Vec<Value> = std::fs::read_to_string(&meta_path)
                .ok()
                .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
                .unwrap_or_default();
            let brief: Vec<Value> = list
                .iter()
                .take(50)
                .map(|a| {
                    json!({
                        "id": a["id"].as_str().unwrap_or(""),
                        "kind": a["kind"].as_str().unwrap_or("image"),
                        "prompt": a["prompt"].as_str().unwrap_or(""),
                        "file": a["file"].as_str().unwrap_or(""),
                    })
                })
                .collect();
            ExecResult {
                ok: true,
                summary: format!("共 {} 件资产", list.len()),
                for_model: truncate_chars(&json_compact(&json!(brief)), MAX_TOOL_RESULT_CHARS),
                detail: json!(brief),
            }
        }
        "read_skill" => {
            let Some(name) = args["name"].as_str() else {
                return fail("缺少 name".into());
            };
            match crate::agent_state::skill_body(&ctx.app, name) {
                Ok(body) => {
                    let chars = body.chars().count();
                    ExecResult {
                        ok: true,
                        summary: format!("{}（{} 字）", name, chars),
                        for_model: body,
                        detail: json!({"skill": name, "chars": chars}),
                    }
                }
                Err(e) => fail(e),
            }
        }
        "get_workbench_status" => {
            let provider = settings::load_provider(&ctx.app);
            let project_count = std::fs::read_dir(&ctx.projects_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter(|e| e.path().join("index.html").is_file())
                        .count()
                })
                .unwrap_or(0);
            let v = json!({
                "model": provider.as_ref().map(|p| p.model.as_str()).unwrap_or(""),
                "provider_kind": provider.as_ref().map(|p| p.kind.as_str()).unwrap_or(""),
                "project_count": project_count,
                "engine": ctx.snapshot_version,
                "port": ctx.port,
                "app_version": ctx.app_version,
            });
            ExecResult {
                ok: true,
                summary: format!(
                    "模型 {} · {} 个项目",
                    v["model"].as_str().unwrap_or("未配置"),
                    project_count
                ),
                for_model: v.to_string(),
                detail: v,
            }
        }
        "update_plan" => {
            let steps = args["steps"].as_array().cloned().unwrap_or_default();
            let n = steps.len();
            ExecResult {
                ok: true,
                summary: format!("计划已更新（{n} 步）"),
                for_model: json!({"ok": true}).to_string(),
                detail: json!(steps),
            }
        }
        other => fail(format!("未知工具：{other}")),
    }
}

// ====================== delta 攒批（30ms 窗口） ======================

#[derive(Clone)]
struct DeltaBatcher {
    inner: Arc<Mutex<(String, std::time::Instant)>>,
    ch: Channel<crate::AiStreamEvent>,
}

impl DeltaBatcher {
    fn new(ch: Channel<crate::AiStreamEvent>) -> Self {
        Self {
            inner: Arc::new(Mutex::new((String::new(), std::time::Instant::now()))),
            ch,
        }
    }
    fn push(&self, d: &str) {
        let flush = {
            let mut g = self.inner.lock().unwrap();
            g.0.push_str(d);
            g.1.elapsed() >= std::time::Duration::from_millis(30)
        };
        if flush {
            self.flush();
        }
    }
    fn flush(&self) {
        let text = {
            let mut g = self.inner.lock().unwrap();
            g.1 = std::time::Instant::now();
            std::mem::take(&mut g.0)
        };
        if !text.is_empty() {
            let _ = self.ch.send(crate::AiStreamEvent::Delta { text });
        }
    }
}

// ====================== agent 主循环 ======================

#[derive(PartialEq)]
enum Exit {
    Text,
    Error,
    Cancelled,
    Exhausted,
}

/// 一条用户消息驱动的完整 agent 任务：流式模型轮 ↔ 工具执行 ↔ 审批。
/// 结束（Done/Error/取消）前把会话落盘。事件全部经 `ch` 推前端。
pub async fn run(
    app: AppHandle,
    ctx: ToolCtx,
    session_id: String,
    user_text: String,
    ch: Channel<crate::AiStreamEvent>,
    cancelled: Arc<AtomicBool>,
    usage_out: &mut llm::TokenUsage,
) -> Result<(), String> {
    let mut session = load_session(&app, &session_id)?;
    let cfg = settings::load_provider(&app).ok_or_else(|| {
        "小奥离线中：先在「模型中心」配置模型（云端或 Ollama 本地），配置好我就能真正对话啦"
            .to_string()
    })?;
    let hub = app
        .try_state::<ApprovalHub>()
        .ok_or("审批服务未就绪")?
        .inner();

    // 用户消息进标题、上下文与展示
    if session.title.is_empty() || session.title == "新会话" {
        session.title = user_text.chars().take(24).collect();
    }
    session.messages.push(json!({"role": "user", "content": user_text.clone()}));
    session.turns.push(json!({"role": "user", "text": user_text}));
    session.model = cfg.model.clone();
    session.updated = now_secs();
    let _ = save_session(&app, &session);

    // Agent State（v0.14.0）：prompt/轮数/温度/工具开关全部来自可快照的文件
    let state = agent_state::load_cached(&app);
    let sys = agent_state::render_system_prompt(&app, &ctx.snapshot_version, ctx.port);
    let max_turns = state.config.max_turns.clamp(1, 16) as usize;
    let max_tokens = state.config.max_tokens.max(256);
    let temperature = state.config.temperature.clamp(0.0, 1.5);
    let tool_enabled =
        |name: &str| state.config.tools.get(name).copied().unwrap_or(true);
    let tools: Vec<Value> = tools_schema()
        .into_iter()
        .filter(|t| tool_enabled(t["function"]["name"].as_str().unwrap_or("")))
        .collect();
    let mut use_tools = cfg.kind != "anthropic" && !tools.is_empty();

    let mut assistant_blocks: Vec<Value> = Vec::new();
    let mut final_text = String::new();
    let mut exit = Exit::Exhausted;
    let mut tool_unsupported_notified = false;
    let mut usage_total = llm::TokenUsage::default();

    let run_result: Result<(), String> = async {
        for _turn_no in 1..=max_turns {
            if cancelled.load(Ordering::Relaxed) {
                let _ = ch.send(crate::AiStreamEvent::Error {
                    message: "已取消".into(),
                });
                exit = Exit::Cancelled;
                break;
            }

            // 重建本轮模型消息：system + 会话协议消息
            let mut msgs = vec![json!({"role": "system", "content": sys})];
            msgs.extend(session.messages.iter().cloned());

            let batcher = DeltaBatcher::new(ch.clone());
            let mut turn_text = String::new();
            let ch_tools = ch.clone();

            let res = if use_tools {
                llm::chat_stream_agent(
                    &cfg,
                    msgs,
                    max_tokens,
                    temperature,
                    &tools,
                    llm::AgentTurnEvents {
                        on_delta: &mut |d: &str| {
                            turn_text.push_str(d);
                            batcher.push(d);
                            true
                        },
                        on_tool_start: &mut |id: &str, name: &str| {
                            let _ = ch_tools.send(crate::AiStreamEvent::ToolStart {
                                id: id.to_string(),
                                name: name.to_string(),
                            });
                        },
                        on_tool_args: &mut |id: &str, frag: &str| {
                            let _ = ch_tools.send(crate::AiStreamEvent::ToolArgs {
                                id: id.to_string(),
                                fragment: frag.to_string(),
                            });
                        },
                    },
                    &cancelled,
                )
                .await
            } else {
                llm::chat_stream(
                    &cfg,
                    msgs,
                    max_tokens,
                    temperature,
                    &mut |d: &str| {
                        turn_text.push_str(d);
                        batcher.push(d);
                        true
                    },
                    &cancelled,
                )
                .await
                .map(|(t, u)| (TurnOutcome::Text(t), u))
            };

            let (outcome, turn_usage) = match res {
                Ok((o, u)) => (o, u),
                Err(e) if use_tools && llm::looks_like_tools_rejected(&e) => {
                    // 该端点不支持 function calling：去工具重跑本轮，降级纯对话
                    use_tools = false;
                    turn_text.clear();
                    batcher.flush();
                    if !tool_unsupported_notified {
                        tool_unsupported_notified = true;
                        let _ = ch.send(crate::AiStreamEvent::ToolUnsupported);
                    }
                    let mut msgs = vec![json!({"role": "system", "content": sys})];
                    msgs.extend(session.messages.iter().cloned());
                    match llm::chat_stream(
                        &cfg,
                        msgs,
                        max_tokens,
                        temperature,
                        &mut |d: &str| {
                            turn_text.push_str(d);
                            batcher.push(d);
                            true
                        },
                        &cancelled,
                    )
                    .await
                    {
                        Ok((t, u2)) => (TurnOutcome::Text(t), u2),
                        Err(e2) => {
                            batcher.flush();
                            let _ = ch.send(crate::AiStreamEvent::Error { message: e2 });
                            exit = Exit::Error;
                            break;
                        }
                    }
                }
                Err(e) => {
                    batcher.flush();
                    let _ = ch.send(crate::AiStreamEvent::Error { message: e });
                    exit = Exit::Error;
                    break;
                }
            };
            batcher.flush();
            usage_total.input += turn_usage.input;
            usage_total.output += turn_usage.output;

            match outcome {
                TurnOutcome::Text(text) => {
                    let text = if text.trim().is_empty() {
                        "(空回复，再试一次？)".to_string()
                    } else {
                        text
                    };
                    final_text = text.clone();
                    session
                        .messages
                        .push(json!({"role": "assistant", "content": text.clone()}));
                    assistant_blocks.push(json!({"type": "text", "content": text}));
                    exit = Exit::Text;
                    break;
                }
                TurnOutcome::ToolCalls(calls) => {
                    let has_text = !turn_text.trim().is_empty();
                    session.messages.push(json!({
                        "role": "assistant",
                        "content": if has_text { Value::String(turn_text.clone()) } else { Value::Null },
                        "tool_calls": calls.iter().map(|c| json!({
                            "id": c.id, "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })).collect::<Vec<_>>(),
                    }));
                    if has_text {
                        assistant_blocks.push(json!({"type": "text", "content": turn_text}));
                    }

                    for call in calls {
                        if cancelled.load(Ordering::Relaxed) {
                            let _ = ch.send(crate::AiStreamEvent::Error {
                                message: "已取消".into(),
                            });
                            exit = Exit::Cancelled;
                            return Ok(());
                        }
                        let args: Value =
                            serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
                        let summary_args = tool_summary(&call.name, &args);

                        // 审批门
                        let approved = if needs_approval(&call.name) {
                            let ap_id = format!(
                                "ap-{:x}-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_nanos())
                                    .unwrap_or(0),
                                call.id
                            );
                            // 写操作的审批卡带 diff
                            let diff = if call.name == "write_project_file" {
                                let id = args["id"].as_str().unwrap_or("");
                                let file = args["file"].as_str().unwrap_or("");
                                let old = project_dir_ok(&ctx, id)
                                    .ok()
                                    .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
                                    .unwrap_or_default();
                                simple_diff(&old, args["content"].as_str().unwrap_or(""))
                            } else {
                                Vec::new()
                            };
                            let title = if call.name == "write_project_file" {
                                format!(
                                    "写入 {}（{}）",
                                    args["file"].as_str().unwrap_or("?"),
                                    args["id"].as_str().unwrap_or("?")
                                )
                            } else {
                                format!("执行 {}", call.name)
                            };
                            let mut rx = hub.register(&ap_id);
                            let _ = ch.send(crate::AiStreamEvent::ApprovalRequest {
                                id: ap_id.clone(),
                                tool: call.name.clone(),
                                title,
                                detail: summary_args.clone(),
                                diff: json!(diff),
                            });
                            // 等裁决：200ms 轮询取消旗标；sender 被 drop = 拒绝
                            let approved;
                            loop {
                                if cancelled.load(Ordering::Relaxed) {
                                    hub.drop_pending(&ap_id);
                                    approved = rx.await.unwrap_or(false);
                                    break;
                                }
                                match tokio::time::timeout(
                                    std::time::Duration::from_millis(200),
                                    &mut rx,
                                )
                                .await
                                {
                                    Ok(v) => {
                                        approved = v.unwrap_or(false);
                                        break;
                                    }
                                    Err(_) => continue,
                                }
                            }
                            assistant_blocks.push(json!({
                                "type": "approval", "tool": call.name,
                                "title": format!("{} · {}", call.name, summary_args),
                                "approved": approved,
                            }));
                            approved
                        } else {
                            true
                        };

                        let started = std::time::Instant::now();
                        let exec = if approved {
                            if call.name == "propose_choices" {
                                // 选项卡（A2UI）：展示给用户，点选后作为新消息进来
                                let options = args["options"].as_array().cloned().unwrap_or_default();
                                let _ = ch.send(crate::AiStreamEvent::Choices {
                                    options: json!(options),
                                });
                                assistant_blocks.push(json!({"type": "choices", "options": options}));
                                ExecResult {
                                    ok: true,
                                    summary: format!("已给出 {} 个选项，等待用户点选", options.len()),
                                    for_model: json!({"ok": true, "note": "选项已展示，等待用户选择；用户点选后其选择会作为新消息到来"}).to_string(),
                                    detail: json!(options),
                                }
                            } else if call.name == "update_plan" {
                                // 计划卡：回显 + 事件
                                let steps = args["steps"].as_array().cloned().unwrap_or_default();
                                let _ = ch.send(crate::AiStreamEvent::Plan {
                                    steps: json!(steps),
                                });
                                assistant_blocks.push(json!({"type": "plan", "steps": steps}));
                                ExecResult {
                                    ok: true,
                                    summary: format!("计划已更新（{} 步）", steps.len()),
                                    for_model: json!({"ok": true}).to_string(),
                                    detail: json!(steps),
                                }
                            } else {
                                exec_tool(&ctx, &call)
                            }
                        } else {
                            ExecResult {
                                ok: false,
                                summary: "用户拒绝了此操作".into(),
                                for_model: json!({"ok": false, "error": "用户拒绝了此操作。不要重试同一操作，用文字说明并给出手动操作路径。"}).to_string(),
                                detail: json!({"denied": true}),
                            }
                        };
                        let duration_ms = started.elapsed().as_millis() as u64;
                        let _ = ch.send(crate::AiStreamEvent::ToolEnd {
                            id: call.id.clone(),
                            ok: exec.ok,
                            summary: exec.summary.clone(),
                            duration_ms,
                        });
                        assistant_blocks.push(json!({
                            "type": "tool", "name": call.name, "args": args,
                            "ok": exec.ok, "summary": exec.summary, "duration_ms": duration_ms,
                            "detail": exec.detail,
                        }));
                        session.messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call.id,
                            "content": truncate_chars(&exec.for_model, MAX_TOOL_RESULT_CHARS),
                        }));
                    }

                    session.updated = now_secs();
                    let _ = save_session(&app, &session);
                    let context_chars: usize = session
                        .messages
                        .iter()
                        .map(|m| m.to_string().chars().count())
                        .sum();
                    let _ = ch.send(crate::AiStreamEvent::Usage {
                        context_chars,
                        turns: session.turns.len(),
                        tokens_in: usage_total.input,
                        tokens_out: usage_total.output,
                    });
                    // 工具轮后继续下一轮模型推理
                }
            }
        }
        Ok(())
    }
    .await;

    // 轮数耗尽（全是工具轮没落文本）给一句收尾提示，只进展示不进上下文
    if exit == Exit::Exhausted && run_result.is_ok() {
        let note = format!(
            "（已连续执行 {} 轮工具，先停在这里——继续对话我就接着干）",
            max_turns
        );
        assistant_blocks.push(json!({"type": "text", "content": note}));
        final_text = note;
    }

    // 收尾：助手 turn 落展示块 + 会话落盘
    if !assistant_blocks.is_empty() {
        session
            .turns
            .push(json!({"role": "assistant", "blocks": assistant_blocks}));
    }
    session.updated = now_secs();
    let _ = save_session(&app, &session);
    *usage_out = usage_total;

    match run_result {
        Err(e) => Err(e),
        Ok(()) => {
            if exit == Exit::Cancelled {
                return Ok(()); // Error{已取消} 已在循环里发过，不再补 Done
            }
            llm::record_call("agent");
            let _ = ch.send(crate::AiStreamEvent::Done {
                result: json!({"text": final_text, "session_id": session.id}),
            });
            Ok(())
        }
    }
}

// ====================== 单测（纯逻辑） ======================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_marks_add_del_with_context() {
        let old = "line1\nline2\nline3\nline4";
        let new = "line1\nline2-changed\nline3\nline4";
        let d = simple_diff(old, new);
        assert!(d.iter().any(|l| l.kind == "del" && l.text == "line2"));
        assert!(d
            .iter()
            .any(|l| l.kind == "add" && l.text == "line2-changed"));
        assert!(d.iter().any(|l| l.kind == "same" && l.text == "line1"));
    }

    #[test]
    fn diff_no_change_note() {
        let d = simple_diff("a\nb", "a\nb");
        assert!(d.iter().any(|l| l.kind == "note"));
    }

    #[test]
    fn diff_truncates_long_output() {
        let old = (0..1000)
            .map(|i| format!("old-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = (0..1000)
            .map(|i| format!("new-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let d = simple_diff(&old, &new);
        assert!(d.len() <= DIFF_MAX_LINES);
        assert!(d.iter().any(|l| l.kind == "note"));
    }

    #[test]
    fn session_id_validation_blocks_traversal() {
        assert!(session_id_ok("s-18f3a2"));
        assert!(!session_id_ok("../evil"));
        assert!(!session_id_ok(""));
        assert!(!session_id_ok("a\\b"));
        assert!(!session_id_ok(&"x".repeat(65)));
    }

    #[test]
    fn tool_summary_shapes() {
        let args = json!({"id": "p1", "file": "story.json"});
        assert_eq!(tool_summary("read_project_file", &args), "story.json · p1");
        let plan = json!({"steps": [1, 2, 3]});
        assert!(tool_summary("update_plan", &plan).contains("3 步"));
    }
}
