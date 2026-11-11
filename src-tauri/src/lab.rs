//! 进化实验室（v0.14.0，学 PenguinHarness 的 Default RSI Toolkit）：
//! 循环 = Benchmark 打分 → Optimizer 提可证伪假设做一处有边界的修改 →
//! 重新评估 → **分数严格提升才保留**（候选 prompt 不落盘，落选即天然回滚）。
//!
//! 信息隔离（本地单用户版）：Evaluator 见 rubric；Target 只见题干；
//! Optimizer 只见逐题分数与评估员理由，**不见 rubric**——防止抄答案。
//!
//! appdata/benchmarks/aoli-beat-v1/
//! ├── benchmark.json   清单（id/title/version/status）
//! ├── cases.json       题目（statement 与 rubric 分离存放）
//! └── scoreboard.json  评估记录（每 case 明细 + summary，可追溯）
//! 另：appdata/agent_traces.jsonl —— 每次 agent 任务一行，append-only。

use crate::agent_state;
use crate::llm;
use crate::settings;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

// ====================== Benchmark 数据结构 ======================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubricItem {
    pub name: String,
    pub weight: u32,
    pub guide: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub statement: Statement,
    pub rubric: Vec<RubricItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statement {
    /// 前后文（世界观与衔接信息）
    pub context: String,
    /// 原节拍
    pub beat: BeatIn,
    /// 改写要求（含约束）
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatIn {
    pub phase: String,
    pub title: String,
    pub desc: String,
}

#[derive(Serialize)]
pub struct BenchmarkOut {
    pub manifest: Value,
    pub cases: Vec<BenchmarkCase>,
}

const BENCH_DIR: &str = "benchmarks/aoli-beat-v1";
const BENCH_VERSION: &str = "2026.11.08.1";

fn bench_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(BENCH_DIR);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ====================== 内置 Benchmark（冻结题库） ======================
// 题干固定、不依赖用户项目（可复现）；rubric 是私有评分细则。

fn seed_cases() -> Vec<BenchmarkCase> {
    let mk = |id: &str, context: &str, phase: &str, title: &str, desc: &str, task: &str, items: Vec<(&str, u32, &str)>| BenchmarkCase {
        id: id.to_string(),
        statement: Statement {
            context: context.to_string(),
            beat: BeatIn { phase: phase.to_string(), title: title.to_string(), desc: desc.to_string() },
            task: task.to_string(),
        },
        rubric: items
            .into_iter()
            .map(|(name, weight, guide)| RubricItem { name: name.to_string(), weight, guide: guide.to_string() })
            .collect(),
    };
    vec![
        mk(
            "c1",
            "海边小城「雾港」。主角沈岸是刚到任的年轻验尸官，前任验尸官上周「失足」坠海。第一晚，他在停尸房整理移交的卷宗。",
            "建置",
            "雾港到任",
            "沈岸抵达雾港，交接验尸官的印信。海边湿冷，同事话少。他在停尸房清点卷宗，准备开始新工作。",
            "把这一拍改得更悬疑：制造不安与未知，不引入新角色与新设定；desc 60-90 字。",
            vec![
                ("悬念要素", 30, "是否制造威胁/未知/倒计时等悬念，而非同义改写"),
                ("事实保持", 25, "未引入新角色、新地点或超自然设定；与上下文不矛盾"),
                ("约束遵守", 20, "desc 长度 60-90 字；phase/title/desc 结构完整"),
                ("文学质量", 25, "画面具体可感，无陈词滥调（如「空气仿佛凝固」），无解释性废话"),
            ],
        ),
        mk(
            "c2",
            "近未来。证忆咖啡馆里，客人把回忆存进杯底，「每一杯都是一段被删除的人生」。店员阿绒发现自己名下有 60 条不属于自己的记忆。",
            "递进",
            "第 61 杯",
            "阿绒整理记忆柜，发现自己的编号后面多了一杯没有署名的记忆。她犹豫要不要打开。",
            "把这一拍改得更悬疑：让「未知的自己」成为威胁；不解释记忆的来源；desc 60-90 字。",
            vec![
                ("悬念要素", 30, "未知是否构成指向主角的威胁，而不是单纯的猎奇"),
                ("事实保持", 25, "未给出记忆来源的解释；与世界观（记忆有价、可存储）一致"),
                ("约束遵守", 20, "desc 长度 60-90 字；JSON 结构完整"),
                ("文学质量", 25, "细节推动不安，避免「她心中一惊」式的空泛描写"),
            ],
        ),
        mk(
            "c3",
            "雪夜的小站。灯守老人每晚会点亮站台的灯，「给赶路的人留一点暖」。今晚暴风雪封了山路，只有一个孩子还在等末班车。",
            "发展",
            "长夜灯",
            "老人给等车的孩子倒了一碗热汤，孩子问他为什么每晚都点灯。老人说总有人需要。",
            "把这一拍改得更温情：让「点灯」的原因有了具体来处，但不煽情不说教；desc 60-90 字。",
            vec![
                ("温情要素", 30, "是否给出具体、克制的情感来处，而非直接抒情或煽情"),
                ("事实保持", 25, "未引入新角色新设定；「灯等的是人」的意象保持连贯"),
                ("约束遵守", 20, "desc 长度 60-90 字；JSON 结构完整"),
                ("文学质量", 25, "用细节（物、动作）承载情感，无说教，无陈词滥调"),
            ],
        ),
        mk(
            "c4",
            "死亡循环地牢。冒险者第 7 次在同一张地图醒来，她已知道三步之后的地砖会射出弩箭。这次她决定改变路线。",
            "对抗",
            "第七次醒来",
            "冒险者在熟悉的房间里醒来，检查装备，向门口移动，躲避了弩箭机关。",
            "把这一拍改得更紧张：节奏更快、压迫感更强，用短句；不新增怪物；desc 60-90 字。",
            vec![
                ("紧张要素", 30, "是否用节奏与信息差制造压迫感，而非堆砌形容词"),
                ("事实保持", 25, "机关与循环设定一致；未新增怪物或新能力"),
                ("约束遵守", 20, "desc 长度 60-90 字；句子短促；JSON 结构完整"),
                ("文学质量", 25, "动作具体（可拍摄），无「心跳加速」式的 telling"),
            ],
        ),
        mk(
            "c5",
            "修仙世界的酒馆。AI 酒保「小酌」按 6 座的客人偏好自动调酒，但今晚全店只有一位客人——一个从不点单的剑客。",
            "喜剧",
            "只此一桌",
            "小酌给空荡的大堂每一桌都摆上一杯酒，说打烊前要「雨露均沾」。剑客看着自己面前突然出现的第三杯酒。",
            "把这一拍改得更幽默：靠错位与分寸感，不靠网络梗；desc 60-90 字。",
            vec![
                ("幽默要素", 30, "错位/自洽的荒谬是否好笑且有分寸，而非玩梗或贫嘴"),
                ("事实保持", 25, "AI 酒保逻辑自洽；未引入新角色新设定"),
                ("约束遵守", 20, "desc 长度 60-90 字；JSON 结构完整"),
                ("文学质量", 25, "包袱落在具体动作或台词上，节奏干净"),
            ],
        ),
        mk(
            "c6",
            "末班站台。传说末班车不为活人停。今晚列车破例停了，下来一个撑伞的乘客，站台上只剩加班到深夜的林小满。",
            "结局前",
            "末班车",
            "列车缓缓停稳，车门打开，一位撑黑伞的乘客走下来，站在原地没有动。林小满握紧了手机。",
            "为终局埋悲剧伏笔：读者事后重读会心一沉，但当下只觉异样；不点破结局；desc 60-90 字。",
            vec![
                ("伏笔质量", 30, "伏笔是否具体可回收（细节能在结局呼应），而非抽象不祥氛围"),
                ("事实保持", 25, "不点破结局、不引入新设定；乘客行为与「传说」一致"),
                ("约束遵守", 20, "desc 长度 60-90 字；JSON 结构完整"),
                ("文学质量", 25, "异样感来自细节；克制、不故弄玄虚"),
            ],
        ),
    ]
}

pub fn ensure_seeded(app: &AppHandle) {
    let dir = bench_dir(app);
    let manifest_path = dir.join("benchmark.json");
    let cases_path = dir.join("cases.json");
    if !manifest_path.is_file() {
        let manifest = json!({
            "id": "aoli-beat-v1",
            "title": "奥粒节拍改写",
            "description": "单拍剧情改写：悬疑/温情/紧张/幽默/伏笔 五种能力维度的冻结题库",
            "version": BENCH_VERSION,
            "status": "published",
            "cases": 6,
        });
        let _ = std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap());
    }
    if !cases_path.is_file() {
        let _ = std::fs::write(&cases_path, serde_json::to_string_pretty(&seed_cases()).unwrap());
    }
}

