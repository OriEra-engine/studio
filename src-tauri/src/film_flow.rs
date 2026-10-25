//! 影游制作工作流（P1 建游的向导形态）：状态落盘 + 豆包 TTS-2.0 配音 + 组装导出。
//! 管线来自《末班站台》实战（interfilm/），wire 事实见 interfilm/docs/POSTMORTEM.md M2.2 段：
//! 单 header `X-Api-Key` + `X-Api-Resource-Id: seed-tts-2.0`，SSE 成功码 20000000。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const PLAYER_TEMPLATE: &str = include_str!("../templates-film-player.html");

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct FilmLocks {
    pub protagonist: String,
    pub supporting: String,
    pub style: String,
}

/// 单条配音的声线绑定（role → 豆包 2.0 speaker id）
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct VoiceBinding {
    pub role: String,
    pub speaker: String,
}

/// 工作流状态 —— 每步完成度全部落在项目目录 film/flow.json，跨会话可续作。
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct FilmFlowState {
    pub step: u32,
    pub locks: FilmLocks,
    pub story: String,
    pub voices: Vec<VoiceBinding>,
    /// shotId（`{node}_{i}`）→ 素材库相对路径（assets/xxx.mp4）
    pub clips: BTreeMap<String, String>,
    /// voId（`{node}_{i}` / `{ending}_0`）→ film/vo 下文件名
    pub vo: BTreeMap<String, String>,
    pub exported_zip: Option<String>,
}

fn film_dir(state: &tauri::State<'_, crate::AppState>, project: &str) -> PathBuf {
    state.projects_dir.join(project).join("film")
}

fn flow_path(state: &tauri::State<'_, crate::AppState>, project: &str) -> PathBuf {
    film_dir(state, project).join("flow.json")
}

#[tauri::command]
pub fn film_flow_load(
    state: tauri::State<'_, crate::AppState>,
    project: String,
) -> Option<FilmFlowState> {
    std::fs::read_to_string(flow_path(&state, &project))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

#[tauri::command]
pub fn film_flow_save(
    state: tauri::State<'_, crate::AppState>,
    project: String,
    flow: FilmFlowState,
) -> Result<(), String> {
    let dir = film_dir(&state, &project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 film 目录失败：{e}"))?;
    let json = serde_json::to_string_pretty(&flow).map_err(|e| e.to_string())?;
    std::fs::write(flow_path(&state, &project), json).map_err(|e| format!("写入失败：{e}"))
}

/// 手写 base64 解码（SSE data 分片 → mp3 字节）。标准字母表，容忍空白与 padding。
fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut vals = Vec::with_capacity(s.len());
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("base64 非法字符 {b}")),
        };
        vals.push(v);
    }
    let mut out = Vec::with_capacity(vals.len() * 3 / 4);
    for chunk in vals.chunks(4) {
        let n = chunk.len();
        if n == 0 {
            continue;
        }
        let tri = (chunk[0] as u32) << 18
            | (*chunk.get(1).unwrap_or(&0) as u32) << 12
            | (*chunk.get(2).unwrap_or(&0) as u32) << 6
            | (*chunk.get(3).unwrap_or(&0) as u32);
        out.push((tri >> 16) as u8);
        if n >= 3 {
            out.push((tri >> 8) as u8);
        }
        if n >= 4 {
            out.push(tri as u8);
        }
    }
    Ok(out)
}

