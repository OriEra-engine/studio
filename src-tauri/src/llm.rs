//! 可选 AI 能力：调 OpenAI 兼容端点做剧情定制生成。
//! 使用用户在模型中心配置的 provider（云端或本地 Ollama 均可）。

use crate::settings::ProviderCfg;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// 节拍分支选项（v0.3 叙事分支）：可选,旧项目/旧模型输出没有也能解析。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatChoice {
    pub label: String,
    /// 跳转目标节拍（0 起）；省略/null/-1 = 顺延下一拍
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<isize>,
    /// 资源效果「资源名+数量 / 资源名-数量」；省略 = 无
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedBeat {
    pub phase: String,
    pub title: String,
    pub desc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<BeatChoice>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedStory {
    pub premise: String,
    pub beats: Vec<GeneratedBeat>,
}

fn chat_endpoint(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.to_string()
    } else {
        format!("{b}/chat/completions")
    }
}

/// 调一次对话补全（按 provider kind 分派 OpenAI 兼容 / Anthropic 协议）。
pub async fn chat(
    cfg: &ProviderCfg,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let messages = vec![
        json!({"role": "system", "content": system}),
        json!({"role": "user", "content": user}),
    ];
    post_chat(cfg, messages, max_tokens, 0.9).await
}

/// 一次对话补全调用（消息数组由调用方组装；首条 system 消息按协议归位）。
/// lab.rs 的评估/优化循环复用（非流式、case 粒度上报）。
pub(crate) async fn post_chat(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    let client = crate::server::http_client();
    if cfg.kind == "anthropic" {
        return post_anthropic(&client, cfg, messages, max_tokens, temperature).await;
    }

    let url = chat_endpoint(&cfg.base);
    let body = request_body(cfg, messages, max_tokens, temperature, false, None);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format_http_error(status.as_u16(), &text));
    }

    #[derive(Deserialize)]
    struct ChatResp {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(Deserialize)]
    struct Msg {
        content: String,
    }
    let parsed: ChatResp = serde_json::from_str(&text).map_err(|e| format!("解析响应失败：{e}"))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "模型未返回内容".to_string())
}

/// 请求体统一构造：DeepSeek V4 默认 high 档思考会吃光 max_tokens 导致空正文/半句截断
/// （引擎侧已踩平的坑），这里注入 thinking disabled；stream 标记供流式通道复用；
/// tools 为 Agent 工作台的工具 schema（OpenAI function calling 格式）。
fn request_body(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
    stream: bool,
    tools: Option<&[serde_json::Value]>,
) -> serde_json::Value {
    let mut body = json!({
        "model": cfg.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
    });
    if stream {
        body["stream"] = json!(true);
        // token 计量：流末尾追加一帧 choices=[] + usage（v0.15.0 成本可观测）
        if cfg.kind != "anthropic" {
            body["stream_options"] = json!({"include_usage": true});
        }
    }
    if let Some(t) = tools {
        body["tools"] = json!(t);
    }
    if cfg.kind == "deepseek" {
        body["thinking"] = json!({"type": "disabled"});
    }
    body
}

