//! 可选 AI 能力：调 OpenAI 兼容端点做剧情定制生成。
//! 使用用户在模型中心配置的 provider（云端或本地 Ollama 均可）。

use crate::settings::ProviderCfg;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedBeat {
    pub phase: String,
    pub title: String,
    pub desc: String,
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

/// 调 OpenAI 兼容 chat/completions。anthropic 协议不同，直接返回提示错误。
pub async fn chat(cfg: &ProviderCfg, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    let messages = vec![
        json!({"role": "system", "content": system}),
        json!({"role": "user", "content": user}),
    ];
    post_chat(cfg, messages, max_tokens, 0.9).await
}

/// 一次 chat/completions 调用（消息数组由调用方组装）。
async fn post_chat(
    cfg: &ProviderCfg,
    messages: Vec<serde_json::Value>,
    max_tokens: u32,
    temperature: f32,
) -> Result<String, String> {
    if cfg.kind == "anthropic" {
        return Err("AI 定制暂不支持 Anthropic 端点，请在模型中心选择智谱 / DeepSeek / Ollama 等 OpenAI 兼容服务".into());
    }
    let url = chat_endpoint(&cfg.base);
    let body = json!({
        "model": cfg.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": temperature,
    });

    let client = reqwest::Client::new();
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
    let text = resp.text().await.map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        let short: String = text.chars().take(400).collect();
        return Err(format!("服务返回 {}：{}", status.as_u16(), short));
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
        let role = if t.role == "assistant" { "assistant" } else { "user" };
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
    let no_fence = no_fence.strip_suffix("```").map(|s| s.trim()).unwrap_or(no_fence);
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
) -> Result<GeneratedStory, String> {
    let system = "你是一位资深游戏叙事设计师。用户给你一个玩法类型和自己的想法，你产出原创、克制、有钩子的中文剧情设计。只输出 JSON，不要任何解释文字。";
    let rhythm = framework_rhythm
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("三幕式（前 1/3 建置、中间对抗升级、最后结局）");
    let user = format!(
        "玩法类型：{}（{}）\n用户想法：{}\n\n请输出严格 JSON，结构：{{\"premise\":\"一句话世界观前提，60字内\",\"beats\":[{{\"phase\":\"阶段名\",\"title\":\"节拍标题\",\"desc\":\"2-3句剧情描述\"}}]}}\n要求：beats 恰好 {} 条，节奏遵循：{rhythm}。desc 要具体有画面感，避免陈词滥调。",
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
    let fmt = |b: Option<&Beat>| b.map(|b| format!("「{}」{}", b.title, b.desc)).unwrap_or_default();
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
    serde_json::from_str::<Beat>(&json_str).map_err(|e| format!("重写结果无法解析（{e}），请重试一次"))
}