fn load_cases(app: &AppHandle) -> Result<Vec<BenchmarkCase>, String> {
    ensure_seeded(app);
    let path = bench_dir(app).join("cases.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 cases.json 失败：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 cases.json 失败：{e}"))
}

// ====================== 记分板 ======================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRecord {
    pub id: String,
    pub score: f64,
    pub duration_ms: u64,
    /// 评估员理由首条摘要（Optimizer 可见；不含 rubric 原文）
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreRecord {
    pub ts: u64,
    /// eval = 参照评估；candidate = 优化候选（adopted=true 才保留）
    pub kind: String,
    pub state_version: u32,
    pub model: String,
    pub avg_score: f64,
    pub avg_duration_ms: u64,
    pub calls: u32,
    pub summary_title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub adopted: Option<bool>,
    pub cases: Vec<CaseRecord>,
}

fn scoreboard_path(app: &AppHandle) -> std::path::PathBuf {
    bench_dir(app).join("scoreboard.json")
}

pub fn load_scoreboard(app: &AppHandle) -> Vec<ScoreRecord> {
    std::fs::read_to_string(scoreboard_path(app))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| serde_json::from_value(v["records"].clone()).ok())
        .unwrap_or_default()
}

fn append_record(app: &AppHandle, rec: ScoreRecord) -> Result<(), String> {
    let mut records = load_scoreboard(app);
    records.push(rec);
    let path = scoreboard_path(app);
    let text = serde_json::to_string_pretty(&json!({ "records": records })).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写记分板失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("落盘记分板失败：{e}"))
}

