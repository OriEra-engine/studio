//! 奥粒 Agent State（v0.14.0，学 PenguinHarness）：
//! 「模型保持固定，harness 进化」——奥粒的提示词与运行参数是文件（Agent State），
//! 每次修改前自动快照，版本单调递增，可一步回滚。
//!
//! appdata/agent_state/
//! ├── system_prompt.md     可编辑人设与规则（{{WORKBENCH}} 占位符运行时注入）
//! ├── config.json          {version, max_turns, temperature, max_tokens, tools{}}
//! └── snapshots/v<N>.json  改动前的完整状态（prompt + config + note）

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// 工具开关（名字 → enabled）；缺省视为 true
    #[serde(default)]
    pub tools: std::collections::HashMap<String, bool>,
}
fn default_version() -> u32 { 1 }
fn default_max_turns() -> u32 { 8 }
fn default_temperature() -> f32 { 0.5 }
fn default_max_tokens() -> u32 { 2048 }

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            max_turns: default_max_turns(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            tools: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub system_prompt: String,
    pub config: AgentConfig,
}

#[derive(Serialize)]
pub struct SnapshotMeta {
    pub version: u32,
    pub saved_at: u64,
    pub note: String,
}

#[derive(Serialize)]
pub struct StateOut {
    pub system_prompt: String,
    pub config: AgentConfig,
    pub snapshots: Vec<SnapshotMeta>,
}

fn state_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("agent_state");
    let _ = std::fs::create_dir_all(dir.join("snapshots"));
    dir
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 内存缓存（同 provider 缓存模式：每条消息不再读盘）
static STATE_CACHE: Mutex<Option<AgentState>> = Mutex::new(None);

fn invalidate_cache() {
    if let Ok(mut c) = STATE_CACHE.lock() {
        *c = None;
    }
}

// ====================== seed ======================

const SEED_PROMPT: &str = r#"你是「奥粒」(Ori),OriEra Studio 世界引擎工作台的常驻小助手,一颗活着的元粒子。
职责:帮玩家用最少步骤做出游戏。风格:简体中文、口语化、短句;给步骤时不超过 6 步,用编号列表;活泼但不啰嗦,禁用客套话堆砌。

你处于 Agent 模式,具备以下工具:
- list_projects / read_project / read_project_file:查项目与设计数据——涉及用户项目的事实问题必须先查再答,不要编造
- write_project_file:整体重写项目的 story.json 或 agents.json(用户会看到 diff 并审批);改剧情必须先 read_project_file 再改
- list_templates / list_assets / get_workbench_status:查模板、素材、工作台状态
- update_plan:多步任务(3 步以上)开始时先展示计划,每完成一步更新状态
- propose_choices:当你给出多个可行方案或方向、需要用户拍板时调用它,让用户点选下一步(2-3 个选项,每个一句话讲清取舍)
- read_skill:读取一个已安装技能(SKILL.md)的全文;下面的技能索引里列出了它们

可用技能(需要时先用 read_skill 读全文,再遵循它执行):
{{SKILLS}}

规则:
1. 写操作被拒绝时,不要换花样重试同一操作,改用文字说明并给手动操作路径
2. story.json 结构:{"premise":"世界观前提","beats":[{"phase":"阶段","title":"节拍标题","desc":"2-3句剧情","choices":[{"label":"选项","next":目标节拍序号(顺延填-1),"effect":"资源+数量"}]}]};保持原有节拍数量与结构,除非用户明确要求增删
3. 一次只调必要的工具;简单问答不要碰工具
4. 当前工作台:{{WORKBENCH}}
"#;

pub fn ensure_seeded(app: &AppHandle) {
    let dir = state_dir(app);
    let prompt_path = dir.join("system_prompt.md");
    let config_path = dir.join("config.json");
    if !prompt_path.is_file() {
        let _ = std::fs::write(&prompt_path, SEED_PROMPT);
    }
    if !config_path.is_file() {
        let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&AgentConfig::default()).unwrap());
    }
}

// ====================== 读 ======================

fn read_state(app: &AppHandle) -> Result<AgentState, String> {
    let dir = state_dir(app);
    ensure_seeded(app);
    let system_prompt = std::fs::read_to_string(dir.join("system_prompt.md"))
        .map_err(|e| format!("读取 system_prompt.md 失败：{e}"))?;
    let config: AgentConfig = serde_json::from_str(
        &std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("读取 config.json 失败：{e}"))?,
    )
    .map_err(|e| format!("解析 config.json 失败：{e}"))?;
    Ok(AgentState { system_prompt, config })
}

