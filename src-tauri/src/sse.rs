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
        Self { linebuf: Vec::new() }
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
        Some(s) if (500..=599).contains(&s) => (
            format!("模型服务端暂时故障（{s}）"),
            "稍后再试一次".into(),
        ),
        _ => {
            let lowered = raw.to_lowercase();
            if lowered.contains("timed out") || lowered.contains("timeout") {
                ("请求超时".into(), "网络较慢或服务拥堵，请重试".into())
            } else if lowered.contains("dns error")
                || lowered.contains("connect")
                || lowered.contains("network")
                || lowered.contains("代理")
            {
                ("网络连接失败".into(), "检查网络/代理后重试；本地模型请确认 Ollama 已启动".into())
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
            anthropic_delta(r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}"#),
            Some("Hi".into())
        );
        assert_eq!(
            anthropic_delta(r#"{"type":"message_start","delta":{}}"#),
            None
        );
    }

    #[test]
    fn classify_covers_basics() {
        assert!(classify_error(Some(401), "").1.contains("Key"));
        assert!(classify_error(Some(429), "").0.contains("限流"));
        assert_eq!(classify_error(None, "dns error").0, "网络连接失败");
    }
}