// ====================== 事件 ======================

#[derive(Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EvalEvent {
    RoundStart { kind: String, total_cases: usize },
    Proposal { hypothesis: String, change_summary: String, diff: Value },
    CaseDone { index: usize, total: usize, case_id: String, score: f64, duration_ms: u64, note: String },
    RoundDone { kind: String, avg_score: f64, prev_avg: Option<f64>, adopted: Option<bool>, state_version: u32 },
    Done { message: String },
    Error { message: String },
}

// ====================== 评估核心 ======================

fn fmt_statement(st: &Statement) -> String {
    format!(
        "上下文：{}\n当前节拍：{}「{}」{}",
        st.context, st.beat.phase, st.beat.title, st.beat.desc
    )
}

fn fmt_rubric(rubric: &[RubricItem]) -> String {
    rubric
        .iter()
        .map(|r| format!("- {}（{}分）：{}", r.name, r.weight, r.guide))
        .collect::<Vec<_>>()
        .join("\n")
}

struct RoundOutcome {
    avg: f64,
    cases: Vec<CaseRecord>,
    avg_duration_ms: u64,
    calls: u32,
}

/// 用给定 prompt 跑完整一轮评估（6 题串行，case 粒度上报与取消）。
async fn eval_with_prompt(
    cfg: &settings::ProviderCfg,
    state_version: u32,
    target_prompt: &str,
    cases: &[BenchmarkCase],
    ch: &Channel<EvalEvent>,
    cancelled: &AtomicBool,
) -> Result<RoundOutcome, String> {
    let _ = ch.send(EvalEvent::RoundStart {
        kind: "eval".into(),
        total_cases: cases.len(),
    });
    let mut total = 0f64;
    let mut total_dur = 0u64;
    let mut calls = 0u32;
    let mut records = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            let _ = ch.send(EvalEvent::Error { message: "已取消".into() });
            return Err("已取消".into());
        }
        let started = std::time::Instant::now();
        // ---- Target：只做题干，不见 rubric ----
        let target_sys = format!(
            "{target_prompt}\n\n【当前任务】你是节拍改写器：按用户的改写要求改写给定节拍。只输出严格 JSON：{{\"phase\":\"…\",\"title\":\"…\",\"desc\":\"…\"}}，不要任何其他文字或代码块标记。"
        );
        let target_user = format!(
            "{}\n改写要求：{}",
            fmt_statement(&case.statement),
            case.statement.task
        );
        let target_out = llm::post_chat(cfg, vec![
            json!({"role": "system", "content": target_sys}),
            json!({"role": "user", "content": target_user}),
        ], 500, 0.5)
        .await?;
        calls += 1;
        // ---- Evaluator：题干 + rubric + 待评输出 → 协议化分数 ----
        let eval_sys = "你是严苛的评估员。依据评分细则为改写结果打分（满分100）。只输出严格 JSON：{\"score\":<0-100整数>,\"reasons\":[{\"item\":\"评分项名\",\"score\":<该项得分>,\"note\":\"一句话理由\"}]}，各项得分之和必须等于 score。不要输出其他文字。";
        let eval_user = format!(
            "{}\n改写要求：{}\n\n【评分细则（满分100）】\n{}\n\n【待评改写】\n{}",
            fmt_statement(&case.statement),
            case.statement.task,
            fmt_rubric(&case.rubric),
            target_out.trim()
        );
        let eval_raw = llm::post_chat(cfg, vec![
            json!({"role": "system", "content": eval_sys}),
            json!({"role": "user", "content": eval_user}),
        ], 400, 0.2)
        .await?;
        calls += 1;
        let parsed: Value = serde_json::from_str(&llm::extract_json(&eval_raw))
            .map_err(|e| format!("case {} 评估输出不是合法 JSON：{e}", case.id))?;
        let score = parsed["score"].as_f64().unwrap_or(0.0).clamp(0.0, 100.0);
        let note = parsed["reasons"]
            .as_array()
            .and_then(|rs| rs.first())
            .and_then(|r| r["note"].as_str())
            .unwrap_or("")
            .chars()
            .take(80)
            .collect::<String>();
        let dur = started.elapsed().as_millis() as u64;
        total += score;
        total_dur += dur;
        records.push(CaseRecord { id: case.id.clone(), score, duration_ms: dur, note: note.clone() });
        let _ = ch.send(EvalEvent::CaseDone {
            index: i + 1,
            total: cases.len(),
            case_id: case.id.clone(),
            score,
            duration_ms: dur,
            note,
        });
    }
    let avg = if cases.is_empty() { 0.0 } else { total / cases.len() as f64 };
    Ok(RoundOutcome {
        avg: (avg * 100.0).round() / 100.0,
        cases: records,
        avg_duration_ms: if cases.is_empty() { 0 } else { total_dur / cases.len() as u64 },
        calls,
    })
}

