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
        let fw = v["story_frameworks"]
            .as_array()
            .expect("story_frameworks 数组");
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

    /// settle 规则只支持这些形态："*" 通配、"变量>=非负整数"，以及 W3 第二判据
    /// "A>=n && B>=m"（逐段求值取与，与 runtime evalRule 一致）。
    fn valid_settle_rule(rule: &Value) -> bool {
        match rule.as_str() {
            Some("*") => true,
            Some(s) => s
                .trim()
                .split("&&")
                .all(|part| match part.trim().split_once(">=") {
                    Some((var, num)) => {
                        !var.is_empty()
                            && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                            && num.trim().parse::<u64>().is_ok()
                    }
                    None => false,
                }),
            _ => false,
        }
    }

    /// #120 影游可执行腿：末班站台的 countdown/ending/settle 字段与 runtime
    /// （runtime/index.html）的约定钉进 CI；同时校验所有包的扩展字段一旦出现
    /// 必须是 runtime 认得的类型（runtime 按字段存在性守卫，缺省走旧路径）。
    #[test]
    fn film_template_runtime_legs_shapes() {
        let v: Value = serde_json::from_str(BUILTIN).unwrap();
        let packs = v["gameplay"].as_array().unwrap();
        let film = packs
            .iter()
            .find(|p| p["id"] == "last-platform-filmgame")
            .expect("影游模板 last-platform-filmgame 存在");

        // economy 必须是资源数组：向导 economyNames.map 与 Rust Vec<EconomyResource>
        // 反序列化都依赖数组形状（旧的对象形 {"变量": "..."} 会让创建项目直接失败）
        assert!(film["economy"].is_array(), "影游模板 economy 必须是数组");

        let beats = film["story_beats"].as_array().unwrap();
        assert!(beats.len() >= 5, "影游模板至少 5 拍");

        // 「选择一 / 选择三」两拍：countdown = 8（秒）且 choices 非空、label 必填
        for i in [1usize, 3] {
            assert_eq!(
                beats[i]["countdown"].as_i64(),
                Some(8),
                "第 {} 拍 countdown 应为 8",
                i + 1
            );
            let chs = beats[i]["choices"]
                .as_array()
                .unwrap_or_else(|| panic!("第 {} 拍倒计时必须有 choices", i + 1));
            assert!(!chs.is_empty(), "第 {} 拍 choices 不能为空", i + 1);
            for c in chs {
                assert!(c["label"].is_string(), "choice.label 必须是字符串");
            }
        }

        // choice effect 形如「变量名±整数」（runtime parseEffect / 变量袋共用该形态）
        for b in beats {
            for c in b["choices"].as_array().unwrap_or(&vec![]) {
                if let Some(eff) = c["effect"].as_str() {
                    let t = eff.trim();
                    let (name, sign, num) = match t.rfind(['+', '-']) {
                        Some(idx) => (&t[..idx], &t[idx..idx + 1], &t[idx + 1..]),
                        None => panic!("effect {eff:?} 缺少 +/-"),
                    };
                    assert!(
                        !name.is_empty()
                            && (sign == "+" || sign == "-")
                            && num.parse::<u64>().is_ok(),
                        "effect {eff:?} 形状应为「变量名±整数」"
                    );
                }
            }
        }

        // W1 影游体验批：第 0/2 拍补了带 video 的氛围拍——必须是相对路径形态
        //（runtime W4 白名单只放行相对路径与 /assets/、/projects/ 前缀），
        // 且与 runtime/assets/clips/ 下的内置占位片同名（create_project 随项目落盘）
        for (i, want) in [
            (0usize, "clips/rain-intro.mp4"),
            (2usize, "clips/confrontation.mp4"),
        ] {
            assert_eq!(
                beats[i]["video"].as_str(),
                Some(want),
                "第 {} 拍 video 应为占位片相对路径 {want}",
                i + 1
            );
        }

        // 末拍 ending 节点：ending=true + settle 对象（值全为合法规则，且兜底 "*" 存在）
        let last = beats.last().unwrap();
        assert_eq!(last["ending"].as_bool(), Some(true), "末拍必须 ending=true");
        let settle = last["settle"]
            .as_object()
            .expect("settle 必须是对象（分支名 → 规则）");
        assert!(!settle.is_empty(), "settle 不能为空");
        for (branch, rule) in settle {
            assert!(
                valid_settle_rule(rule),
                "settle.{branch} = {rule} 不是合法规则（支持 \"*\"、\"变量>=数值\" 与 \"&&\" 复合）"
            );
        }
        assert!(
            settle.values().any(|r| r.as_str() == Some("*")),
            "settle 必须含 \"*\" 兜底分支，避免规则全不命中时无结局"
        );
        // W3：trust 接入 settle——win 规则必须是「affinity>=2 && trust>=1」复合第二判据
        assert_eq!(
            settle["win"].as_str(),
            Some("affinity>=2 && trust>=1"),
            "win 规则应含 trust 第二判据（&& 复合）"
        );

        // W3 差分评语：ending_lines 支持「分支@规则」键——@前必须是 settle 已声明分支，
        // @后必须是合法规则（且不许是 "*"——通配差分没有意义，直接写基础分支即可）
        if let Some(lines) = last["ending_lines"].as_object() {
            for (k, v) in lines {
                assert!(v.is_string(), "ending_lines.{k} 必须是字符串");
                if let Some(at) = k.find('@') {
                    let (branch, rule) = (&k[..at], &k[at + 1..]);
                    assert!(
                        settle.contains_key(branch),
                        "差分评语键 {k} 的分支未在 settle 声明"
                    );
                    assert!(
                        rule != "*" && valid_settle_rule(&Value::String(rule.to_string())),
                        "差分评语键 {k} 的规则非法"
                    );
                }
            }
        }

        // 全部包：可选扩展字段一旦出现，类型必须与 runtime 约定一致
        for p in packs {
            for b in p["story_beats"].as_array().unwrap_or(&vec![]) {
                if !b["countdown"].is_null() {
                    assert!(
                        b["countdown"].is_number() && b["countdown"].as_f64().unwrap() > 0.0,
                        "包 {} 某拍 countdown 必须是正数",
                        p["id"].as_str().unwrap_or("?")
                    );
                }
                if !b["ending"].is_null() {
                    assert!(
                        b["ending"].is_boolean(),
                        "包 {} 某拍 ending 必须是布尔",
                        p["id"].as_str().unwrap_or("?")
                    );
                }
                if !b["video"].is_null() {
                    assert!(
                        b["video"].is_string(),
                        "包 {} 某拍 video 必须是字符串",
                        p["id"].as_str().unwrap_or("?")
                    );
                }
                if !b["settle"].is_null() {
                    assert!(
                        b["settle"]
                            .as_object()
                            .map(|o| o.values().all(valid_settle_rule))
                            .unwrap_or(false),
                        "包 {} 某拍 settle 形状非法",
                        p["id"].as_str().unwrap_or("?")
                    );
                }
            }
        }
    }
}
