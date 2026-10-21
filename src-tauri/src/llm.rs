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
async fn post_chat(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    if cfg.kind == "anthropic" {
        return post_anthropic(&client, cfg, messages, max_tokens, temperature).await;
    }

    let url = chat_endpoint(&cfg.base);
    let body = json!({
        "model": cfg.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
    });

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

    let mut last_err = String::from("未知解析错误");
    for attempt in 0..2 {
        let mut prompt = user.clone();
        if attempt > 0 {
            prompt.push_str("\n（注意：你上一次的输出不是合法 JSON，这次只输出 JSON 本体。）");
        }
        let raw = chat(cfg, system, &prompt, 2000).await?;
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
