//! Demo 目录与打开逻辑：元数据 + webview 窗口 + provider 注入脚本。

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Clone, Serialize)]
pub struct DemoInfo {
    pub id: String,
    pub title: String,
    pub desc: String,
    pub tag: String,
    pub featured: bool,
}

/// 内置 demo 的中文元数据（id 与 examples/ 目录名一致）。
/// title 取自各 demo README/index.html，desc 为中文简介。
const DEMOS: &[( &str, &str, &str, &str, bool)] = &[
    ("01-hello-triangle", "01 · Hello Triangle", "WebGPU 渲染管线最小示例：一个三角形的完整渲染闭环，L5 层的「Hello World」。", "渲染 · WebGPU", false),
    ("02-ecs-spawn", "02 · ECS Spawn", "实体组件系统演示：实体的生成与销毁，L3 ECS 层数据驱动架构入门。", "ECS 架构", false),
    ("03-physics", "03 · Physics", "物理沙盒：刚体运动、碰撞与作用力，实时物理模拟体验。", "物理模拟", false),
    ("04-agent-chat", "04 · Agent Chat", "Agent 对话演示：规则引擎驱动的 NPC 意图理解与应答（离线即可运行）。", "AI Agent", false),
    ("05-gltf-model", "05 · glTF Model", "glTF 3D 模型加载：Three.js 渲染带动画的 3D 角色，L4 资产层示例。", "3D 资产", false),
    ("06-agent-physics", "06 · Agent Physics", "Agent × 物理联动：用自然语言指令指挥物理世界（「生成 20 个」「关掉重力」）。", "AI × 物理", false),
    ("07-webgpu-particles", "07 · WebGPU 1M Particles", "WebGPU 百万粒子系统：GPU 计算驱动的粒子流场，考验渲染上限。", "渲染 · WebGPU", false),
    ("08-ecs-visualizer", "08 · ECS Visualizer", "ECS 可视化：实时观察实体-组件-系统的数据流转，理解引擎架构。", "ECS 架构", false),
    ("09-full-arch", "09 · Full Architecture", "全架构串联演示：从 PAL 到渲染的完整分层协作，一次看懂引擎全貌。", "全架构", false),
    ("10-word-spirit", "10 · 言灵世界 Word Spirit", "AI 原生小游戏：文字驱动世界生成与演化，「所说即所见」的世界模型雏形。", "AI 原生玩法", false),
    ("11-ai-cafe", "11 · 隅角咖啡馆", "旗舰示例：3D 世界里的 AI NPC——会记忆、会聊天、会推进剧情。配置模型后 NPC 由大模型驱动，拥有长期记忆与导演系统。", "AI NPC · 旗舰", true),
    ("12-memory-atlas", "12 · 记忆图谱 Memory Atlas", "长期记忆可视化：MemoryStore 的检索式记忆以图谱呈现——写入、衰减、召回一览。", "L9 记忆", false),
    ("13-genai-gallery", "13 · 生成式画廊 GenAI Gallery", "一句话装帧世界：L8 生成管线（文生图/文生3D）+ L4 命名热替换，配 Key 后海报与穹顶实时生成。", "L8 生成", false),
];

pub fn catalog() -> Vec<DemoInfo> {
    DEMOS
        .iter()
        .map(|(id, title, desc, tag, featured)| DemoInfo {
            id: id.to_string(),
            title: title.to_string(),
            desc: desc.to_string(),
            tag: tag.to_string(),
            featured: *featured,
        })
        .collect()
}

/// 用户 demo 清单（%APPDATA%/cn.oriera.studio/demos/<id>/demo.json）。
/// 与内置同字段 + entry（入口相对路径，默认 index.html）——影游工作流的
/// 导出 zip 解压进来配一份 manifest 即可上架 Demo 厅。
#[derive(Debug, Clone, serde::Deserialize)]
struct UserDemo {
    title: String,
    desc: String,
    tag: String,
    #[serde(default)]
    featured: bool,
    #[serde(default = "default_entry")]
    entry: String,
}

fn default_entry() -> String {
    "index.html".into()
}

pub fn user_demos_dir(app: &AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("demos")
}

fn user_entry(app: &AppHandle, id: &str) -> Option<String> {
    let manifest = user_demos_dir(app).join(id).join("demo.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let ud: UserDemo = serde_json::from_str(&text).ok()?;
    Some(normalize_entry(&ud.entry))
}

/// 目录形态（无扩展名）补尾斜杠：ServeDir 对无斜杠目录发 307 时 Location
/// 会丢 nest 前缀（/demos/last-platform/player → 127.0.0.1:…/last-platform/…），
/// 自定义协议窗口跟随即 404。文件形态（含 .）原样返回。
fn normalize_entry(entry: &str) -> String {
    let e = entry.trim_matches('/');
    if e.contains('.') { e.to_string() } else { format!("{e}/") }
}

/// 内置 + 用户合并目录。用户 id 与内置冲突时内置优先（不可被覆盖）。
fn catalog_merged(app: &AppHandle) -> Vec<DemoInfo> {
    let mut out = catalog();
    out.extend(scan_user_dir(&user_demos_dir(app), &out));
    out
}

fn scan_user_dir(dir: &std::path::Path, builtin: &[DemoInfo]) -> Vec<DemoInfo> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    let mut out = Vec::new();
    for path in entries {
        let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !path.is_dir() || builtin.iter().any(|b| b.id == id) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path.join("demo.json")) else {
            continue;
        };
        let Ok(ud) = serde_json::from_str::<UserDemo>(&text) else {
            continue;
        };
        out.push(DemoInfo {
            id: id.to_string(),
            title: ud.title,
            desc: ud.desc,
            tag: ud.tag,
            featured: ud.featured,
        });
    }
    out
}

