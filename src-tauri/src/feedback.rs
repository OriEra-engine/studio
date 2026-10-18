//! 用户反馈链路：桌面端 → OriEra 世界引擎仓库（github.com/OriEra-engine/engine）issues。
//! 双通道，按是否配置 GitHub PAT 自动选择：
//! 1) 直提（协作者配置了 PAT）→ GitHub REST API 创建 issue
//! 2) 兜底（普通用户，无 PAT）→ 返回预填好的 issue URL（前端用系统浏览器打开）
//!    并把反馈存成 markdown 文件，供无 GitHub/无网络条件的用户手动投递。

use crate::hardware;
use serde::Serialize;
use tauri::Manager;

pub const FEEDBACK_REPO: &str = "OriEra-engine/engine";
const KEYRING_SERVICE: &str = "OriEraStudio";

#[derive(Serialize)]
pub struct FeedbackResult {
    /// "direct" = 已直提 issue；"manual" = 需浏览器/手动投递
    pub mode: String,
    /// direct: 新建的 issue 地址；manual: 预填好的 issue new 地址
    pub url: String,
    /// manual: 本地存档的反馈文件路径
    pub file: String,
}

fn feedback_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, "feedback-github-pat").map_err(|e| format!("凭据管理器不可用：{e}"))
}

fn pat_get() -> Result<String, String> {
    match feedback_entry()?.get_password() {
        Ok(v) => Ok(v),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!("读取凭据失败：{e}")),
    }
}

fn pat_set(v: &str) -> Result<(), String> {
    match feedback_entry()?.set_password(v) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("写入凭据失败：{e}")),
    }
}

fn pat_del() -> Result<(), String> {
    match feedback_entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除凭据失败：{e}")),
    }
}

fn system_footer() -> String {
    let hw = hardware::detect();
    let gpu = hw
        .gpus
        .first()
        .map(|g| {
            format!(
                "{} ({})",
                g.name,
                match g.vram_mb {
                    Some(mb) => format!("{mb}MB"),
                    None => "显存未知".into(),
                }
            )
        })
        .unwrap_or_else(|| "未检测到".into());
    format!(
        "\n\n---\n**环境信息**（OriEra Studio 自动附带）\n- Studio 版本：v{}\n- 系统：{}\n- GPU：{}\n- 内存：{} GB\n- 反馈来源：OriEra Studio 桌面端",
        env!("CARGO_PKG_VERSION"),
        hw.os,
        gpu,
        hw.ram_gb,
    )
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 提交反馈。kind: bug / idea / question / other。
#[tauri::command]
pub async fn submit_feedback(
    app: tauri::AppHandle,
    kind: String,
    title: String,
    body: String,
) -> Result<FeedbackResult, String> {
    let title = format!("[Studio][{}] {}", kind_label(&kind), title.trim());
    let title = if title.chars().count() > 110 {
        format!("{}…", title.chars().take(110).collect::<String>())
    } else {
        title
    };
    if title.trim().is_empty() {
        return Err("标题不能为空".into());
    }
    let full_body = format!("{}\n{}", body.trim(), system_footer());

    let pat = pat_get().unwrap_or_default();
    if !pat.is_empty() {
        // 直提通道
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("https://api.github.com/repos/{FEEDBACK_REPO}/issues"))
            .header("Authorization", format!("Bearer {pat}"))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "OriEraStudio-Feedback")
            .json(&serde_json::json!({ "title": title, "body": full_body }))
            .send()
            .await
            .map_err(|e| format!("请求 GitHub 失败（需要能访问 api.github.com）：{e}"))?;
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        if status.is_success() {
            let html_url = serde_json::from_str::<serde_json::Value>(&txt)
                .ok()
                .and_then(|v| v.get("html_url").and_then(|u| u.as_str()).map(String::from))
                .unwrap_or_default();
            return Ok(FeedbackResult { mode: "direct".into(), url: html_url, file: String::new() });
        }
        return Err(format!("GitHub 返回 {status}：{}", txt.chars().take(300).collect::<String>()));
    }

    // 兜底通道：预填 URL + 本地存档
    let url = format!(
        "https://github.com/{}/issues/new?title={}&body={}",
        FEEDBACK_REPO,
        urlencode(&title),
        urlencode(&full_body),
    );
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("feedback");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = dir.join(format!("feedback-{ts}.md"));
    let content = format!("# {title}\n\n{full_body}\n\n> 请将本文件内容投递至 https://github.com/{FEEDBACK_REPO}/issues 或发送给 OriEra 社区。\n");
    std::fs::write(&file, content).map_err(|e| e.to_string())?;
    Ok(FeedbackResult {
        mode: "manual".into(),
        url,
        file: file.to_string_lossy().into_owned(),
    })
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "bug" => "问题反馈",
        "idea" => "功能建议",
        "question" => "使用咨询",
        _ => "其他",
    }
}

/// 是否已配置反馈 PAT（协作者直提通道）。
#[tauri::command]
pub fn has_feedback_pat() -> bool {
    !pat_get().unwrap_or_default().is_empty()
}

#[tauri::command]
pub fn save_feedback_pat(pat: String) -> Result<(), String> {
    if pat.trim().is_empty() {
        return pat_del();
    }
    pat_set(pat.trim())
}

#[tauri::command]
pub fn clear_feedback_pat() -> Result<(), String> {
    pat_del()
}