/// 豆包语音合成 2.0（单向流式 SSE）。key 只走参数不落盘；产物写 film/vo/{vo_id}.mp3。
#[tauri::command]
pub async fn tts_doubao(
    state: tauri::State<'_, crate::AppState>,
    project: String,
    key: String,
    speaker: String,
    text: String,
    vo_id: String,
) -> Result<String, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("需要豆包语音 API Key（console.volcengine.com/speech/new 创建，区别于方舟 Key）".into());
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("台词为空".into());
    }
    let safe_vo: String = vo_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if safe_vo.is_empty() {
        return Err("voId 非法".into());
    }
    let speaker_id = if speaker.trim().is_empty() {
        "zh_male_shenyeboke_uranus_bigtts".to_string()
    } else {
        speaker.trim().to_string()
    };

    let body = serde_json::json!({
        "user": { "uid": "oriera-studio" },
        "req_params": {
            "text": text,
            "speaker": speaker_id,
            "audio_params": { "format": "mp3", "sample_rate": 24000, "bit_rate": 160000 }
        }
    });
    let client = reqwest::Client::new();
    let resp = client
        .post("https://openspeech.bytedance.com/api/v3/tts/unidirectional/sse")
        .header("X-Api-Key", &key)
        .header("X-Api-Resource-Id", "seed-tts-2.0")
        .header("X-Api-Request-Id", uuid_v4())
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| format!("请求豆包语音失败：{e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let head = &body[..body.len().min(200)];
        return Err(format!("豆包语音 HTTP {status}：{head}"));
    }

    // SSE：逐行找 data: 前缀，code∈{0,20000000} 视为成功，data 是 base64 音频分片
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut audio: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取音频流失败：{e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let payload = &line[5..];
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
                continue;
            };
            match v.get("code").and_then(|c| c.as_i64()) {
                Some(0) | Some(20000000) => {}
                Some(c) => {
                    let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
                    return Err(format!("豆包语音返回 {c}：{msg}"));
                }
                None => {}
            }
            if let Some(b64) = v.get("data").and_then(|d| d.as_str()) {
                audio.extend(b64_decode(b64)?);
            }
        }
    }
    if audio.is_empty() {
        return Err("音频流为空（检查 Key 是否已开通「豆包语音合成2.0」资源）".into());
    }
    if audio.len() < 512 {
        return Err(format!("音频过短（{} 字节），疑似鉴权或参数问题", audio.len()));
    }

    let dir = film_dir(&state, &project).join("vo");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 vo 目录失败：{e}"))?;
    let file = format!("{safe_vo}.mp3");
    std::fs::write(dir.join(&file), &audio).map_err(|e| format!("写入 mp3 失败：{e}"))?;
    println!(
        "[film_flow] TTS {safe_vo} ← {speaker_id}，{} 字符 {} 字节",
        text.chars().count(),
        audio.len()
    );
    Ok(file)
}

fn uuid_v4() -> String {
    // 无 uuid crate：时间戳 + 进程内计数足够做 Request-Id（非安全用途）
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{ms:x}-{seq:x}")
}

/// 渲染播放器：占位符两侧带引号，注入 JSON 字符串字面量。
fn render_player(story: &serde_json::Value, clips: &BTreeMap<String, String>, vo: &BTreeMap<String, String>) -> Result<String, String> {
    let vo_map: BTreeMap<String, String> = vo
        .iter()
        .map(|(k, v)| (k.clone(), format!("vo/{v}")))
        .collect();
    let html = PLAYER_TEMPLATE
        .to_string()
        .replace(
            "\"__STORY__\"",
            &serde_json::to_string(story).map_err(|e| e.to_string())?,
        )
        .replace(
            "\"__CLIPS__\"",
            &serde_json::to_string(clips).map_err(|e| e.to_string())?,
        )
        .replace(
            "\"__VOS__\"",
            &serde_json::to_string(&vo_map).map_err(|e| e.to_string())?,
        );
    Ok(html)
}

