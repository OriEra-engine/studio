//! Ollama 本地模型链路：检测安装/运行状态、列模型、一键拉取（带进度事件）。

use serde::Serialize;
use std::time::Duration;
use tauri::Emitter;

pub const OLLAMA_BASE: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Serialize, Default)]
pub struct OllamaStatus {
    pub installed: bool,
    pub install_path: Option<String>,
    pub version: Option<String>,
    pub running: bool,
    pub models: Vec<OllamaModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaModel {
    pub name: String,
    pub size_gb: f32,
}

/// 找 ollama.exe：PATH → 默认安装路径。
fn find_ollama() -> Option<(String, String)> {
    // 1) PATH
    if let Ok(out) = std::process::Command::new("ollama").arg("--version").output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Some(("ollama".into(), v));
        }
    }
    // 2) 默认安装路径
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let p = std::path::Path::new(&local)
            .join("Programs")
            .join("Ollama")
            .join("ollama.exe");
        if p.exists() {
            if let Ok(out) = std::process::Command::new(&p).arg("--version").output() {
                if out.status.success() {
                    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    return Some((p.to_string_lossy().into_owned(), v));
                }
            }
        }
    }
    None
}

async fn api_version(client: &reqwest::Client) -> Option<String> {
    let resp = client
        .get(format!("{OLLAMA_BASE}/api/version"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    v["version"].as_str().map(|s| s.to_string())
}

async fn api_models(client: &reqwest::Client) -> Vec<OllamaModel> {
    let resp = match client
        .get(format!("{OLLAMA_BASE}/api/tags"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    v["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    Some(OllamaModel {
                        name: m["name"].as_str()?.to_string(),
                        size_gb: (m["size"].as_f64()? / 1024.0 / 1024.0 / 1024.0) as f32,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub async fn ollama_status() -> OllamaStatus {
    let mut st = OllamaStatus::default();
    if let Some((path, version)) = find_ollama() {
        st.installed = true;
        st.install_path = Some(path);
        st.version = Some(version);
    }
    let client = reqwest::Client::new();
    if let Some(v) = api_version(&client).await {
        st.running = true;
        if st.version.is_none() {
            st.version = Some(v);
        }
        st.models = api_models(&client).await;
    }
    st
}

/// 未运行时启动 `ollama serve`（后台进程）。
#[tauri::command]
pub async fn ollama_serve(app: tauri::AppHandle) -> Result<(), String> {
    let client = reqwest::Client::new();
    if api_version(&client).await.is_some() {
        return Ok(()); // 已在运行
    }
    let exe = find_ollama()
        .map(|(p, _)| p)
        .ok_or_else(|| "未检测到 Ollama，请先安装".to_string())?;

    {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new(&exe)
                .arg("serve")
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new(&exe).arg("serve").spawn();
        }
    }

    // 等待服务就绪
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if api_version(&client).await.is_some() {
            let _ = app.emit("ollama-serve-ready", ());
            return Ok(());
        }
    }
    Err("ollama serve 启动超时".into())
}

/// 拉取模型，进度通过 `ollama-pull-progress` 事件推送：{status, percent}。
#[tauri::command]
pub async fn ollama_pull(app: tauri::AppHandle, model: String) -> Result<(), String> {
    use futures_util::StreamExt;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{OLLAMA_BASE}/api/pull"))
        .json(&serde_json::json!({ "model": model }))
        .timeout(Duration::from_secs(3600))
        .send()
        .await
        .map_err(|e| format!("请求失败（Ollama 未运行？）：{e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Ollama 返回 {}", resp.status()));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断：{e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        // NDJSON：按行解析
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let status = v["status"].as_str().unwrap_or("").to_string();
                let percent = if v["total"].is_number() && v["completed"].is_number() {
                    let total = v["total"].as_f64().unwrap_or(0.0);
                    let done = v["completed"].as_f64().unwrap_or(0.0);
                    if total > 0.0 { Some((done / total * 100.0) as u32) } else { None }
                } else {
                    None
                };
                let _ = app.emit(
                    "ollama-pull-progress",
                    serde_json::json!({ "model": model, "status": status, "percent": percent }),
                );
                if v["error"].is_string() {
                    return Err(v["error"].as_str().unwrap_or("拉取失败").to_string());
                }
            }
        }
    }
    let _ = app.emit("ollama-pull-done", serde_json::json!({ "model": model }));
    Ok(())
}

/// 删除本地模型（POST /api/delete）。
#[tauri::command]
pub async fn ollama_delete_model(model: String) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{OLLAMA_BASE}/api/delete"))
        .json(&serde_json::json!({ "model": model }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("请求失败（Ollama 未运行？）：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Ollama 返回 {}", resp.status()));
    }
    Ok(())
}

/// GPU 显存占用（MB）：nvidia-smi，无 N 卡返回 None。
#[tauri::command]
pub async fn gpu_vram_usage() -> Option<(u32, u32)> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    let mut it = line.split(',');
    let used: u32 = it.next()?.trim().parse().ok()?;
    let total: u32 = it.next()?.trim().parse().ok()?;
    Some((used, total))
}
