//! SSE 流解析与错误分类 —— 纯逻辑模块，零外部依赖。
//! 独立于 tauri/reqwest，可被 `rustc` 单独编译验证（本机 tauri cargo check 死结的绕行方案）。

/// 增量式 SSE 解析器：逐行喂数据，产出完整事件的 data 载荷。
/// 只关心 `data:` 行；事件以空行分隔，但这里按行处理即可满足
/// OpenAI 兼容端点与 Anthropic 的流式格式（每个 data 行是一个独立 JSON）。
#[derive(Default)]
pub struct SseParser {
    linebuf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            linebuf: Vec::new(),
        }
    }

    /// 喂一段原始字节；返回其中完成的 data 载荷（可能 0~N 个）。
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        self.linebuf.extend_from_slice(chunk);
        while let Some(pos) = self.linebuf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.linebuf.drain(..=pos).collect();
            let mut len = line.len() - 1; // 去掉 \n
            if len > 0 && line[len - 1] == b'\r' {
                len -= 1; // 去掉 \r
            }
            let text = String::from_utf8_lossy(&line[..len]);
            if let Some(payload) = extract_data(&text) {
                out.push(payload);
            }
        }
        out
    }

    /// 流结束但缓冲里还有半行（对 SSE 正常是空）；有 data 就吐出来。
    pub fn flush(&mut self) -> Vec<String> {
        if self.linebuf.is_empty() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&self.linebuf).into_owned();
        self.linebuf.clear();
        match extract_data(&text) {
            Some(p) => vec![p],
            None => Vec::new(),
        }
    }
}

/// 一行 SSE 文本 → data 载荷；非 data 行返回 None。
fn extract_data(line: &str) -> Option<String> {
    let rest = line.strip_prefix("data:")?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    Some(rest.to_string())
}

/// OpenAI 兼容流式增量：choices[0].delta.content。
/// 显式忽略 reasoning_content（DeepSeek V4 等思考模型先吐思考再吐正文）。
pub fn openai_delta(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("error").is_some() {
        return None; // 错误载荷交给 error_payload 处理
    }
    let delta = &v["choices"][0]["delta"];
    let content = delta.get("content")?.as_str()?;
    Some(content.to_string())
}

/// OpenAI 兼容流式工具调用分片：一次 delta 可携带 tool_calls 数组的若干元素，
/// 按 `index` 对号入座——`id`/`function.name` 首帧出现，`function.arguments`
/// 是增量字符串片段（调用方累积拼接）。v0.13.0 Agent 工作台。
#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_fragment: Option<String>,
}

/// OpenAI 兼容流式工具调用增量：choices[0].delta.tool_calls[*]。
pub fn openai_tool_delta(payload: &str) -> Vec<ToolCallDelta> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    if v.get("error").is_some() {
        return Vec::new(); // 错误载荷交给 error_payload 处理
    }
    let Some(calls) = v["choices"][0]["delta"]["tool_calls"].as_array() else {
        return Vec::new();
    };
    calls
        .iter()
        .filter_map(|c| {
            Some(ToolCallDelta {
                index: c.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize,
                id: c.get("id").and_then(|x| x.as_str()).map(String::from),
                name: c
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|x| x.as_str())
                    .map(String::from),
                arguments_fragment: c
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|x| x.as_str())
                    .map(String::from),
            })
        })
        .collect()
}

/// 该帧的 finish_reason 是否宣告工具调用收尾。
pub fn openai_finished_tool_calls(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v["choices"][0]["finish_reason"]
                .as_str()
                .map(|s| s == "tool_calls")
        })
        .unwrap_or(false)
}

/// OpenAI 兼容 usage 帧（stream_options.include_usage 的收尾帧：choices 空 + usage）。
/// 返回 (prompt_tokens, completion_tokens)。
pub fn openai_usage(payload: &str) -> Option<(u64, u64)> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let usage = v.get("usage")?;
    let p = usage.get("prompt_tokens")?.as_u64()?;
    let c = usage.get("completion_tokens")?.as_u64()?;
    Some((p, c))
}

/// Anthropic message_start 帧的 input_tokens。
pub fn anthropic_usage_in(payload: &str) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("type")?.as_str()? != "message_start" {
        return None;
    }
    v["message"]["usage"]["input_tokens"].as_u64()
}

/// Anthropic message_delta 帧的 output_tokens（累计值，直接取即可）。
pub fn anthropic_usage_out(payload: &str) -> Option<u64> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("type")?.as_str()? != "message_delta" {
        return None;
    }
    v["usage"]["output_tokens"].as_u64()
}

/// Anthropic 流式增量：content_block_delta 事件的 delta.text。
pub fn anthropic_delta(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    if v.get("type")?.as_str()? != "content_block_delta" {
        return None;
    }
    let delta = &v["delta"];
    if delta.get("type")?.as_str()? != "text_delta" {
        return None;
    }
    Some(delta.get("text")?.as_str()?.to_string())
}

/// 流中出现的错误载荷 → 人类可读信息（OpenAI 的 {"error":{...}} 或 Anthropic 的 error 事件）。
pub fn error_payload(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let err = v.get("error")?;
    let msg = err
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("未知错误");
    Some(msg.to_string())
}