/// 带缓存的读取（agent_run 热路径）。
pub fn load_cached(app: &AppHandle) -> AgentState {
    if let Ok(c) = STATE_CACHE.lock() {
        if let Some(hit) = c.as_ref() {
            return hit.clone();
        }
    }
    let state = read_state(app).unwrap_or(AgentState {
        system_prompt: String::new(),
        config: AgentConfig::default(),
    });
    if let Ok(mut c) = STATE_CACHE.lock() {
        *c = Some(state.clone());
    }
    state
}

/// 渲染运行时 system prompt：{{WORKBENCH}} 注入引擎快照与端口；
/// {{SKILLS}} 注入技能索引（无 {{SKILLS}} 占位符的旧版 prompt 自动追加）。
pub fn render_system_prompt(app: &AppHandle, snapshot_version: &str, port: u16) -> String {
    let state = load_cached(app);
    let index = skills_index(app);
    let out = state
        .system_prompt
        .replace(
            "{{WORKBENCH}}",
            &format!("引擎快照 {} ,内置服务端口 {}", snapshot_version, port),
        )
        .replace("{{SKILLS}}", &index);
    if !state.system_prompt.contains("{{SKILLS}}") && !index.is_empty() {
        // 旧版 prompt（进化产物）没有占位符：追加，保证技能索引可见
        return format!("{}\n\n可用技能(需要时先用 read_skill 读全文,再遵循它执行):\n{}", out.trim_end(), index);
    }
    out
}

// ====================== Skills（学 Penguin：SKILL.md 渐进加载） ======================
// agent_state/skills/<name>/SKILL.md；frontmatter(name/description/version) + 正文。
// system prompt 只注入「名称+一句话描述」索引；模型需要时用 read_skill 读全文。

#[derive(Debug, Clone, Serialize)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub version: String,
    pub chars: usize,
}

fn skills_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = state_dir(app).join("skills");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn skill_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 解析 SKILL.md：首段 frontmatter 的 name/description/version + 正文。
fn parse_skill(text: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut desc = String::new();
    let mut version = String::new();
    let mut rest = text;
    if let Some(stripped) = text.strip_prefix("---") {
        if let Some(end) = stripped.find("\n---") {
            let fm = &stripped[..end];
            rest = &stripped[end + 4..];
            for line in fm.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    desc = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("version:") {
                    version = v.trim().to_string();
                }
            }
        }
    }
    (name, desc, version)
}

fn skill_meta_from(path: &std::path::Path) -> Option<SkillMeta> {
    let dir_name = path.file_name()?.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(path.join("SKILL.md")).ok()?;
    let (fm_name, desc, version) = parse_skill(&text);
    // 目录名是权威名称（Penguin 同款规则），覆盖 frontmatter
    let _ = fm_name;
    Some(SkillMeta {
        name: dir_name,
        description: desc,
        version,
        chars: text.chars().count(),
    })
}

