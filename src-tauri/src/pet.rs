//! 桌面宠物（Ori / Era，PLAN 用户需求 2026-10-26）：透明置顶小窗，
//! 双角色可选、可拖拽、点击台词、对话接 L7（key 不出 Rust）。
//!
//! Godot 融合的 Studio 侧延伸（F 系列精神）：创作伴侣不只是面板里的
//! 对话框——它可以悬浮在桌面上陪着创作者。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 打开（或聚焦）桌面宠物窗口。`character`: "ori" | "era"。
#[tauri::command]
pub fn open_pet(app: AppHandle, character: String) -> Result<(), String> {
    let character = if character == "era" { "era" } else { "ori" };
    if let Some(w) = app.get_webview_window("pet") {
        // 已开窗：切换角色只改 localStorage 由前端自理；这里重新导航
        // 到带角色参数的地址即可（前端读 query）。
        let _ = w.eval(&format!(
            "window.dispatchEvent(new CustomEvent('pet-character', {{ detail: '{character}' }}))"
        ));
        let _ = w.set_focus();
        return Ok(());
    }
    let (w, h) = if character == "era" { (240.0, 330.0) } else { (250.0, 430.0) };
    WebviewWindowBuilder::new(
        &app,
        "pet",
        WebviewUrl::App(format!("pet.html?c={character}").into()),
    )
    .title("OriEra Pet")
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .shadow(false)
    .inner_size(w, h)
    .build()
    .map_err(|e| format!("open pet window: {e}"))?;
    Ok(())
}

/// 拖拽宠物窗口（pet.html mousedown 时调用——独立窗口无装饰，拖拽
/// 需显式触发 start_dragging）。
#[tauri::command]
pub fn pet_start_drag(app: AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("pet")
        .ok_or_else(|| "pet window not found".to_string())?;
    w.start_dragging()
        .map_err(|e| format!("start dragging: {e}"))
}

/// 关闭宠物窗口。
#[tauri::command]
pub fn pet_close(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("pet") {
        w.close().map_err(|e| format!("close pet: {e}"))?;
    }
    Ok(())
}

/// 宠物对话：Rust 侧直接调已配置的 L7 provider（key 不出 Rust 域）。
/// 未配置模型时返回 rule 兜底台词提示（前端有自己的离线台词库）。
#[tauri::command]
pub async fn pet_chat(
    app: AppHandle,
    character: String,
    message: String,
) -> Result<String, String> {
    let cfg = crate::settings::load_provider(&app);
    let Some(cfg) = cfg else {
        return Err("NO_PROVIDER".into());
    };
    if cfg.key.is_empty() || cfg.kind == "rule" {
        return Err("NO_PROVIDER".into());
    }
    let persona = if character == "era" {
        "你是 Era，OriEra 桌面漂浮助手，白色圆润身体、蓝色发光眼睛。性格温柔治愈、简洁、偶尔幽默。用中文回复，一句话以内（最多两句）。"
    } else {
        "你是 Ori，OriEra 桌面机械助手，橙棕工业风、履带底座、蓝色屏幕眼。性格可靠直接、工程师气质、偶尔冷幽默。用中文回复，一句话以内（最多两句）。"
    };
    let base = if cfg.base.trim().is_empty() {
        match cfg.kind.as_str() {
            "zhipu" => "https://open.bigmodel.cn/api/paas/v4".to_string(),
            "deepseek" => "https://api.deepseek.com".to_string(),
            _ => format!("https://api.{}.com", cfg.kind),
        }
    } else {
        cfg.base.trim_end_matches('/').to_string()
    };
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": persona},
            {"role": "user", "content": message}
        ],
        "max_tokens": 120,
        "temperature": 0.8
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cfg.key))
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!(" pet chat: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let reply = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("……")
        .to_string();
    Ok(reply)
}
