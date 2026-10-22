//! AI 素材中心（M1）：智谱 CogView 插画生成 + 本地资产库 + 项目画廊墙挂载。
//! 资产落盘 %APPDATA%/cn.oriera.studio/assets/，由 axum 与 oriera 协议同源服务。

use crate::settings::ProviderCfg;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeta {
    pub id: String,
    pub file: String,   // 形如 assets/1737500000-a1b2.png（对页面是 /assets/...）
    pub prompt: String,
    pub created: String,
}

fn assets_dir(state: &tauri::State<'_, crate::AppState>) -> std::path::PathBuf {
    // 与 projects 同级：app_data/assets
    state
        .projects_dir
        .parent()
        .map(|p| p.join("assets"))
        .unwrap_or_else(|| std::path::PathBuf::from("assets"))
}

fn meta_path(state: &tauri::State<'_, crate::AppState>) -> std::path::PathBuf {
    assets_dir(state).join("meta.json")
}

fn read_meta(state: &tauri::State<'_, crate::AppState>) -> Vec<AssetMeta> {
    std::fs::read_to_string(meta_path(state))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_meta(state: &tauri::State<'_, crate::AppState>, list: &[AssetMeta]) -> Result<(), String> {
    std::fs::create_dir_all(assets_dir(state)).map_err(|e| e.to_string())?;
    std::fs::write(
        meta_path(state),
        serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 智谱 CogView 插画生成：prompt → 图片字节 → 存入资产库。
/// 仅支持智谱（CogView 走 OpenAI images 方言）；其他厂商给出明确指引。
pub async fn generate_asset(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    model: Option<String>,
) -> Result<AssetMeta, String> {
    let cfg = crate::settings::load_provider(&app)
        .ok_or_else(|| "插画生成需要先在模型中心配置智谱 API Key（CogView 免费档可用）".to_string())?;
    if cfg.kind != "zhipu" {
        return Err("插画生成当前仅支持智谱 CogView——请在模型中心选择智谱并填入 API Key".into());
    }

    let base = if cfg.base.trim().is_empty() {
        "https://open.bigmodel.cn/api/paas/v4".to_string()
    } else {
        cfg.base.trim_end_matches('/').to_string()
    };
    let url = format!("{}/images/generations", base);
    let body = json!({
        "model": model.unwrap_or_else(|| "cogview-3-flash".into()),
        "prompt": prompt,
        // cogview-3-flash 支持 1024x1024
        "size": "1024x1024",
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
        let short: String = text.chars().take(300).collect();
        return Err(format!("CogView 返回 {}：{}", status.as_u16(), short));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析响应失败：{e}"))?;
    let img_url = parsed["data"][0]["url"]
        .as_str()
        .ok_or_else(|| format!("响应中无图片 URL：{}", text.chars().take(200).collect::<String>()))?
        .to_string();

    // 下载图片（Zhipu 返回签名 URL，不带鉴权直接 GET）
    let bytes = client
        .get(&img_url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("下载图片失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("下载图片内容失败：{e}"))?
        .to_vec();

    // 落盘
    let dir = assets_dir(&state);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ts = now_ts();
    let file = format!("{ts}-cogview.png");
    std::fs::write(dir.join(&file), &bytes).map_err(|e| format!("写入图片失败：{e}"))?;

    let meta = AssetMeta {
        id: format!("a{ts}"),
        file: format!("assets/{file}"),
        prompt: prompt.chars().take(120).collect(),
        created: ts.to_string(),
    };
    let mut list = read_meta(&state);
    list.insert(0, meta.clone());
    write_meta(&state, &list)?;
    Ok(meta)
}

#[tauri::command]
pub async fn generate_asset_command(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    model: Option<String>,
) -> Result<AssetMeta, String> {
    generate_asset(app, state, prompt, model).await
}

#[tauri::command]
pub fn list_assets(state: tauri::State<'_, crate::AppState>) -> Vec<AssetMeta> {
    read_meta(&state)
}

#[tauri::command]
pub fn delete_asset(
    state: tauri::State<'_, crate::AppState>,
    id: String,
) -> Result<(), String> {
    let mut list = read_meta(&state);
    if let Some(pos) = list.iter().position(|a| a.id == id) {
        let meta = list.remove(pos);
        let file = assets_dir(&state).join(
            meta.file
                .strip_prefix("assets/")
                .unwrap_or(&meta.file),
        );
        let _ = std::fs::remove_file(file);
        write_meta(&state, &list)?;
    }
    Ok(())
}

/// 把资产挂进项目的画廊墙：写 projects/<id>/art.json（runtime 启动时读取生成挂画）。
#[tauri::command]
pub fn add_asset_to_project(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    asset_id: String,
    project_id: String,
) -> Result<(), String> {
    let list = read_meta(&state);
    let asset = list
        .iter()
        .find(|a| a.id == asset_id)
        .ok_or_else(|| format!("资产不存在：{asset_id}"))?
        .clone();

    let dir = state.projects_dir.join(&project_id);
    if !dir.join("index.html").is_file() {
        return Err(format!("项目不存在：{project_id}"));
    }
    let art_path = dir.join("art.json");
    let mut arts: Vec<serde_json::Value> = std::fs::read_to_string(&art_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    arts.push(json!({
        "file": format!("/assets/{}",
            asset.file.strip_prefix("assets/").unwrap_or(&asset.file)),
        "prompt": asset.prompt,
    }));
    std::fs::write(
        &art_path,
        serde_json::to_vec_pretty(&arts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入失败：{e}"))?;

    crate::project::reload_project_window(&app, &project_id);
    Ok(())
}

/// 供项目卡导入（ProviderCfg 仅在 zhipu 时可用）——导出给测试与前端校验用
pub fn provider_is_zhipu(cfg: &ProviderCfg) -> bool {
    cfg.kind == "zhipu"
}