/// Anthropic Messages 协议：system 独立字段、x-api-key 鉴权、content 为分块数组。
async fn post_anthropic(
    client: &reqwest::Client,
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    let b = cfg.base.trim_end_matches('/');
    let url = if b.ends_with("/messages") {
        b.to_string()
    } else {
        format!("{b}/messages")
    };
    // anthropic 不接受 messages 里的 system 角色，摘出来放独立字段
    let system = messages
        .first()
        .filter(|m| m["role"] == "system")
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    let turns: Vec<serde_json::Value> = messages
        .into_iter()
        .filter(|m| m["role"] != "system")
        .collect();
    let mut body = json!({
        "model": cfg.model,
        "max_tokens": max_tokens,
        "temperature": temperature,
        "messages": turns,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }

    let resp = client
        .post(&url)
        .header("x-api-key", &cfg.key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format_http_error(status.as_u16(), &text));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析响应失败：{e}"))?;
    let mut out = String::new();
    if let Some(blocks) = parsed["content"].as_array() {
        for blk in blocks {
            if blk["type"] == "text" {
                if let Some(t) = blk["text"].as_str() {
                    out.push_str(t);
                }
            }
        }
    }
    if out.is_empty() {
        return Err("模型未返回内容".to_string());
    }
    Ok(out)
}

/// HTTP 错误 → 说人话的中文提示（带端点原文片段便于排查）。
fn format_http_error(status: u16, body: &str) -> String {
    let hint = match status {
        401 | 403 => "API Key 无效或无权限——请到「模型中心」检查密钥",
        404 => "端点或模型名不存在——请检查 Base URL 与模型名",
        429 => "触发限流或额度不足——稍后再试，或更换免费档模型",
        500..=599 => "模型服务端暂时故障——稍后再试",
        _ => "请求被服务端拒绝",
    };
    let short: String = body.chars().take(300).collect();
    format!("服务返回 {status}：{hint}｜{short}")
}

/// Copilot 对话历史中的一轮（"user" | "assistant"）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// Studio 助手「奥粒」多轮对话：system + 历史 + 本轮输入。
pub async fn copilot_chat(
    cfg: &ProviderCfg,
    system: &str,
    history: &[ChatTurn],
    user: &str,
) -> Result<String, String> {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for t in history {
        let role = if t.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        messages.push(json!({"role": role, "content": t.content}));
    }
    messages.push(json!({"role": "user", "content": user}));
    post_chat(cfg, messages, 1024, 0.7).await
}

/// 从模型输出中提取第一个 JSON 数组/对象（容忍 ```json 代码块包裹）。
pub fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();
    let no_fence = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim())
        .unwrap_or(trimmed);
    let no_fence = no_fence
        .strip_suffix("```")
        .map(|s| s.trim())
        .unwrap_or(no_fence);
    no_fence.to_string()
}