/// 组装导出：播放器模板注入 story/clips/vo → film/export/ 下 zip。
/// 素材缺失时播放器自动氛围兜底（CSS 雨幕/场景色），导出不因此失败。
#[tauri::command]
pub fn export_film(
    state: tauri::State<'_, crate::AppState>,
    project: String,
    flow: FilmFlowState,
) -> Result<String, String> {
    use std::io::Write as _;
    let story: serde_json::Value =
        serde_json::from_str(&flow.story).map_err(|e| format!("story.json 无效：{e}"))?;

    // clips：素材库相对路径（assets/xxx.mp4）→ zip 内归位为 clips/{shotId}.mp4
    let assets_root = state
        .projects_dir
        .parent()
        .map(|p| p.join("assets"))
        .unwrap_or_else(|| PathBuf::from("assets"));
    let clips_src: BTreeMap<String, PathBuf> = flow
        .clips
        .iter()
        .map(|(shot, rel)| (shot.clone(), assets_root.join(rel.trim_start_matches("assets/"))))
        .collect();

    // 渲染播放器并写入 zip
    let html = render_player(&story, &flow.clips, &flow.vo)?;

    // zip 组装
    let export_dir = film_dir(&state, &project).join("export");
    std::fs::create_dir_all(&export_dir).map_err(|e| format!("创建 export 目录失败：{e}"))?;
    let title = story
        .get("meta")
        .and_then(|m| m.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("interactive-film")
        .to_string();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let zip_path = export_dir.join(format!("{}-{}.zip", sanitize(&title), stamp));
    let file = std::fs::File::create(&zip_path).map_err(|e| format!("创建 zip 失败：{e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    let plain = zip::write::SimpleFileOptions::default();
    let media = zip::write::SimpleFileOptions::default().large_file(true);

    zw.start_file("index.html", plain).map_err(|e| e.to_string())?;
    zw.write_all(html.as_bytes())
        .map_err(|e| format!("写入 index.html 失败：{e}"))?;

    zw.start_file("story.json", plain).map_err(|e| e.to_string())?;
    let story_pretty =
        serde_json::to_string_pretty(&story).map_err(|e| e.to_string())?;
    zw.write_all(story_pretty.as_bytes()).map_err(|e| e.to_string())?;

    for (shot, src) in &clips_src {
        if let Ok(bytes) = std::fs::read(src) {
            zw.start_file(format!("clips/{shot}.mp4"), media)
                .map_err(|e| e.to_string())?;
            zw.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    let vo_dir = film_dir(&state, &project).join("vo");
    for fname in flow.vo.values() {
        if let Ok(bytes) = std::fs::read(vo_dir.join(fname)) {
            zw.start_file(format!("vo/{fname}"), plain)
                .map_err(|e| e.to_string())?;
            zw.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    zw.start_file("玩法说明.txt", plain).map_err(|e| e.to_string())?;
    let readme = format!(
        "{}\n\n解压后双击 index.html 即玩（视频/配音缺失的镜头自动氛围兜底）。\n镜头 {} 条 · 配音 {} 条\n由 OriEra Studio 影游工作流导出。",
        title,
        clips_src.len(),
        flow.vo.len()
    );
    zw.write_all(readme.as_bytes()).map_err(|e| e.to_string())?;
    zw.finish().map_err(|e| format!("收尾 zip 失败：{e}"))?;

    println!(
        "[film_flow] 导出 {}（clips {}/vo {}）",
        zip_path.display(),
        clips_src.len(),
        flow.vo.len()
    );
    Ok(zip_path.to_string_lossy().into_owned())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '/' && *c != '\\')
        .take(24)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_decode_golden() {
        assert_eq!(b64_decode("QUJDREVGRw==").unwrap(), b"ABCDEFG");
        assert_eq!(b64_decode("YWJj").unwrap(), b"abc");
        assert_eq!(b64_decode("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo=").unwrap(),
            b"abcdefghijklmnopqrstuvwxyz");
        // 二进制边界：2 字节 → 3 字符，1 字节 → 2 字符
        assert_eq!(b64_decode("AAEC").unwrap(), vec![0x00, 0x01, 0x02]);
        assert_eq!(b64_decode("AAE=").unwrap(), vec![0x00, 0x01]);
        assert!(b64_decode("A?B").is_err());
    }

    #[test]
    fn render_player_injects_and_leaves_no_placeholder() {
        let story: serde_json::Value = serde_json::from_str(
            r#"{"meta":{"title":"末班站台"},"vars":{},"nodes":[{"id":"s1","type":"scene","title":"夜"}]}"#,
        )
        .unwrap();
        let mut clips = BTreeMap::new();
        clips.insert("s1_0".into(), "assets/clip-x.mp4".into());
        let mut vo = BTreeMap::new();
        vo.insert("s1_0".into(), "s1_0.mp3".into());
        let html = render_player(&story, &clips, &vo).unwrap();
        assert!(!html.contains("__STORY__"), "STORY 占位符必须被替换");
        assert!(!html.contains("__CLIPS__"));
        assert!(!html.contains("__VOS__"));
        assert!(html.contains("末班站台"));
        assert!(html.contains("assets/clip-x.mp4"));
        assert!(html.contains("vo/s1_0.mp3"));
    }
}