/// 错误四分类 → (一句话标题, 行动建议)。
pub fn classify_error(status: Option<u16>, raw: &str) -> (String, String) {
    let short: String = raw.chars().take(200).collect();
    match status {
        Some(401) | Some(403) => (
            "API Key 无效或无权限".into(),
            "到「模型中心」重新粘贴正确的 Key".into(),
        ),
        Some(404) => (
            "端点或模型名不存在".into(),
            "检查 Base URL 与模型名拼写（免费档推荐 glm-4-flash）".into(),
        ),
        Some(429) => (
            "触发限流或额度不足".into(),
            "稍等半分钟再试，或换一个免费档模型".into(),
        ),
        Some(s) if (500..=599).contains(&s) => {
            (format!("模型服务端暂时故障（{s}）"), "稍后再试一次".into())
        }
        _ => {
            let lowered = raw.to_lowercase();
            if lowered.contains("timed out") || lowered.contains("timeout") {
                ("请求超时".into(), "网络较慢或服务拥堵，请重试".into())
            } else if lowered.contains("dns error")
                || lowered.contains("connect")
                || lowered.contains("network")
                || lowered.contains("代理")
            {
                (
                    "网络连接失败".into(),
                    "检查网络/代理后重试；本地模型请确认 Ollama 已启动".into(),
                )
            } else if lowered.contains("eof") || lowered.contains("reset") {
                ("连接被中断".into(), "服务端提前断开，请重试一次".into())
            } else {
                ("请求失败".into(), format!("详情：{short}"))
            }
        }
    }
}

// ===================== 独立验证（cargo 不可用时的 rustc 单测） =====================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_splits_lines_and_data() {
        let mut p = SseParser::new();
        let got = p.feed(b"data: {\"a\":1}\n\ndata: [DONE]\n");
        assert_eq!(got, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn sse_handles_crlf_and_split_chunks() {
        let mut p = SseParser::new();
        assert!(p.feed(b"data: {\"x").is_empty());
        let got = p.feed(b"\":1}\r\ndata: two\n");
        assert_eq!(got, vec!["{\"x\":1}".to_string(), "two".to_string()]);
    }

    #[test]
    fn openai_delta_skips_reasoning() {
        assert_eq!(
            openai_delta(r#"{"choices":[{"delta":{"reasoning_content":"想"}}]}"#),
            None
        );
        assert_eq!(
            openai_delta(r#"{"choices":[{"delta":{"content":"你好"}}]}"#),
            Some("你好".into())
        );
    }

    #[test]
    fn anthropic_delta_shape() {
        assert_eq!(
            anthropic_delta(
                r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}"#
            ),
            Some("Hi".into())
        );
        assert_eq!(
            anthropic_delta(r#"{"type":"message_start","delta":{}}"#),
            None
        );
    }

    #[test]
    fn openai_tool_delta_merges_shards() {
        // 首帧：id+name；次帧：arguments 增量片段（主流网关的真实形态）
        let first = openai_tool_delta(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"list_projects","arguments":""}}]}}]}"#,
        );
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].index, 0);
        assert_eq!(first[0].id.as_deref(), Some("call_1"));
        assert_eq!(first[0].name.as_deref(), Some("list_projects"));
        assert_eq!(first[0].arguments_fragment.as_deref(), Some(""));

        let second = openai_tool_delta(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"id\":"}}]}}]}"#,
        );
        assert_eq!(second[0].id, None);
        assert_eq!(second[0].arguments_fragment.as_deref(), Some("{\"id\":"));
    }

    #[test]
    fn openai_tool_delta_ignores_plain_frames() {
        assert!(openai_tool_delta(r#"{"choices":[{"delta":{"content":"你好"}}]}"#).is_empty());
        assert!(openai_tool_delta(r#"{"choices":[{"delta":{}}]}"#).is_empty());
        assert!(openai_tool_delta("[DONE]").is_empty());
        assert!(openai_tool_delta(r#"{"error":{"message":"x"}}"#).is_empty());
    }

    #[test]
    fn openai_finish_reason_tool_calls() {
        assert!(openai_finished_tool_calls(
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]}"#
        ));
        assert!(!openai_finished_tool_calls(
            r#"{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#
        ));
        assert!(openai_tool_delta(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"m","arguments":"{}"}}]},"finish_reason":null}]}"#
        )[0]
            .index
            == 1);
    }

    #[test]
    fn openai_usage_frame() {
        assert_eq!(
            openai_usage(r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":58}}"#),
            Some((120, 58))
        );
        assert_eq!(openai_usage(r#"{"choices":[{"delta":{"content":"x"}}]}"#), None);
    }

    #[test]
    fn anthropic_usage_frames() {
        assert_eq!(
            anthropic_usage_in(
                r#"{"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#
            ),
            Some(25)
        );
        assert_eq!(
            anthropic_usage_out(
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":142}}"#
            ),
            Some(142)
        );
        assert_eq!(anthropic_usage_in(r#"{"type":"content_block_delta"}"#), None);
    }

    #[test]
    fn classify_covers_basics() {
        assert!(classify_error(Some(401), "").1.contains("Key"));
        assert!(classify_error(Some(429), "").0.contains("限流"));
        assert_eq!(classify_error(None, "dns error").0, "网络连接失败");
    }
}