pub fn collect_skills(app: &AppHandle) -> Vec<SkillMeta> {
    let dir = skills_dir(app);
    let mut out: Vec<SkillMeta> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() && p.join("SKILL.md").is_file() {
                if let Some(m) = skill_meta_from(&p) {
                    out.push(m);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 技能索引（system prompt 注入用）：每行「name — description」。
pub fn skills_index(app: &AppHandle) -> String {
    collect_skills(app)
        .iter()
        .map(|m| format!("- {}：{}", m.name, if m.description.is_empty() { "（无描述）" } else { &m.description }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// read_skill 工具用：读全文（白名单目录内，防穿越；截断 8000 字符）。
pub fn skill_body(app: &AppHandle, name: &str) -> Result<String, String> {
    if !skill_name_ok(name) {
        return Err(format!("非法技能名：{name}"));
    }
    let path = skills_dir(app).join(name).join("SKILL.md");
    let text = std::fs::read_to_string(&path).map_err(|_| format!("技能 {name} 不存在"))?;
    Ok(trunc(&text, 8000))
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn skill_seed_story_craft() -> &'static str {
    r#"---
name: story-craft
description: 节拍剧情写作守则：五种情绪手法与反模式清单，写/改剧情前必读
version: 2026.11.08.1
---
# 节拍剧情写作守则

desc 只有 2-3 句，每一句都要干活。写之前先问：这一拍要让玩家「感到什么」？

## 五种情绪的具体手法
- 悬疑：给「指向主角的未知」（署名缺失的物件、指向性异常），别只堆氛围词
- 温情：让情感有一个「具体来处」（往事、旧物、动作），克制不煽情
- 紧张：短句 + 信息差 + 倒计时；写可拍摄的动作，不写「心跳加速」
- 幽默：错位与自洽的荒谬，包袱落在动作或台词上；不玩网络梗
- 伏笔：细节必须「可回收」（结局能呼应的实物细节），当下只觉异样

## 反模式（出现即扣质量）
- 陈词滥调：空气凝固/心跳加速/心中一惊
- 解释性废话：把潜台词写明
- 细节堆叠：一个节拍只留一个最强的具体细节，其余删掉
- 新增事实：不引入新角色/新设定，除非任务明确要求

## 结构纪律
- 保持既有节拍数量与字段结构；choices 只在关键拍出现（2 个选项，两条路都讲得通）
"#
}

fn skill_seed_workbench_sop() -> &'static str {
    r#"---
name: workbench-sop
description: 工作台操作 SOP：查项目→改剧情→diff 审批→验证的标准流程
version: 2026.11.08.1
---
# 工作台操作 SOP

## 改剧情标准流程
1. list_projects 找到目标项目 id（名字不等于 id）
2. read_project_file 读 story.json 当前内容（改前必读）
3. 心里对比：改哪些拍、哪些字段一字不动
4. write_project_file 提交完整新 JSON（用户会看到 diff 并审批）
5. 被拒绝：不要换花样重试，说明理由并给手动路径（剧情编辑器 → AI 重写）

## 汇报纪律
- 只报告真实发生的变更；不确定就先查，不编造
- 多步任务先 update_plan；给出方案分歧时用 propose_choices 让用户拍板
"#
}

pub fn ensure_skill_seeds(app: &AppHandle) {
    let dir = skills_dir(app);
    let seeds = [
        ("story-craft", skill_seed_story_craft()),
        ("workbench-sop", skill_seed_workbench_sop()),
    ];
    for (name, content) in seeds {
        let dir_n = dir.join(name);
        let path = dir_n.join("SKILL.md");
        if !path.is_file() {
            let _ = std::fs::create_dir_all(&dir_n);
            let _ = std::fs::write(&path, content);
        }
    }
}

// ====================== Skill 命令 ======================

#[tauri::command]
pub fn skill_list(app: AppHandle) -> Vec<SkillMeta> {
    collect_skills(&app)
}

#[tauri::command]
pub fn skill_get(app: AppHandle, name: String) -> Result<String, String> {
    skill_body(&app, &name)
}

#[tauri::command]
pub fn skill_save(app: AppHandle, name: String, content: String) -> Result<(), String> {
    if !skill_name_ok(&name) {
        return Err("技能名只能用字母/数字/-(不超过48字符)".into());
    }
    let content = content.trim().to_string();
    if content.chars().count() < 20 {
        return Err("SKILL.md 内容太短（至少 20 字符）".into());
    }
    let dir = skills_dir(&app).join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败：{e}"))?;
    std::fs::write(dir.join("SKILL.md"), content).map_err(|e| format!("写入失败：{e}"))
}

#[tauri::command]
pub fn skill_delete(app: AppHandle, name: String) -> Result<(), String> {
    if !skill_name_ok(&name) {
        return Err(format!("非法技能名：{name}"));
    }
    let dir = skills_dir(&app).join(&name);
    if !dir.join("SKILL.md").is_file() {
        return Err(format!("技能 {name} 不存在"));
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("删除失败：{e}"))
}

// ====================== 写（先快照再落盘） ======================

fn write_state(app: &AppHandle, state: &AgentState) -> Result<(), String> {
    let dir = state_dir(app);
    std::fs::write(dir.join("system_prompt.md"), &state.system_prompt)
        .map_err(|e| format!("写入 system_prompt.md 失败：{e}"))?;
    std::fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(&state.config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入 config.json 失败：{e}"))?;
    invalidate_cache();
    Ok(())
}

/// 保存并自动快照：v<N>.json 存改动前状态，新版本号 = 旧 + 1。返回新版本号。
pub fn save_with_snapshot(
    app: &AppHandle,
    system_prompt: &str,
    config: &AgentConfig,
    note: &str,
) -> Result<u32, String> {
    let current = read_state(app)?;
    snapshot_current(app, &current, note)?;
    let mut next = config.clone();
    next.version = current.config.version + 1;
    write_state(app, &AgentState { system_prompt: system_prompt.to_string(), config: next.clone() })?;
    Ok(next.version)
}

fn snapshot_current(app: &AppHandle, current: &AgentState, note: &str) -> Result<(), String> {
    let v = current.config.version;
    let path = state_dir(app).join(format!("snapshots/v{v}.json"));
    if path.is_file() {
        return Ok(()); // 同版本已有快照直接复用（Penguin 同款语义：不覆盖）
    }
    let payload = json!({
        "version": v,
        "saved_at": now_secs(),
        "note": note,
        "system_prompt": current.system_prompt,
        "config": current.config,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?)
        .map_err(|e| format!("写快照失败：{e}"))
}

// ====================== 快照列表与回滚 ======================

fn read_snapshot(app: &AppHandle, version: u32) -> Result<AgentState, String> {
    let path = state_dir(app).join(format!("snapshots/v{version}.json"));
    let text = std::fs::read_to_string(&path).map_err(|_| format!("快照 v{version} 不存在"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("快照损坏：{e}"))?;
    let system_prompt = v["system_prompt"].as_str().unwrap_or("").to_string();
    let config: AgentConfig =
        serde_json::from_value(v["config"].clone()).map_err(|e| format!("快照 config 损坏：{e}"))?;
    Ok(AgentState { system_prompt, config })
}

pub fn snapshots(app: &AppHandle) -> Vec<SnapshotMeta> {
    let dir = state_dir(app).join("snapshots");
    let mut out: Vec<SnapshotMeta> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(ver) = name
                .strip_prefix("v")
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let (saved_at, note) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .map(|v| {
                    (
                        v["saved_at"].as_u64().unwrap_or(0),
                        v["note"].as_str().unwrap_or("").to_string(),
                    )
                })
                .unwrap_or((0, String::new()));
            out.push(SnapshotMeta { version: ver, saved_at, note });
        }
    }
    out.sort_by(|a, b| b.version.cmp(&a.version));
    out
}

/// 回滚：把快照内容写回当前（版本号仍单调 +1，note 记来源），返回新版本号。
pub fn restore(app: &AppHandle, version: u32) -> Result<u32, String> {
    let snap = read_snapshot(app, version)?;
    let current = read_state(app)?;
    snapshot_current(app, &current, "restore 前的当前状态")?;
    let mut next = snap.config.clone();
    next.version = current.config.version + 1;
    write_state(app, &AgentState { system_prompt: snap.system_prompt, config: next.clone() })?;
    Ok(next.version)
}

// ====================== 命令 ======================

#[tauri::command]
pub fn agent_state_get(app: AppHandle) -> Result<StateOut, String> {
    ensure_seeded(&app);
    let s = read_state(&app)?;
    Ok(StateOut {
        system_prompt: s.system_prompt,
        config: s.config,
        snapshots: snapshots(&app),
    })
}

#[tauri::command]
pub fn agent_state_save(
    app: AppHandle,
    system_prompt: String,
    config: AgentConfig,
    note: Option<String>,
) -> Result<u32, String> {
    let prompt = system_prompt.trim_end().to_string() + "\n";
    if prompt.trim().is_empty() {
        return Err("system_prompt 不能为空".into());
    }
    save_with_snapshot(&app, &prompt, &config, &note.unwrap_or_else(|| "手动保存".into()))
}

#[tauri::command]
pub fn agent_state_snapshots(app: AppHandle) -> Vec<SnapshotMeta> {
    snapshots(&app)
}

#[tauri::command]
pub fn agent_state_restore(app: AppHandle, version: u32) -> Result<u32, String> {
    let out = restore(&app, version)?;
    // provider 缓存无关；state 缓存已在 write_state 失效
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = AgentConfig::default();
        assert_eq!(c.version, 1);
        assert_eq!(c.max_turns, 8);
        assert!((c.temperature - 0.5).abs() < 1e-6);
        assert!(c.tools.is_empty());
    }

    #[test]
    fn seed_prompt_has_placeholder_and_tools() {
        assert!(SEED_PROMPT.contains("{{WORKBENCH}}"));
        assert!(SEED_PROMPT.contains("propose_choices"));
        assert!(SEED_PROMPT.contains("write_project_file"));
    }

    #[test]
    fn render_replaces_workbench() {
        let out = SEED_PROMPT.replace("{{WORKBENCH}}", "引擎快照 test ,内置服务端口 1");
        assert!(!out.contains("{{WORKBENCH}}"));
        assert!(out.contains("test"));
    }

    #[test]
    fn parse_skill_frontmatter() {
        let (name, desc, version) = parse_skill("---\nname: story-craft\ndescription: 写作守则\nversion: 2026.11.08.1\n---\n正文");
        assert_eq!(name, "story-craft");
        assert_eq!(desc, "写作守则");
        assert_eq!(version, "2026.11.08.1");
        // 无 frontmatter：全空，不炸
        let (n2, d2, v2) = parse_skill("没有frontmatter");
        assert!(n2.is_empty() && d2.is_empty() && v2.is_empty());
    }

    #[test]
    fn skill_name_rules() {
        assert!(skill_name_ok("story-craft"));
        assert!(skill_name_ok("SOP_1"));
        assert!(!skill_name_ok("../evil"));
        assert!(!skill_name_ok("中文名"));
        assert!(!skill_name_ok(""));
    }

    #[test]
    fn seed_skills_have_frontmatter_and_body() {
        for (name, content) in [
            ("story-craft", skill_seed_story_craft()),
            ("workbench-sop", skill_seed_workbench_sop()),
        ] {
            let (n, d, _) = parse_skill(content);
            assert_eq!(n, name);
            assert!(d.contains("：") || d.contains("：") || !d.is_empty());
            assert!(content.len() > 200);
        }
    }
}
