//! 玩法/叙事模板库：内置（编译进二进制）+ 用户目录（%APPDATA%/cn.oriera.studio/templates）合并。
//! 用户可用「导入玩法包」把自己的 JSON 加入库中——v0.2 UGC 起点。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const BUILTIN: &str = include_str!("../templates-builtin.json");

#[derive(Serialize, Deserialize)]
pub struct TemplateLib {
    pub gameplay: Vec<Value>,
    pub story_frameworks: Vec<Value>,
    #[serde(default)]
    pub user_pack_files: Vec<String>,
    /// 解析失败的用户包（文件名 + 原因），前端提示用
    #[serde(default)]
    pub broken: Vec<(String, String)>,
}

fn user_templates_dir(app: &AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("templates");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[tauri::command]
pub fn list_templates(app: AppHandle) -> TemplateLib {
    let mut lib: TemplateLib = serde_json::from_str(BUILTIN).unwrap_or(TemplateLib {
        gameplay: vec![],
        story_frameworks: vec![],
        user_pack_files: vec![],
        broken: vec![],
    });

    let dir = user_templates_dir(&app);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let fname = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
                {
                    Ok(v) => {
                        // 单包 or 包数组都接受
                        match v.as_array() {
                            Some(arr) => lib.gameplay.extend(arr.iter().cloned()),
                            None => lib.gameplay.push(v),
                        }
                        lib.user_pack_files.push(fname);
                    }
                    Err(e) => lib.broken.push((fname, e)),
                }
            }
        }
    }
    lib
}

/// 校验一个玩法包的必要字段。
fn validate_pack(v: &Value) -> Result<(), String> {
    let s = |k: &str| v[k].as_str().map(|s| !s.is_empty()).unwrap_or(false);
    let arr = |k: &str| v[k].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    if !(s("id")
        && s("name")
        && s("tagline")
        && s("desc")
        && arr("core_loop")
        && arr("mechanics")
        && arr("story_beats")
        && arr("agents"))
    {
        return Err(
            "缺少必要字段：id/name/tagline/desc/core_loop/mechanics/story_beats/agents".into(),
        );
    }
    if v["story_beats"][0]["title"].is_null() || v["agents"][0]["name"].is_null() {
        return Err("story_beats 或 agents 内部结构不完整".into());
    }
    Ok(())
}

/// 导入玩法包：原生文件选择器 → 校验 → 存入用户模板目录。
#[tauri::command]
pub async fn import_gameplay_pack(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(file) = app
        .dialog()
        .file()
        .add_filter("玩法包 JSON", &["json"])
        .blocking_pick_file()
    else {
        return Ok(String::new()); // 用户取消
    };
    let path = file.into_path().map_err(|e| format!("路径无效：{e}"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取失败：{e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("不是合法 JSON：{e}"))?;

    let packs: Vec<&Value> = match v.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![&v],
    };
    if packs.is_empty() {
        return Err("包为空".into());
    }
    for p in &packs {
        validate_pack(p)?;
    }

    let dir = user_templates_dir(&app);
    let name = packs
        .first()
        .and_then(|p| p["id"].as_str())
        .map(|id| format!("{id}.json"))
        .unwrap_or_else(|| "imported.json".into());
    std::fs::write(
        dir.join(&name),
        serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入失败：{e}"))?;
    let label = packs
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect::<Vec<_>>()
        .join("、");
    Ok(format!("已导入玩法包：{label}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 评审 R2 PM-P0：内置包曾因 last-platform-filmgame 的 story_beats
    /// 写成字符串数组（其余模板为 {phase,title,desc}）导致向导第 5 步
    /// 被 serde 拒——金路径断裂且内置包从不跑 validate_pack。此测试把
    /// 全部内置包钉进 CI：任何形状漂移立即红。
    #[test]
    fn every_builtin_pack_passes_validate_pack() {
        let v: Value = serde_json::from_str(BUILTIN).expect("templates-builtin.json 合法 JSON");
        let packs = v["gameplay"].as_array().expect("gameplay 数组");
        assert!(packs.len() >= 17, "内置包数量异常: {}", packs.len());
        for p in packs {
            validate_pack(p).unwrap_or_else(|e| {
                panic!("内置包 {} 校验失败: {e}", p["id"].as_str().unwrap_or("?"))
            });
        }
        let fw = v["story_frameworks"].as_array().expect("story_frameworks 数组");
        assert!(!fw.is_empty(), "story_frameworks 不得为空");
    }

    /// 双源漂移陷阱（QA-P1）：内置 JSON 与前端 gameplay-packs.ts 的包 id
    /// 集合必须一致——export-templates.mjs 以 ts 为真源重打包，缺 id 即
    /// 静默删模板。
    #[test]
    fn builtin_ids_match_frontend_packs() {
        let v: Value = serde_json::from_str(BUILTIN).unwrap();
        let mut rust_ids: Vec<String> = v["gameplay"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["id"].as_str().map(str::to_string))
            .collect();
        rust_ids.sort();
        // 前端单源在仓库内相对路径（src/data/gameplay-packs.ts）
        let ts = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/data/gameplay-packs.ts"
        ))
        .expect("读 gameplay-packs.ts");
        let mut ts_ids: Vec<String> = ts
            .lines()
            .filter_map(|l| l.trim().strip_prefix("id: "))
            .filter_map(|l| l.split('"').nth(1).map(str::to_string))
            .collect();
        ts_ids.sort();
        assert_eq!(rust_ids, ts_ids, "内置 JSON 与前端包 id 集合漂移");
    }
}