fn build_record(
    kind: &str,
    state_version: u32,
    cfg: &settings::ProviderCfg,
    out: &RoundOutcome,
    summary_title: String,
    summary: &str,
    adopted: Option<bool>,
) -> ScoreRecord {
    ScoreRecord {
        ts: now_secs(),
        kind: kind.to_string(),
        state_version,
        model: cfg.model.clone(),
        avg_score: out.avg,
        avg_duration_ms: out.avg_duration_ms,
        calls: out.calls,
        summary_title,
        summary: summary.to_string(),
        adopted,
        cases: out.cases.clone(),
    }
}

/// 跑一轮参照评估（不改动 State）。
pub async fn run_eval(
    app: AppHandle,
    ch: Channel<EvalEvent>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let cfg = settings::load_provider(&app)
        .ok_or("小奥离线中：先在「模型中心」配置模型")?;
    ensure_seeded(&app);
    let cases = load_cases(&app)?;
    let state = agent_state::load_cached(&app);
    let prompt = agent_state::render_system_prompt(&app, "eval", 0);
    let out = eval_with_prompt(&cfg, state.config.version, &prompt, &cases, &ch, &cancelled).await?;
    let kind = if load_scoreboard(&app).is_empty() { "baseline" } else { "eval" };
    let rec = build_record(
        kind,
        state.config.version,
        &cfg,
        &out,
        format!("v{} 基线评估", state.config.version),
        "手动触发的参照评估",
        None,
    );
    let _ = ch.send(EvalEvent::RoundDone {
        kind: rec.kind.clone(),
        avg_score: out.avg,
        prev_avg: None,
        adopted: None,
        state_version: state.config.version,
    });
    append_record(&app, rec)?;
    llm::record_call("lab");
    let _ = ch.send(EvalEvent::Done {
        message: format!("评估完成：平均 {:.1} 分（{} 题，{} 次调用）", out.avg, out.cases.len(), out.calls),
    });
    Ok(())
}

