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
    /// 素材类别：image（插画，M1）| mesh（3D 模型，Rodin）。旧 meta 无此
    /// 字段时默认 image——全部历史资产都是插画。
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "image".into()
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
        kind: "image".into(),
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

// ====================== M2 供给侧：Hyper3D Rodin 文生 3D ======================
//
// wire 逐条核实 docs.hyper3d.ai（2026-10-23，与引擎 oriera-genai/rodin.rs
// 同一契约）：POST /rodin（multipart+Bearer，201 含顶层 uuid 与
// jobs.subscription_key 两个不同标识符）→ POST /status（subscription_key）
// 状态 Waiting/Generating/Done/Failed → POST /download（顶层 uuid）list[]
// 按 .glb 取签名 URL（会过期，立即 GET，不带 Authorization）。
//
// key 纪律：Rodin Key 由前端传入（localStorage 记忆），只进本函数内存，
// 不写配置、不写日志、不进 meta.json。

const RODIN_POLL_SECS: u64 = 5;
const RODIN_MAX_POLLS: u32 = 60; // 5 分钟上限（生成通常 1-3 分钟）

fn rodin_multipart(prompt: &str) -> (String, Vec<u8>) {
    const BOUNDARY: &str = "oriera-rodin-f3a9";
    let mut body = Vec::new();
    for (name, value) in [
        ("prompt", prompt),
        ("geometry_file_format", "glb"),
        ("tier", "Low"),
        ("quality", "medium"),
    ] {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\ncontent-disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    (BOUNDARY.into(), body)
}

/// Hyper3D Rodin 文生 3D：prompt → GLB 字节 → 存入资产库（kind=mesh）。
pub async fn generate_mesh(
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    key: String,
    base: Option<String>,
) -> Result<AssetMeta, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("需要 Hyper3D API Key（hyper3d.ai 控制台申请，国内站选 hyper3d.cn）".into());
    }
    let base = base
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| "https://api.hyper3d.com/api/v2".into())
        .trim_end_matches('/')
        .to_string();
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("先描述你想要的 3D 模型".into());
    }

    let client = reqwest::Client::new();
    let (boundary, body) = rodin_multipart(&prompt);
    let resp = client
        .post(format!("{base}/rodin"))
        .header("Authorization", format!("Bearer {key}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .timeout(std::time::Duration::from_secs(60))
        .body(body)
        .send()
        .await
        .map_err(|e| format!("创建 Rodin 任务失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        let short: String = text.chars().take(300).collect();
        let hint = match status.as_u16() {
            401 => "（Key 无效或未开通 API 权限）",
            429 => "（触发限流，稍后再试）",
            _ => "",
        };
        return Err(format!("Rodin 返回 {}{}：{}", status.as_u16(), hint, short));
    }
    let created: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析创建响应失败：{e}"))?;
    let uuid = created["uuid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Rodin 未受理任务：{}", text.chars().take(200).collect::<String>()))?
        .to_string();
    let subkey = created["jobs"]["subscription_key"]
        .as_str()
        .ok_or_else(|| "Rodin 响应缺 jobs.subscription_key".to_string())?
        .to_string();

    // 轮询状态：任一 Failed 即失败；全 Done 才进下载。
    let mut done = false;
    for _ in 0..RODIN_MAX_POLLS {
        tokio::time::sleep(std::time::Duration::from_secs(RODIN_POLL_SECS)).await;
        let resp = client
            .post(format!("{base}/status"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&serde_json::json!({ "subscription_key": subkey }))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("查询状态失败：{e}"))?;
        let st: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析状态失败：{e}"))?;
        let jobs = st["jobs"]
            .as_array()
            .ok_or_else(|| "Rodin 状态响应缺 jobs 数组".to_string())?;
        let mut all_done = true;
        for j in jobs {
            match j["status"].as_str() {
                Some("Done") => {}
                Some("Waiting") | Some("Generating") => all_done = false,
                Some(other) => return Err(format!("Rodin 任务失败：{other}")),
                None => return Err("Rodin 状态响应缺 status 字段".into()),
            }
        }
        if all_done && !jobs.is_empty() {
            done = true;
            break;
        }
    }
    if !done {
        return Err(format!(
            "生成超时（超 {} 次轮询）——任务可能仍在排队，稍后重试",
            RODIN_MAX_POLLS
        ));
    }

    // 下载：body 用顶层 uuid（不是 subscription_key）。
    let resp = client
        .post(format!("{base}/download"))
        .header("Authorization", format!("Bearer {key}"))
        .json(&serde_json::json!({ "task_uuid": uuid }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("请求下载清单失败：{e}"))?;
    let dl: serde_json::Value = resp.json().await.map_err(|e| format!("解析下载清单失败：{e}"))?;
    let glb_url = dl["list"]
        .as_array()
        .and_then(|list| {
            list.iter().find_map(|e| {
                let name = e["name"].as_str()?;
                if name.to_ascii_lowercase().ends_with(".glb") {
                    e["url"].as_str().map(str::to_string)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| "Rodin 下载清单中没有 .glb 条目".to_string())?;

    // 签名 URL 会过期：立即取回（不带鉴权头）。
    let bytes = client
        .get(&glb_url)
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("下载 GLB 失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("下载 GLB 内容失败：{e}"))?
        .to_vec();

    // 落盘 + 入库（GLB 可能几 MB——页面按 mesh 类别渲染占位卡）。
    let dir = assets_dir(&state);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ts = now_ts();
    let file = format!("{ts}-rodin.glb");
    std::fs::write(dir.join(&file), &bytes).map_err(|e| format!("写入 GLB 失败：{e}"))?;
    let meta = AssetMeta {
        id: format!("a{ts}"),
        file: format!("assets/{file}"),
        prompt: prompt.chars().take(120).collect(),
        created: ts.to_string(),
        kind: "mesh".into(),
    };
    let mut list = read_meta(&state);
    list.insert(0, meta.clone());
    write_meta(&state, &list)?;
    Ok(meta)
}

#[tauri::command]
pub async fn generate_mesh_command(
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    key: String,
    base: Option<String>,
) -> Result<AssetMeta, String> {
    generate_mesh(state, prompt, key, base).await
}

// ====================== 影游供给侧：火山方舟 Seedance 文生视频 ======================
//
// wire 按 R0/M1 实测（2026-10-24/25，interfilm/docs/R0-report.md，与引擎
// oriera-genai/seedance.rs 同一契约）：POST {base}/api/v3/contents/
// generations/tasks（Bearer + JSON：model/content[text]/ratio/duration/
// watermark:false——水印必须显式关）→ {id}；GET tasks/{id} 轮询 status
// queued|running|succeeded|failed|cancelled；succeeded 取
// content.video_url（公网签名 URL，立即 GET 且不带 Authorization）。
// 实测耗时 82~212s/条（5s 720p），计费 ~108.9k tokens/条（mini 档）。
//
// key 纪律：与 Rodin 同族——方舟 Key 由前端传入（localStorage 记忆），
// 只进本函数内存，不写配置、不写日志、不进 meta.json。

const SEEDANCE_POLL_SECS: u64 = 10;
const SEEDANCE_MAX_POLLS: u32 = 90; // 15 分钟上限（实测 1.5~3.5 分钟）
const SEEDANCE_TOLERATED_BLIPS: u32 = 3; // 轮询网络抖动容忍次数（评审 R2 性能 P1）
const SEEDANCE_DEFAULT_MODEL: &str = "doubao-seedance-2-0-mini-260615"; // 草稿档（¥2.50/5s@720p 实测）

/// 视频任务的在途记录（评审 R2 AI原生/性能/PM 三方 P1：task_id 只活在
/// 函数内存，超时/关窗即丢 → 重试=双倍扣费）。落盘 assets/pending.json；
/// **刻意不含 Key**——续查用本次调用传入的 Key，纪律与主链一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingVideo {
    pub task_id: String,
    pub prompt: String,
    pub base: String,
    pub model: String,
    pub ratio: String,
    pub started: u64,
}

fn pending_path(state: &tauri::State<'_, crate::AppState>) -> std::path::PathBuf {
    assets_dir(state).join("pending.json")
}

fn read_pending(state: &tauri::State<'_, crate::AppState>) -> Vec<PendingVideo> {
    std::fs::read_to_string(pending_path(state))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_pending(state: &tauri::State<'_, crate::AppState>, list: &[PendingVideo]) {
    if list.is_empty() {
        let _ = std::fs::remove_file(pending_path(state));
        return;
    }
    let _ = std::fs::create_dir_all(assets_dir(state));
    if let Ok(bytes) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(pending_path(state), bytes);
    }
}

/// reqwest 错误脱敏（对照 #79 教训）：完整 URL 可能带 query 凭据形态，
/// 只留 host 与错误类别。
fn short_err(e: &reqwest::Error) -> String {
    let host = e.url().and_then(|u| u.host_str()).unwrap_or("?");
    format!("{host}: {e}")
}

/// 火山方舟 Seedance 文生视频：prompt → MP4 字节 → 资产库（kind=video）。
pub async fn generate_video(
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    key: String,
    base: Option<String>,
    model: Option<String>,
    ratio: Option<String>,
) -> Result<AssetMeta, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("需要火山方舟 API Key（console.volcengine.com/ark 控制台创建）".into());
    }
    let base = base
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| "https://ark.cn-beijing.volces.com".into())
        .trim_end_matches('/')
        .to_string();
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("先描述你想要的镜头（例如：雨夜地铁站台，镜头缓推，霓虹反射在湿地面上）".into());
    }
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| SEEDANCE_DEFAULT_MODEL.into());
    let ratio = ratio.filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "16:9".into());

    // 防双倍扣费（评审 R2 三方 P1）：同 prompt 有在途任务 → 续查而不是
    // 再创建。Key 用本次传入的（纪律：pending 不存 Key）。
    let existing = read_pending(&state)
        .into_iter()
        .find(|t| t.prompt == prompt);
    let client = reqwest::Client::new();
    let task_id = if let Some(pending) = existing {
        println!("[assets] 续查在途视频任务 {}（不重复创建）", pending.task_id);
        pending.task_id.clone()
    } else {
        create_video_task(&client, &base, &key, &model, &ratio, &prompt, &state)?
    };

    // 轮询：queued|running 继续；succeeded 取 video_url；failed/cancelled 报错。
    let video_url = poll_video_task(&client, &base, &key, &task_id).await?;

    // 公网签名 URL：立即取回（不带鉴权头，会过期）。
    let bytes = client
        .get(&video_url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载视频失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取视频内容失败：{e}"))?
        .to_vec();

    // 任务完结：清在途记录（成功路径；终态失败在 poll 内已报错，pending
    // 里的该条同样清除——供应商终态不可续查）。
    {
        let mut pend = read_pending(&state);
        pend.retain(|t| t.task_id != task_id);
        write_pending(&state, &pend);
    }

    // 落盘 + 入库（页面按 video 类别用 <video> 渲染）。
    let dir = assets_dir(&state);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ts = now_ts();
    let file = format!("{ts}-seedance.mp4");
    std::fs::write(dir.join(&file), &bytes).map_err(|e| format!("写入 MP4 失败：{e}"))?;
    let meta = AssetMeta {
        id: format!("a{ts}"),
        file: format!("assets/{file}"),
        prompt: prompt.chars().take(120).collect(),
        created: ts.to_string(),
        kind: "video".into(),
    };
    let mut list = read_meta(&state);
    list.insert(0, meta.clone());
    write_meta(&state, &list)?;
    Ok(meta)
}

/// 创建 Seedance 任务并落 pending（防双倍扣费）。
async fn create_video_task(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    model: &str,
    ratio: &str,
    prompt: &str,
    state: &tauri::State<'_, crate::AppState>,
) -> Result<String, String> {
    let resp = client
        .post(format!("{base}/api/v3/contents/generations/tasks"))
        .header("Authorization", format!("Bearer {key}"))
        .json(&serde_json::json!({
            "model": model,
            "content": [ { "type": "text", "text": prompt } ],
            "ratio": ratio,
            "duration": 5,
            "watermark": false
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("创建 Seedance 任务失败：{}", short_err(&e)))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        let short: String = text.chars().take(300).collect();
        let hint = match status.as_u16() {
            401 => "（Key 无效）",
            403 => "（该模型未开通：方舟控制台 → 开通管理 → 开通 Seedance 系列需余额 ≥200 元）",
            429 => "（触发限流，稍后再试）",
            _ => "",
        };
        return Err(format!("Seedance 返回 {}{}：{}", status.as_u16(), hint, short));
    }
    let created: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析创建响应失败：{e}"))?;
    let task_id = created["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("方舟未受理任务：{}", text.chars().take(200).collect::<String>()))?
        .to_string();
    let mut pend = read_pending(state);
    pend.retain(|t| t.prompt != prompt);
    pend.push(PendingVideo {
        task_id: task_id.clone(),
        prompt: prompt.to_string(),
        base: base.to_string(),
        model: model.to_string(),
        ratio: ratio.to_string(),
        started: now_ts(),
    });
    write_pending(state, &pend);
    Ok(task_id)
}

/// 轮询 Seedance 任务到终态（带网络抖动容忍，评审 R2 性能 P1）。
async fn poll_video_task(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    task_id: &str,
) -> Result<String, String> {
    let mut blips = 0u32;
    for _ in 0..SEEDANCE_MAX_POLLS {
        tokio::time::sleep(std::time::Duration::from_secs(SEEDANCE_POLL_SECS)).await;
        // 抖动容忍：连续 SEEDANCE_TOLERATED_BLIPS 次网络/解析失败才放弃，
        // 单次抖动不再报废已付费任务。
        let v = match client
            .get(format!("{base}/api/v3/contents/generations/tasks/{task_id}"))
            .header("Authorization", format!("Bearer {key}"))
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(r) => match r.json::<serde_json::Value>().await {
                Ok(v) => v,
                Err(e) => {
                    blips += 1;
                    if blips >= SEEDANCE_TOLERATED_BLIPS {
                        return Err(format!("解析状态连续失败：{}", short_err(&e)));
                    }
                    continue;
                }
            },
            Err(e) => {
                blips += 1;
                if blips >= SEEDANCE_TOLERATED_BLIPS {
                    return Err(format!("查询状态连续失败：{}", short_err(&e)));
                }
                continue;
            }
        };
        blips = 0;
        match v["status"].as_str().unwrap_or_default() {
            "queued" | "running" => {}
            "succeeded" => {
                let url = v["content"]["video_url"].as_str().map(str::to_string);
                return url.ok_or_else(|| "任务成功但响应缺 video_url".to_string());
            }
            other => {
                let detail = v["error"]["message"].as_str().unwrap_or("无详情");
                return Err(format!("生成失败（{other}）：{detail}"));
            }
        }
    }
    // 超时：pending 保留——同 prompt 重新生成将续查而非重复扣费
    Err(format!(
        "生成超时（超 {} 次轮询）——在途任务已记录，同描述重新生成将续查而非重复扣费",
        SEEDANCE_MAX_POLLS
    ))
}

#[tauri::command]
pub async fn generate_video_command(
    state: tauri::State<'_, crate::AppState>,
    prompt: String,
    key: String,
    base: Option<String>,
    model: Option<String>,
    ratio: Option<String>,
) -> Result<AssetMeta, String> {
    generate_video(state, prompt, key, base, model, ratio).await
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
    // 评审 R2 玩家 P0：视频/3D 模型挂进画廊墙会静默加载失败（runtime 的
    // art.json 走 TextureLoader 只认图片），还白占坑位——后端诚实拒绝。
    if asset.kind != "image" {
        return Err(format!(
            "画廊墙只挂插画；{} 类资产的场景挂载随素材中心 M2 跟进（当前可导出收藏）",
            if asset.kind == "video" { "视频" } else { "3D 模型" }
        ));
    }

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