#[tauri::command]
pub fn list_demos(app: AppHandle) -> Vec<DemoInfo> {
    catalog_merged(&app)
}

#[tauri::command]
pub fn open_demo(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    folder: String,
) -> Result<(), String> {
    // 白名单校验（内置表 + 用户 manifest），杜绝路径穿越
    let merged = catalog_merged(&app);
    let Some(demo) = merged.iter().find(|d| d.id == folder) else {
        return Err(format!("unknown demo: {folder}"));
    };

    // 用户 demo（manifest 有 entry 者）走 /demos/{id}/{entry}；内置走引擎快照
    let user_entry = user_entry(&app, &folder);
    let url: tauri::Url = if let Some(entry) = &user_entry {
        format!("http://oriera.localhost/demos/{}/{}", folder, entry)
    } else {
        format!("http://oriera.localhost/examples/{}/", folder)
    }
    .parse()
    .map_err(|e| format!("bad url: {e}"))?;

    let title = format!("OriEra · {}", demo.title);

    let label = format!("demo-{folder}");
    // 已存在同名窗口则先关闭，保证注入脚本重新生效
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }

    // 用户上传多为竖屏影游（工作流导出物），给手机比例窗口
    let mut builder = if user_entry.is_some() {
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url.clone()))
            .title(&title)
            .inner_size(560.0, 940.0)
            .min_inner_size(420.0, 720.0)
    } else {
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url.clone()))
            .title(&title)
            .inner_size(1280.0, 860.0)
            .min_inner_size(960.0, 640.0)
    };
    // 同 open_project：不挂 initialization_script，避免新版 WebView2 吞初始导航
    let win = builder.build().map_err(|e| e.to_string())?;
    // 同 open_project：新版 WebView2 下显式补导航，防 about:blank 空窗
    let _ = win.navigate(url.clone());
    if crate::settings::load(&app).developer_mode {
        win.open_devtools();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oriera-demos-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn user_demo_manifest_defaults_entry() {
        let json = r#"{"title":"末班站台","desc":"互动影游","tag":"影游"}"#;
        let ud: UserDemo = serde_json::from_str(json).unwrap();
        assert_eq!(ud.entry, "index.html");
        assert!(!ud.featured);
        let json2 = r#"{"title":"x","desc":"y","tag":"t","featured":true,"entry":"player/"}"#;
        let ud2: UserDemo = serde_json::from_str(json2).unwrap();
        assert_eq!(ud2.entry, "player/");
        assert!(ud2.featured);
        // 治本：目录形态补尾斜杠（307 丢前缀坑），文件形态原样
        assert_eq!(normalize_entry("player/"), "player/");
        assert_eq!(normalize_entry("player"), "player/");
        assert_eq!(normalize_entry("/player/"), "player/");
        assert_eq!(normalize_entry("player/index.html"), "player/index.html");
    }

    #[test]
    fn user_demo_scan_merges_skips_builtin_and_broken() {
        let root = tmp_root("scan");
        let d = root.join("demos");
        // 正常用户 demo
        std::fs::create_dir_all(d.join("last-platform")).unwrap();
        std::fs::write(
            d.join("last-platform").join("demo.json"),
            r#"{"title":"末班站台","desc":"互动影游样例","tag":"互动影游 · 旗舰","featured":true,"entry":"player/"}"#,
        )
        .unwrap();
        // 坏 manifest：跳过不致命
        std::fs::create_dir_all(d.join("broken")).unwrap();
        std::fs::write(d.join("broken").join("demo.json"), "{ not json").unwrap();
        // 无 manifest 目录：跳过
        std::fs::create_dir_all(d.join("no-manifest")).unwrap();

        let merged = scan_user_dir(&d, &catalog());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "last-platform");
        assert!(merged[0].featured);
        assert_eq!(merged[0].tag, "互动影游 · 旗舰");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 与内置冲突的用户 id 被让位：内置不可覆盖。
    #[test]
    fn user_demo_cannot_shadow_builtin() {
        let root = tmp_root("shadow");
        let d = root.join("demos");
        std::fs::create_dir_all(d.join("11-ai-cafe")).unwrap();
        std::fs::write(
            d.join("11-ai-cafe").join("demo.json"),
            r#"{"title":"冒牌咖啡馆","desc":"x","tag":"t"}"#,
        )
        .unwrap();
        let merged = scan_user_dir(&d, &catalog());
        assert!(merged.iter().all(|m| m.id != "11-ai-cafe"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// 用系统默认浏览器打开 demo（webview 子窗口受 WebView2 运行时兼容性影响时兜底）。
#[tauri::command]
pub fn open_demo_in_browser(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    folder: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let user_entry = user_entry(&app, &folder);
    if catalog_merged(&app).iter().all(|d| d.id != folder) {
        return Err(format!("unknown demo: {folder}"));
    }
    let url = match &user_entry {
        Some(entry) => format!("http://127.0.0.1:{}/demos/{}/{}", state.port, folder, entry),
        None => format!("http://127.0.0.1:{}/examples/{}/", state.port, folder),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("打开浏览器失败：{e}"))
}