// ====================== 优化循环（严格提升才保留） ======================

pub async fn run_optimize(
    app: AppHandle,
    ch: Channel<EvalEvent>,
    cancelled: Arc<AtomicBool>,
    rounds: u32,
) -> Result<(), String> {
    let cfg = settings::load_provider(&app)
        .ok_or("小奥离线中：先在「模型中心」配置模型")?;
    ensure_seeded(&app);
    let cases = load_cases(&app)?;

    for round in 1..=rounds.max(1) {
        if cancelled.load(Ordering::Relaxed) {
            let _ = ch.send(EvalEvent::Error { message: "已取消".into() });
            return Ok(());
        }
        let score = optimize_one_round(&app, &cfg, &cases, &ch, &cancelled, round).await?;
        // Penguin：达到目标分数提前停止
        if score >= 95.0 {
            let _ = ch.send(EvalEvent::Done {
                message: format!("第 {round} 轮后已达 {score:.1} 分（≥95 目标），提前收工"),
            });
            return Ok(());
        }
    }
    let _ = ch.send(EvalEvent::Done {
        message: format!("优化 {rounds} 轮完成——记分板见最新采纳版本"),
    });
    Ok(())
}

/// 单轮优化。返回该轮评估分（供多轮提前停止判断）。
async fn optimize_one_round(
    app: &AppHandle,
    cfg: &settings::ProviderCfg,
    cases: &[BenchmarkCase],
    ch: &Channel<EvalEvent>,
    cancelled: &AtomicBool,
    round: u32,
) -> Result<f64, String> {
    let state = agent_state::load_cached(app);

    // 前提：已有参照评估（Penguin：没有基线就没有可比的提升）
    let reference = load_scoreboard(app)
        .last()
        .filter(|r| r.kind == "baseline" || r.kind == "eval" || r.kind == "candidate")
        .cloned();
    let Some(reference) = reference else {
        let _ = ch.send(EvalEvent::Error {
            message: "还没有参照评估——先点「跑一轮评估」建立基线，才有可比的提升".into(),
        });
        return Err("no-reference".into());
    };

    if cancelled.load(Ordering::Relaxed) {
        let _ = ch.send(EvalEvent::Error { message: "已取消".into() });
        return Err("已取消".into());
    }

    // ---- Optimizer：只见分数+理由，不见 rubric ----
    let per_case = reference
        .cases
        .iter()
        .map(|c| format!("- {}：{:.0} 分｜评估员：{}", c.id, c.score, c.note))
        .collect::<Vec<_>>()
        .join("\n");
    let opt_sys = "你是 Agent 提示词优化器。给你：某助手的当前 system prompt，以及它在固定题库上最近一轮的逐题分数与评估员理由（评分细则不提供）。任务：提出一个可证伪的假设，并只做一处有边界的修改（调整某条规则的措辞、新增或删除一条规则、强调某个行为倾向），让弱项得分提高且不伤强项。禁止：改输出 JSON 格式要求、改工具语义、大改人设、堆砌新规则（一次最多动一处）。输出严格 JSON：{\"hypothesis\":\"假设（一句话）\",\"change_summary\":\"改了什么\",\"new_prompt\":\"完整的新 system prompt 文本\"}。new_prompt 必须是完整可用的 prompt，不是 diff。";
    // OPRO 历史带：历轮假设+结果喂回去，避免重复同类修改（学 OPRO：依据历史指令与分数优化）
    let history = load_scoreboard(app)
        .iter()
        .filter(|r| r.kind == "candidate")
        .rev()
        .take(5)
        .map(|r| {
            format!(
                "- 「{}」→ {:.1} 分，{}",
                r.summary.chars().take(60).collect::<String>(),
                r.avg_score,
                if r.adopted.unwrap_or(false) { "已采纳" } else { "已丢弃" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let history_band = if history.is_empty() {
        String::new()
    } else {
        format!("\n【历史尝试（不要重复同类修改）】\n{}\n", history)
    };
    let opt_user = format!(
        "【当前 system prompt】\n{}\n{}\n【最近一轮评估（平均 {:.1} 分）】\n{}\n\n请提出这一轮（第 {} 轮）的修改。",
        state.system_prompt, history_band, reference.avg_score, per_case, round
    );
    let opt_raw = llm::post_chat(&cfg, vec![
        json!({"role": "system", "content": opt_sys}),
        json!({"role": "user", "content": opt_user}),
    ], 2500, 0.4)
    .await?;
    llm::record_call("lab");
    let parsed: Value = serde_json::from_str(&llm::extract_json(&opt_raw))
        .map_err(|e| format!("优化器输出不是合法 JSON：{e}"))?;
    let hypothesis = parsed["hypothesis"].as_str().unwrap_or("").trim().to_string();
    let change_summary = parsed["change_summary"].as_str().unwrap_or("").trim().to_string();
    let new_prompt = parsed["new_prompt"].as_str().unwrap_or("").trim().to_string();
    if new_prompt.len() < 100 || !new_prompt.contains("奥粒") {
        let _ = ch.send(EvalEvent::Error {
            message: "优化器给出的 new_prompt 不合格（过短或丢了人设），本轮中止".into(),
        });
        return Err("invalid-candidate".into());
    }
    let diff = crate::agent::simple_diff(&state.system_prompt, &new_prompt);
    let _ = ch.send(EvalEvent::Proposal {
        hypothesis: hypothesis.clone(),
        change_summary: change_summary.clone(),
        diff: json!(diff),
    });

    if cancelled.load(Ordering::Relaxed) {
        let _ = ch.send(EvalEvent::Error { message: "已取消".into() });
        return Err("已取消".into());
    }

    // ---- Candidate 评估：候选 prompt 不落盘，落选即天然回滚 ----
    let out = eval_with_prompt(&cfg, state.config.version, &new_prompt, &cases, &ch, &cancelled).await?;
    let adopted = out.avg > reference.avg_score;
    if adopted {
        agent_state::save_with_snapshot(&app, &new_prompt, &state.config, &format!("optimizer 采纳：{}", change_summary))?;
    }
    let new_version = agent_state::load_cached(&app).config.version;
    let rec = build_record(
        "candidate",
        if adopted { new_version } else { state.config.version },
        &cfg,
        &out,
        format!("v{} 候选：{}", state.config.version + 1, change_summary),
        &hypothesis,
        Some(adopted),
    );
    let _ = ch.send(EvalEvent::RoundDone {
        kind: "candidate".into(),
        avg_score: out.avg,
        prev_avg: Some(reference.avg_score),
        adopted: Some(adopted),
        state_version: rec.state_version,
    });
    append_record(&app, rec)?;
    llm::record_call("lab");
    let verdict = if adopted {
        format!(
            "✅ 假设成立：{:.1} → {:.1}（+{:.1}），已采纳为新版本 v{}",
            reference.avg_score, out.avg, out.avg - reference.avg_score, new_version
        )
    } else {
        format!(
            "↩️ 假设不成立：{:.1} → {:.1}，候选已丢弃，State 保持 v{}",
            reference.avg_score, out.avg, state.config.version
        )
    };
    let _ = ch.send(EvalEvent::Done {
        message: format!("第 {} 轮：{}\n假设：{}", round, verdict, hypothesis),
    });
    Ok(out.avg)
}

// ====================== 命令 ======================

#[tauri::command]
pub fn benchmark_get(app: AppHandle) -> Result<BenchmarkOut, String> {
    ensure_seeded(&app);
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(bench_dir(&app).join("benchmark.json"))
            .map_err(|e| format!("读取 benchmark.json 失败：{e}"))?,
    )
    .map_err(|e| format!("解析 benchmark.json 失败：{e}"))?;
    let cases = load_cases(&app)?;
    Ok(BenchmarkOut { manifest, cases })
}

#[tauri::command]
pub fn lab_scoreboard_get(app: AppHandle) -> Vec<ScoreRecord> {
    load_scoreboard(&app)
}

#[tauri::command]
pub async fn lab_eval_run(
    app: AppHandle,
    tasks: tauri::State<'_, crate::AiTasks>,
    channel: Channel<EvalEvent>,
) -> Result<String, String> {
    settings::load_provider(&app).ok_or("小奥离线中：先在「模型中心」配置模型")?;
    let flag = Arc::new(AtomicBool::new(false));
    let id = crate::next_task_id();
    tasks.0.lock().unwrap().insert(id.clone(), flag.clone());
    let app2 = app.clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_eval(app2.clone(), channel.clone(), flag).await {
            if e != "已取消" {
                let _ = channel.send(EvalEvent::Error { message: e });
            }
        }
        if let Some(t) = app2.try_state::<crate::AiTasks>() {
            t.0.lock().unwrap().remove(&task_id);
        }
    });
    Ok(id)
}

#[tauri::command]
pub async fn lab_optimize_run(
    app: AppHandle,
    tasks: tauri::State<'_, crate::AiTasks>,
    channel: Channel<EvalEvent>,
    rounds: Option<u32>,
) -> Result<String, String> {
    settings::load_provider(&app).ok_or("小奥离线中：先在「模型中心」配置模型")?;
    let rounds = rounds.unwrap_or(1).clamp(1, 5);
    let flag = Arc::new(AtomicBool::new(false));
    let id = crate::next_task_id();
    tasks.0.lock().unwrap().insert(id.clone(), flag.clone());
    let app2 = app.clone();
    let task_id = id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_optimize(app2.clone(), channel.clone(), flag, rounds).await {
            if e != "已取消" && e != "no-reference" && e != "invalid-candidate" {
                let _ = channel.send(EvalEvent::Error { message: e });
            }
        }
        if let Some(t) = app2.try_state::<crate::AiTasks>() {
            t.0.lock().unwrap().remove(&task_id);
        }
    });
    Ok(id)
}

