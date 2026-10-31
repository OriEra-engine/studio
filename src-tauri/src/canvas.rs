//! 世界画布（PLAN-canvas v2.2）：board.json 透明存取 + 播放器预览渲染。
//! 画布 schema 由前端 src/lib/board.ts 唯一持有，Rust 只做透明存取——
//! 防两端类型漂移（film/flow.json 同惯例）。剧情单一真源原则：画布的
//! 连边/节拍内容一律从 story 派生，board.json 只存布局、候选组、生成
//! 任务记录与看片批注。

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn canvas_dir(state: &tauri::State<'_, crate::AppState>, project: &str) -> PathBuf {
    state.projects_dir.join(project).join("canvas")
}

fn board_path(state: &tauri::State<'_, crate::AppState>, project: &str) -> PathBuf {
    canvas_dir(state, project).join("board.json")
}

/// 读取画布数据（未创建过返回 None）。schema 透明：解析失败按不存在处理，
/// 由前端进入只读模式提示，绝不静默覆盖。
#[tauri::command]
pub fn canvas_load(
    state: tauri::State<'_, crate::AppState>,
    project: String,
) -> Result<Option<Value>, String> {
    if !crate::project::valid_project_id(&project) {
        return Err(format!("非法项目 id：{project}"));
    }
    match std::fs::read_to_string(board_path(&state, &project)) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("board.json 已损坏（画布将以只读模式打开）：{e}")),
        Err(_) => Ok(None),
    }
}

/// 保存画布数据。先写临时文件再改名——断电/崩溃不会留半个 JSON。
#[tauri::command]
pub fn canvas_save(
    state: tauri::State<'_, crate::AppState>,
    project: String,
    board: Value,
) -> Result<(), String> {
    if !crate::project::valid_project_id(&project) {
        return Err(format!("非法项目 id：{project}"));
    }
    let dir = canvas_dir(&state, &project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 canvas 目录失败：{e}"))?;
    let json = serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?;
    let tmp = board_path(&state, &project).with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("写入失败：{e}"))?;
    std::fs::rename(&tmp, board_path(&state, &project)).map_err(|e| format!("提交失败：{e}"))
}

/// 渲染「完整试玩」预览 HTML：与导出 zip 的 index.html 是同一份模板
/// （WYSIWYG 底线），仅把媒体地址改写为内置服务器绝对 URL，供 iframe
/// srcdoc 内嵌。clips/vo 一律读盘上的 film/flow.json（单一真源）。
#[tauri::command]
pub fn canvas_preview_html(
    state: tauri::State<'_, crate::AppState>,
    project: String,
    story: String,
) -> Result<String, String> {
    if !crate::project::valid_project_id(&project) {
        return Err(format!("非法项目 id：{project}"));
    }
    let story: Value = serde_json::from_str(&story).map_err(|e| format!("story.json 无效：{e}"))?;
    let (clips, vo): (BTreeMap<String, String>, BTreeMap<String, String>) =
        match std::fs::read_to_string(crate::film_flow::flow_path(&state, &project)) {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .map(|f| {
                    let clips = f
                        .get("clips")
                        .and_then(|c| c.as_object())
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    let vo = f
                        .get("vo")
                        .and_then(|c| c.as_object())
                        .map(|m| {
                            m.iter()
                                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    (clips, vo)
                })
                .unwrap_or_default(),
            Err(_) => Default::default(),
        };
    let base = format!("http://127.0.0.1:{}", state.port);
    let clip_urls: BTreeMap<String, String> = clips
        .iter()
        .map(|(k, v)| {
            let url = if v.starts_with("assets/") {
                format!("{base}/{v}")
            } else {
                v.clone()
            };
            (k.clone(), url)
        })
        .collect();
    let vo_urls: BTreeMap<String, String> = vo
        .iter()
        .map(|(k, v)| (k.clone(), format!("{base}/projects/{project}/film/vo/{v}")))
        .collect();
    crate::film_flow::render_player_with_urls(&story, &clip_urls, &vo_urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_save_is_atomic_shape() {
        // 只验证 Value 往返；文件级原子写在 save 命令里（tmp + rename）
        let board = serde_json::json!({"version": 1, "nodes": []});
        let text = serde_json::to_string_pretty(&board).unwrap();
        assert!(text.contains("\"version\": 1"));
    }
}