/// 剧情定制提示词（阻塞与流式两条通道共用，避免两处漂移）。
pub fn story_prompt(
    gameplay_name: &str,
    tagline: &str,
    user_idea: &str,
    beat_count: usize,
    framework_rhythm: Option<&str>,
    economy_names: &[String],
) -> (String, String) {
    let system = "你是一位资深游戏叙事设计师。用户给你一个玩法类型和自己的想法，你产出原创、克制、有钩子的中文剧情设计。只输出 JSON，不要任何解释文字。";
    let econ = if economy_names.is_empty() {
        "金币".to_string()
    } else {
        economy_names.join("、")
    };
    let rhythm = framework_rhythm
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("三幕式（前 1/3 建置、中间对抗升级、最后结局）");
    let user = format!(
        "玩法类型：{}（{}）
用户想法：{}
游戏内资源：{econ}

请输出严格 JSON，结构：{{\"premise\":\"一句话世界观前提，60字内\",\"beats\":[{{\"phase\":\"阶段名\",\"title\":\"节拍标题\",\"desc\":\"2-3句剧情描述\",\"choices\":\"可选的分支数组\"}}]}}
要求：beats 恰好 {} 条，节奏遵循：{rhythm}。desc 要具体有画面感，避免陈词滥调。
choices 只在 2~3 个关键节拍（非最后一拍）出现，其余节拍省略该字段；每个元素形如 {{\"label\":\"不超过12字的选择\",\"next\":目标节拍序号(0起,顺延填-1),\"effect\":\"资源名+数量 或 资源名-数量(可省略)\"}}，每个 choices 数组给 2 个选项，两条路都要讲得通。",
        gameplay_name,
        tagline,
        if user_idea.trim().is_empty() { "（无特定想法，请自由发挥）" } else { user_idea },
        beat_count,
    );
    (system.to_string(), user)
}

/// 剧情定制：基于玩法包与用户想法生成前提 + 节拍。
/// `framework_rhythm` 为叙事框架的节奏要求（名称+阶段+节拍提示）；
/// 不传时回退三幕式。解析失败自动重试一次。
pub async fn generate_story(
    cfg: &ProviderCfg,
    gameplay_name: &str,
    tagline: &str,
    user_idea: &str,
    beat_count: usize,
    framework_rhythm: Option<&str>,
    economy_names: &[String],
) -> Result<GeneratedStory, String> {
    let (system, user) = story_prompt(
        gameplay_name,
        tagline,
        user_idea,
        beat_count,
        framework_rhythm,
        economy_names,
    );

    let mut last_err = String::from("未知解析错误");
    for attempt in 0..2 {
        let mut prompt = user.clone();
        if attempt > 0 {
            prompt.push_str("\n（注意：你上一次的输出不是合法 JSON，这次只输出 JSON 本体。）");
        }
        let raw = chat(cfg, &system, &prompt, 2000).await?;
        let json_str = extract_json(&raw);
        match serde_json::from_str::<GeneratedStory>(&json_str) {
            Ok(story) => return Ok(story),
            Err(e) => last_err = format!("生成的剧情格式无法解析（{e}）"),
        }
    }
    Err(format!("{last_err}，已自动重试一次仍失败，请再点一次生成"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Beat {
    pub phase: String,
    pub title: String,
    pub desc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<BeatChoice>>,
}

/// 重写预览：新旧两版对比，采纳前不落盘（Codex 式 diff 确认制）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatPreview {
    pub old: Beat,
    pub new: Beat,
}

/// 单拍重写：保持前后文连贯地重写一个节拍（v0.2 编辑器核心）。
#[allow(clippy::too_many_arguments)]
pub async fn regenerate_beat(
    cfg: &ProviderCfg,
    game_name: &str,
    premise: &str,
    index: usize,
    total: usize,
    prev: Option<&Beat>,
    cur: &Beat,
    next: Option<&Beat>,
    instruction: &str,
) -> Result<Beat, String> {
    let fmt = |b: Option<&Beat>| {
        b.map(|b| format!("「{}」{}", b.title, b.desc))
            .unwrap_or_default()
    };
    let system = "你是一位资深游戏叙事设计师，正在修改一部游戏剧情的其中一个节拍。保持世界观、语气和前后衔接，只重写这一个节拍。只输出 JSON，不要任何解释。";
    let user = format!(
        "游戏：{game_name}\n世界观前提：{premise}\n该节拍位置：第 {index}/{total} 拍\n前一拍：{prev}\n当前要重写的节拍：「{ct}」{cd}\n后一拍：{nxt}\n修改要求：{ins}\n\n输出严格 JSON：{{\"phase\":\"阶段名（与原节拍一致或更准确）\",\"title\":\"新节拍标题\",\"desc\":\"2-3句新剧情描述\"}}",
        prev = fmt(prev),
        ct = cur.title,
        cd = cur.desc,
        nxt = fmt(next),
        ins = if instruction.trim().is_empty() { "自由发挥，写得更精彩" } else { instruction },
    );
    // 单拍重写是外科手术式修订：低温度保前后一致，不用整篇生成的 0.9
    let raw = post_chat(
        cfg,
        vec![
            serde_json::json!({"role": "system", "content": system}),
            serde_json::json!({"role": "user", "content": user}),
        ],
        600,
        0.4,
    )
    .await?;
    let json_str = extract_json(&raw);
    let mut beat: Beat = serde_json::from_str(&json_str)
        .map_err(|e| format!("重写结果无法解析（{e}），请重试一次"))?;
    // 重写只动文案;已有分支选项原样保留(编辑器可再改)
    beat.choices = cur.choices.clone();
    Ok(beat)
}

// ====================== 流式通道（SSE） ======================

use crate::sse;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};

/// 一次调用的 token 用量（学 Penguin 成本中心：消耗必须可见）。
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input + self.output
    }
}

/// 打开一条流式补全连接（chat_stream / chat_stream_agent 共用）。
/// 学 Penguin「错误收敛为消息」：retryable 错误（网络/超时/429/5xx）在
/// 建连阶段自动重试 ≤2 次、指数退避；401/403/404 等请求性错误直接失败。
async fn open_stream(
    cfg: &ProviderCfg,
    body: serde_json::Value,
    anthropic: bool,
) -> Result<reqwest::Response, String> {
    let mut attempt = 0u32;
    loop {
        match open_stream_once(cfg, &body, anthropic).await {
            Ok(r) => return Ok(r),
            Err(e) if attempt < 2 && is_retryable_err(&e) => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(800 * u64::from(attempt))).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// 网络抖动/限流/服务端故障 → 可自动重试；鉴权与请求错误 → 不重试。
fn is_retryable_err(err: &str) -> bool {
    let lowered = err.to_lowercase();
    lowered.contains("429")
        || lowered.contains("500")
        || lowered.contains("502")
        || lowered.contains("503")
        || lowered.contains("504")
        || lowered.contains("请求超时")
        || lowered.contains("网络连接失败")
        || lowered.contains("连接被中断")
        || lowered.contains("暂时故障")
        || lowered.contains("timed out")
        || lowered.contains("connection")
}

async fn open_stream_once(
    cfg: &ProviderCfg,
    body: &serde_json::Value,
    anthropic: bool,
) -> Result<reqwest::Response, String> {
    let client = crate::server::http_client();
    let url = if anthropic {
        let b = cfg.base.trim_end_matches('/');
        if b.ends_with("/messages") {
            b.to_string()
        } else {
            format!("{b}/messages")
        }
    } else {
        chat_endpoint(&cfg.base)
    };
    let req = if anthropic {
        client
            .post(&url)
            .header("x-api-key", &cfg.key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(180))
            .json(&body)
    } else {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", cfg.key))
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(180))
            .json(&body)
    };
    let resp = req.send().await.map_err(|e| {
        let (title, hint) = sse::classify_error(None, &e.to_string());
        format!("{title}：{hint}")
    })?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format_http_error(status.as_u16(), &text));
    }
    Ok(resp)
}

/// 流式对话补全：每收到一段正文增量就调 `on_delta`；
/// `cancelled` 置真后在下个增量边界停止读取。返回完整正文。
/// `on_delta` 返回 false 表示调用方要求提前停止（同样返回已累计文本）。
pub async fn chat_stream(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
    on_delta: &mut (dyn FnMut(&str) -> bool + Send),
    cancelled: &AtomicBool,
) -> Result<(String, TokenUsage), String> {
    let anthropic = cfg.kind == "anthropic";
    let body = request_body(cfg, messages, max_tokens, temperature, true, None);
    let resp = open_stream(cfg, body, anthropic).await?;

    let mut stream = resp.bytes_stream();
    let mut parser = sse::SseParser::new();
    let mut full = String::new();
    let mut usage = TokenUsage::default();

    while let Some(item) = stream.next().await {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let chunk = item.map_err(|e| {
            let (title, hint) = sse::classify_error(None, &e.to_string());
            format!("{title}：{hint}")
        })?;
        for payload in parser.feed(&chunk) {
            if payload.trim() == "[DONE]" {
                return Ok((full, usage));
            }
            if let Some(msg) = sse::error_payload(&payload) {
                let (title, hint) = sse::classify_error(None, &msg);
                return Err(format!("{title}：{hint}"));
            }
            // usage 帧采集（OpenAI 收尾帧 / Anthropic message_start+delta）
            if anthropic {
                if let Some(i) = sse::anthropic_usage_in(&payload) {
                    usage.input = i;
                }
                if let Some(o) = sse::anthropic_usage_out(&payload) {
                    usage.output = o;
                }
            } else if let Some((i, o)) = sse::openai_usage(&payload) {
                usage.input = i;
                usage.output = o;
            }
            let delta = if anthropic {
                sse::anthropic_delta(&payload)
            } else {
                sse::openai_delta(&payload)
            };
            if let Some(d) = delta {
                if d.is_empty() {
                    continue;
                }
                full.push_str(&d);
                if !on_delta(&d) {
                    return Ok((full, usage));
                }
            }
        }
    }
    for payload in parser.flush() {
        if let Some(d) = if anthropic {
            sse::anthropic_delta(&payload)
        } else {
            sse::openai_delta(&payload)
        } {
            full.push_str(&d);
        }
    }
    if full.is_empty() {
        return Err("模型未返回内容（流提前结束），请重试一次".into());
    }
    Ok((full, usage))
}

// ====================== Agent 工作台：带工具调用的流式一轮 ======================

/// 一轮结束后完整收口的工具调用（分片已按 index 拼齐）。
#[derive(Debug, Clone)]
pub struct CompletedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 一轮流式结果：纯文本收尾，或模型请求了工具调用。
#[derive(Debug)]
pub enum TurnOutcome {
    Text(String),
    ToolCalls(Vec<CompletedToolCall>),
}

/// Agent 一流的回调集合：文本增量 / 工具开始 / 工具参数增量。
pub struct AgentTurnEvents<'a> {
    pub on_delta: &'a mut (dyn FnMut(&str) -> bool + Send),
    pub on_tool_start: &'a mut (dyn FnMut(&str, &str) + Send),
    pub on_tool_args: &'a mut (dyn FnMut(&str, &str) + Send),
}

/// 带工具 schema 的流式一轮：文本 delta 与 tool_calls 分片同时上报。
/// 结束时按 finish_reason / 是否有工具分片收口为 Text 或 ToolCalls。
pub async fn chat_stream_agent(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
    tools: &[serde_json::Value],
    events: AgentTurnEvents<'_>,
    cancelled: &AtomicBool,
) -> Result<(TurnOutcome, TokenUsage), String> {
    let body = request_body(cfg, messages, max_tokens, temperature, true, Some(tools));
    let resp = open_stream(cfg, body, false).await?;

    let mut stream = resp.bytes_stream();
    let mut parser = sse::SseParser::new();
    let mut full = String::new();
    let mut usage = TokenUsage::default();
    // 分片拼装：index → 槽位；id/name 首帧出现，arguments 逐帧追加
    let mut calls: Vec<CompletedToolCall> = Vec::new();
    let mut slot_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut saw_tool_calls = false;

    while let Some(item) = stream.next().await {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let chunk = item.map_err(|e| {
            let (title, hint) = sse::classify_error(None, &e.to_string());
            format!("{title}：{hint}")
        })?;
        for payload in parser.feed(&chunk) {
            if payload.trim() == "[DONE]" {
                return Ok((finish_agent_turn(full, calls, saw_tool_calls), usage));
            }
            if let Some(msg) = sse::error_payload(&payload) {
                let (title, hint) = sse::classify_error(None, &msg);
                return Err(format!("{title}：{hint}"));
            }
            if let Some((i, o)) = sse::openai_usage(&payload) {
                usage.input = i;
                usage.output = o;
            }
            // 文本增量（可与工具分片同帧混出）
            if let Some(d) = sse::openai_delta(&payload) {
                if !d.is_empty() {
                    full.push_str(&d);
                    if !(events.on_delta)(&d) {
                        return Ok((finish_agent_turn(full, calls, saw_tool_calls), usage));
                    }
                }
            }
            // 工具分片
            for tc in sse::openai_tool_delta(&payload) {
                saw_tool_calls = true;
                let slot = *slot_of.entry(tc.index).or_insert_with(|| {
                    calls.push(CompletedToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                    calls.len() - 1
                });
                let call = &mut calls[slot];
                if let Some(id) = tc.id {
                    call.id = id;
                }
                if let Some(name) = tc.name {
                    if call.name != name {
                        let is_first = call.name.is_empty();
                        call.name = name;
                        if is_first {
                            let cid = if call.id.is_empty() {
                                format!("call-{slot}")
                            } else {
                                call.id.clone()
                            };
                            (events.on_tool_start)(&cid, &call.name);
                        }
                    }
                }
                if let Some(frag) = tc.arguments_fragment {
                    call.arguments.push_str(&frag);
                    let cid = if call.id.is_empty() {
                        format!("call-{slot}")
                    } else {
                        call.id.clone()
                    };
                    (events.on_tool_args)(&cid, &frag);
                }
            }
            if sse::openai_finished_tool_calls(&payload) {
                saw_tool_calls = true;
            }
        }
    }
    for payload in parser.flush() {
        if let Some(d) = sse::openai_delta(&payload) {
            full.push_str(&d);
        }
    }
    Ok((finish_agent_turn(full, calls, saw_tool_calls), usage))
}

/// 一轮收口：有工具调用 → ToolCalls（id 缺省补 call-N），否则 Text。
fn finish_agent_turn(
    full: String,
    mut calls: Vec<CompletedToolCall>,
    saw_tool_calls: bool,
) -> TurnOutcome {
    if saw_tool_calls && !calls.is_empty() {
        for (i, c) in calls.iter_mut().enumerate() {
            if c.id.is_empty() {
                c.id = format!("call-{i}");
            }
            if c.arguments.trim().is_empty() {
                c.arguments = "{}".into();
            }
        }
        TurnOutcome::ToolCalls(std::mem::take(&mut calls))
    } else {
        TurnOutcome::Text(full)
    }
}

/// 4xx 且错误原文提及 tool/function → 大概率该端点不支持 function calling
/// （宽容启发式：误判也只是降级为纯对话，永远可用）。
pub fn looks_like_tools_rejected(err: &str) -> bool {
    let lowered = err.to_lowercase();
    (err.contains("400") || err.contains("422") || err.contains("404"))
        && (lowered.contains("tool") || lowered.contains("function"))
}

/// 流式剧情定制：一边吐增量一边积累，结束后解析 JSON（单次尝试，失败给可行动的错误）。
pub async fn generate_story_stream(
    cfg: &ProviderCfg,
    gameplay_name: &str,
    tagline: &str,
    user_idea: &str,
    beat_count: usize,
    framework_rhythm: Option<&str>,
    economy_names: &[String],
    on_delta: &mut (dyn FnMut(&str) -> bool + Send),
    cancelled: &AtomicBool,
) -> Result<GeneratedStory, String> {
    let (system, user) = story_prompt(
        gameplay_name,
        tagline,
        user_idea,
        beat_count,
        framework_rhythm,
        economy_names,
    );
    let (raw, _usage) = chat_stream(
        cfg,
        vec![
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": user}),
        ],
        2000,
        0.9,
        on_delta,
        cancelled,
    )
    .await?;
    let json_str = extract_json(&raw);
    serde_json::from_str::<GeneratedStory>(&json_str)
        .map_err(|e| format!("生成的剧情格式无法解析（{e}），点「重试」再来一次"))
}

/// 流式 Copilot 对话（奥粒面板）。
pub async fn copilot_chat_stream(
    cfg: &ProviderCfg,
    system: &str,
    history: &[ChatTurn],
    user: &str,
    on_delta: &mut (dyn FnMut(&str) -> bool + Send),
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let mut messages = vec![json!({"role": "system", "content": system})];
    for t in history {
        let role = if t.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        messages.push(json!({"role": role, "content": t.content}));
    }
    messages.push(json!({"role": "user", "content": user}));
    let (text, _usage) = chat_stream(cfg, messages, 1024, 0.7, on_delta, cancelled).await?;
    Ok(text)
}

// ---- 月度调用账本（2026-10-26 付费用户测评 P0：对话消耗不可见）----
// 只记成功调用次数、按月分桶、不折算价格——价目在供应商侧，诚实不猜。
// 落 appdata/chat_ledger.json：{"2026-10": {"copilot": 12, "story": 3}}
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

static LEDGER_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// setup 时注入账本文件路径（appdata）。
pub fn init_ledger(path: PathBuf) {
    if let Ok(mut guard) = LEDGER_PATH.lock() {
        *guard = Some(path);
    }
}

/// 记一次成功调用（fail-open：账本任何故障都不影响主流程）。
/// v0.13.0：读改写挪到独立线程 fire-and-forget——流式收尾不再被写盘阻塞。
pub fn record_call(kind: &str) {
    let path = {
        let Ok(guard) = LEDGER_PATH.lock() else {
            return;
        };
        let Some(path) = guard.as_ref() else { return };
        path.clone()
    };
    let kind = kind.to_string();
    std::thread::spawn(move || {
        let month = current_month();
        let mut root: serde_json::Map<String, serde_json::Value> =
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
        let bucket = root.entry(month).or_insert_with(|| json!({}));
        if let Some(obj) = bucket.as_object_mut() {
            let c = obj.entry(kind).or_insert(json!(0));
            *c = json!(c.as_u64().unwrap_or(0) + 1);
        }
        if let Ok(text) = serde_json::to_string(&root) {
            // 原子写：temp+rename，崩溃不留半个账本
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    });
}

fn current_month() -> String {
    // days since epoch → civil year/month（Hinnant 算法），不引 chrono
    // 东八区口径（Studio 主场为中国大陆用户；无 chrono 依赖，+8h 硬
    // 口径并注释——其他时区用户月份边界差几小时，可接受）
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() + 8 * 3600) / 86400)
        .unwrap_or(0) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}", if m <= 2 { y + 1 } else { y }, m)
}

/// 当月各 kind 的调用次数（前端月度展示用）。
pub fn month_calls() -> serde_json::Map<String, serde_json::Value> {
    let Ok(guard) = LEDGER_PATH.lock() else {
        return Default::default();
    };
    let Some(path) = guard.as_ref() else {
        return Default::default();
    };
    let root: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    root.get(&current_month())
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}