// ====================== agent run trace（append-only，可审计） ======================

/// 每完成一次 agent 任务追加一行（Penguin：Trace 是恢复与审计的唯一事实源；
/// 本地版为审计辅助，不做恢复源——恢复仍以 session json 为准）。
#[allow(clippy::too_many_arguments)]
pub fn trace_agent_run(
    app: &AppHandle,
    session_id: &str,
    task_id: &str,
    model: &str,
    ok: bool,
    duration_ms: u64,
    error: Option<&str>,
    tokens_in: u64,
    tokens_out: u64,
) {
    let line = json!({
        "ts": now_secs(),
        "session_id": session_id,
        "task_id": task_id,
        "model": model,
        "ok": ok,
        "duration_ms": duration_ms,
        "error": error,
        "tokens_in": tokens_in,
        "tokens_out": tokens_out,
    });
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("agent_traces.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

// ====================== 单测 ======================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_benchmark_shape() {
        let cases = seed_cases();
        assert_eq!(cases.len(), 6);
        for c in &cases {
            assert!(!c.id.is_empty());
            assert!(!c.statement.task.is_empty());
            assert!(!c.statement.beat.title.is_empty());
            let total: u32 = c.rubric.iter().map(|r| r.weight).sum();
            assert_eq!(total, 100, "case {} rubric 权重和应为 100", c.id);
            assert!(c.rubric.len() >= 3);
        }
    }

    #[test]
    fn rubric_fmt_lists_items() {
        let cases = seed_cases();
        let s = fmt_rubric(&cases[0].rubric);
        assert!(s.contains("悬念要素"));
        assert!(s.contains("30分"));
    }

    #[test]
    fn statement_fmt_contains_all() {
        let cases = seed_cases();
        let s = fmt_statement(&cases[0].statement);
        assert!(s.contains("上下文"));
        assert!(s.contains(&cases[0].statement.beat.title));
    }

    #[test]
    fn record_serde_roundtrip() {
        let rec = ScoreRecord {
            ts: 1,
            kind: "eval".into(),
            state_version: 3,
            model: "m".into(),
            avg_score: 82.5,
            avg_duration_ms: 1000,
            calls: 12,
            summary_title: "t".into(),
            summary: "s".into(),
            adopted: None,
            cases: vec![CaseRecord { id: "c1".into(), score: 82.0, duration_ms: 900, note: "n".into() }],
        };
        let text = serde_json::to_string(&rec).unwrap();
        let back: ScoreRecord = serde_json::from_str(&text).unwrap();
        assert_eq!(back.avg_score, 82.5);
        assert_eq!(back.cases.len(), 1);
    }
}
